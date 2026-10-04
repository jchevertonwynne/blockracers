//! The game's side of the lobby: the list of sessions that are being hosted, kept by
//! a small server (`crates/lobby`). The port's own.
//!
//! A host puts its session on the list and says every few seconds that it is still
//! there; a player reads the list and dials the host it picks. Nothing of a race goes
//! by way of the lobby. Asking it anything waits on the network, so it is done on
//! `transport::runtime` and the answers are picked up here a frame or more later.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use bevy::prelude::*;
use lobby_api::{BEAT_EVERY, MAX_PLAYERS, PROTOCOL, Register, Registered, Session as Listed, Status};
use reqwest::StatusCode;

use super::transport::runtime;
use super::{Role, Session, Wire};
use crate::menu::{Circuits, Screen, Settings};

/// Where the lobby is, unless `BRICK_LOBBY` says otherwise.
const LOBBY: &str = "https://lego.jchevertonwynne.uk";

#[derive(thiserror::Error, Debug)]
enum LobbyError {
    #[error("the lobby couldn't be reached: {0}")]
    Unreached(#[from] reqwest::Error),
    #[error("the lobby said {0}")]
    Refused(StatusCode),
}

enum Heard {
    List(Vec<Listed>),
    Registered(Registered),
    /// The lobby no longer has the session: it is to be listed again.
    Forgotten,
    Failed(String),
}

#[derive(Resource)]
pub struct Lobby {
    url: String,
    client: reqwest::Client,
    tell: Sender<Heard>,
    heard: Mutex<Receiver<Heard>>,
    /// The sessions on the list when it was last read.
    pub sessions: Vec<Listed>,
    /// What went wrong the last time the lobby was asked something, if anything did.
    pub trouble: Option<String>,
    /// This game's own session on the list, and whether it is being put there now.
    listed: Option<Registered>,
    listing: bool,
    /// Seconds until the lobby is next told the session is still there.
    beat: f32,
}

impl Default for Lobby {
    fn default() -> Self {
        let (tell, heard) = channel();
        // The runtime's making is also what readies the client for HTTPS.
        runtime();
        Lobby {
            url: std::env::var("BRICK_LOBBY").unwrap_or_else(|_| LOBBY.to_string()),
            client: reqwest::Client::new(),
            tell,
            heard: Mutex::new(heard),
            sessions: Vec::new(),
            trouble: None,
            listed: None,
            listing: false,
            beat: 0.0,
        }
    }
}

async fn answered(sent: Result<reqwest::Response, reqwest::Error>) -> Result<reqwest::Response, LobbyError> {
    let answer = sent?;
    if answer.status().is_success() { Ok(answer) } else { Err(LobbyError::Refused(answer.status())) }
}

impl Lobby {
    /// Does `ask` on the network's threads and keeps what it comes back with.
    fn ask(&self, ask: impl Future<Output = Result<Heard, LobbyError>> + Send + 'static) {
        let tell = self.tell.clone();
        runtime().spawn(async move {
            let heard = ask.await.unwrap_or_else(|error| match error {
                LobbyError::Refused(StatusCode::NOT_FOUND) => Heard::Forgotten,
                error => Heard::Failed(error.to_string()),
            });
            let _ = tell.send(heard);
        });
    }

    /// Reads the list afresh; `sessions` has it once the lobby has answered.
    pub fn list(&self) {
        let request = self.client.get(format!("{}/sessions?protocol={PROTOCOL}", self.url));
        self.ask(async move { Ok(Heard::List(answered(request.send().await).await?.json().await?)) });
    }

    fn register(&mut self, ask: &Register) {
        self.listing = true;
        let request = self.client.post(format!("{}/sessions", self.url)).json(ask);
        self.ask(async move { Ok(Heard::Registered(answered(request.send().await).await?.json().await?)) });
    }

    fn still_here(&self, listed: &Registered, status: &Status) {
        let request = self.client.put(format!("{}/sessions/{}", self.url, listed.id)).bearer_auth(&listed.token).json(status);
        self.ask(async move {
            answered(request.send().await).await?;
            Ok(Heard::List(Vec::new()))
        });
    }

    /// Takes this game's session off the list.
    pub fn close(&mut self) {
        if let Some(listed) = self.listed.take() {
            let request = self.client.delete(format!("{}/sessions/{}", self.url, listed.id)).bearer_auth(&listed.token);
            runtime().spawn(async move { drop(request.send().await) });
        }
    }
}

/// Picks up the lobby's answers, and keeps a hosted session on its list.
pub fn keep(
    time: Res<Time<Real>>,
    role: Res<Role>,
    screen: Res<State<Screen>>,
    session: Res<Session>,
    wire: Option<Res<Wire>>,
    settings: Res<Settings>,
    circuits: Res<Circuits>,
    mut lobby: ResMut<Lobby>,
) {
    let lobby = &mut *lobby;
    let heard: Vec<Heard> = lobby.heard.lock().map(|heard| heard.try_iter().collect()).unwrap_or_default();
    for heard in heard {
        match heard {
            // A beat's answer is an empty list, and is not the list.
            Heard::List(sessions) if *role == Role::Host => drop(sessions),
            Heard::List(sessions) => (lobby.sessions, lobby.trouble) = (sessions, None),
            Heard::Registered(listed) => (lobby.listed, lobby.listing, lobby.beat, lobby.trouble) = (Some(listed), false, BEAT_EVERY as f32, None),
            Heard::Forgotten => lobby.listed = None,
            Heard::Failed(why) => {
                warn!("{why}");
                (lobby.trouble, lobby.listing) = (Some(why), false);
            }
        }
    }
    if *role != Role::Host {
        lobby.close();
        return;
    }
    // A host is on the list from when it can be dialled until it stops hosting.
    let Some(endpoint) = wire.as_ref().and_then(|wire| wire.0.address()) else { return };
    let racing = *screen.get() != Screen::Menu;
    let status = Status {
        players: (session.members.len() + 1).min(MAX_PLAYERS as usize) as u8,
        circuit: if racing { circuits.0.get(settings.circuit).map(|c| c.name.clone()).unwrap_or_default().chars().take(lobby_api::MAX_NAME).collect() } else { String::new() },
        racing,
    };
    lobby.beat -= time.delta_secs();
    match lobby.listed.clone() {
        Some(listed) if lobby.beat <= 0.0 => {
            lobby.beat = BEAT_EVERY as f32;
            lobby.still_here(&listed, &status);
        }
        Some(_) => {}
        None if !lobby.listing && lobby.beat <= 0.0 => {
            // If the lobby can't be reached it is tried again, but not every frame.
            lobby.beat = 2.0;
            lobby.register(&Register {
                protocol: PROTOCOL,
                name: session.title.clone(),
                host: session.name.clone(),
                endpoint,
                locked: !session.password.is_empty(),
                max: MAX_PLAYERS,
                status,
            });
        }
        None => {}
    }
}
