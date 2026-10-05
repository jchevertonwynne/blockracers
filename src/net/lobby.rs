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
use lobby_api::{
    BEAT_EVERY, MAX_PLAYERS, PROTOCOL, Register, Registered, Session as Listed, Status,
};
use reqwest::StatusCode;

use super::transport::runtime;
use super::{Role, Session, Wire};
use crate::menu::{Circuits, Screen, Settings};

/// Where the lobby is, unless `BRICK_LOBBY` says otherwise.
const LOBBY: &str = "https://racers.jchevertonwynne.uk";

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
    /// The session a code is for, if it is for any.
    Found(Option<Listed>),
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
    /// What a code asked about turned out to be for, once the lobby has said: a
    /// session, or none.
    pub found: Option<Option<Listed>>,
    /// What went wrong the last time the lobby was asked something, if anything did.
    pub trouble: Option<String>,
    /// Counts up whenever either changes, for whatever draws them.
    pub revision: u32,
    /// This game's own session on the list, and whether it is being put there now.
    listed: Option<Registered>,
    listing: bool,
    /// What the list was told of the session that a beat can't change: what it is
    /// called, whether it is locked, and how many it takes.
    told: (String, bool, u8, bool),
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
            found: None,
            trouble: None,
            revision: 0,
            listed: None,
            listing: false,
            told: default(),
            beat: 0.0,
        }
    }
}

async fn answered(
    sent: Result<reqwest::Response, reqwest::Error>,
) -> Result<reqwest::Response, LobbyError> {
    let answer = sent?;
    if answer.status().is_success() {
        Ok(answer)
    } else {
        Err(LobbyError::Refused(answer.status()))
    }
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
        let request = self
            .client
            .get(format!("{}/sessions?protocol={PROTOCOL}", self.url));
        self.ask(async move {
            Ok(Heard::List(
                answered(request.send().await).await?.json().await?,
            ))
        });
    }

    /// Asks which session a code is for; `found` has the answer once there is one.
    pub fn find(&mut self, code: &str) {
        self.found = None;
        let request = self.client.get(format!(
            "{}/codes/{}?protocol={PROTOCOL}",
            self.url,
            code.trim()
        ));
        self.ask(async move {
            match answered(request.send().await).await {
                Ok(answer) => Ok(Heard::Found(Some(answer.json().await?))),
                Err(LobbyError::Refused(StatusCode::NOT_FOUND)) => Ok(Heard::Found(None)),
                Err(error) => Err(error),
            }
        });
    }

    fn register(&mut self, ask: &Register) {
        self.listing = true;
        let request = self.client.post(format!("{}/sessions", self.url)).json(ask);
        self.ask(async move {
            Ok(Heard::Registered(
                answered(request.send().await).await?.json().await?,
            ))
        });
    }

    fn still_here(&self, listed: &Registered, status: &Status) {
        let request = self
            .client
            .put(format!("{}/sessions/{}", self.url, listed.id))
            .bearer_auth(&listed.token)
            .json(status);
        self.ask(async move {
            answered(request.send().await).await?;
            Ok(Heard::List(Vec::new()))
        });
    }

    /// Takes this game's session off the list.
    pub fn close(&mut self) {
        if let Some(listed) = self.listed.take() {
            let request = self
                .client
                .delete(format!("{}/sessions/{}", self.url, listed.id))
                .bearer_auth(&listed.token);
            runtime().spawn(async move { drop(request.send().await) });
        }
    }
}

/// The game is closing: a session it hosts comes off the list first, and is not left
/// there for the lobby to give up on. Not waited on for long.
pub fn farewell(mut closing: MessageReader<AppExit>, mut lobby: ResMut<Lobby>) {
    if closing.read().next().is_none() {
        return;
    }
    if let Some(listed) = lobby.listed.take() {
        let request = lobby
            .client
            .delete(format!("{}/sessions/{}", lobby.url, listed.id))
            .bearer_auth(&listed.token);
        let _ = runtime().block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(2), request.send()).await
        });
    }
}

/// Picks up the lobby's answers, and keeps a hosted session on its list.
pub fn keep(
    time: Res<Time<Real>>,
    role: Res<Role>,
    screen: Res<State<Screen>>,
    mut session: ResMut<Session>,
    wire: Option<Res<Wire>>,
    settings: Res<Settings>,
    circuits: Res<Circuits>,
    mut lobby: ResMut<Lobby>,
) {
    let lobby = &mut *lobby;
    let heard: Vec<Heard> = lobby
        .heard
        .lock()
        .map(|heard| heard.try_iter().collect())
        .unwrap_or_default();
    for heard in heard {
        match heard {
            // A beat's answer is an empty list, and is not the list.
            Heard::List(sessions) if *role == Role::Host => drop(sessions),
            Heard::List(sessions) => {
                (lobby.sessions, lobby.trouble, lobby.revision) =
                    (sessions, None, lobby.revision + 1)
            }
            Heard::Registered(listed) => {
                session.code = listed.code.clone();
                (lobby.listed, lobby.listing, lobby.beat, lobby.trouble) =
                    (Some(listed), false, BEAT_EVERY as f32, None)
            }
            Heard::Found(found) => {
                (lobby.found, lobby.revision) = (Some(found), lobby.revision + 1)
            }
            Heard::Forgotten => lobby.listed = None,
            Heard::Failed(why) => {
                warn!("{why}");
                (lobby.trouble, lobby.listing, lobby.revision) =
                    (Some(why), false, lobby.revision + 1);
            }
        }
    }
    if *role != Role::Host {
        lobby.close();
        return;
    }
    // A host is on the list from when it can be dialled until it stops hosting.
    let Some(endpoint) = wire.as_ref().and_then(|wire| wire.0.address()) else {
        return;
    };
    let racing = *screen.get() != Screen::Menu;
    let status = Status {
        players: (session.members.len() + 1).min(MAX_PLAYERS as usize) as u8,
        circuit: if racing {
            circuits
                .0
                .get(settings.circuit)
                .map(|c| c.name.clone())
                .unwrap_or_default()
                .chars()
                .take(lobby_api::MAX_NAME)
                .collect()
        } else {
            String::new()
        },
        racing,
    };
    lobby.beat -= time.delta_secs();
    // A session the host has changed is listed afresh.
    let telling = (
        session.title.clone(),
        !session.password.is_empty(),
        session.most(),
        session.unlisted,
    );
    if lobby.listed.is_some() && lobby.told != telling {
        lobby.close();
        lobby.beat = 0.0;
    }
    match lobby.listed.clone() {
        Some(listed) if lobby.beat <= 0.0 => {
            lobby.beat = BEAT_EVERY as f32;
            lobby.still_here(&listed, &status);
        }
        Some(_) => {}
        None if !lobby.listing && lobby.beat <= 0.0 => {
            // If the lobby can't be reached it is tried again, but not every frame.
            lobby.beat = 2.0;
            lobby.told = telling;
            lobby.register(&Register {
                protocol: PROTOCOL,
                name: session.title.clone(),
                host: session.name.clone(),
                endpoint,
                locked: !session.password.is_empty(),
                max: session.most(),
                status,
                unlisted: session.unlisted,
            });
        }
        None => {}
    }
}
