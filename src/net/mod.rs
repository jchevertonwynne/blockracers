//! Racing other people over the network. The port's own: the original's second
//! player shares the screen.
//!
//! One player hosts, and their game runs the race: every car, brick and power-up is
//! decided there (`host`). The others send what they are pressing, drive their own
//! car at once on the expectation that the host will make the same of it, and are
//! shown everything else as the host last said it was (`client`). `protocol` is what
//! they say to each other and `link` what carries it.
//!
//! Online the race is stepped sixty times a second on every machine, whatever rate
//! the screen is drawn at, so that a player's game and the host step a car alike. A
//! race online is live and nothing else: it can't be paused, photographed or watched
//! again, and it is run by the session's rules and not the player's own settings.

pub mod client;
pub mod display;
pub mod host;
pub mod link;
pub mod lobby;
pub mod protocol;
pub mod state;
pub mod transport;
#[cfg(test)]
mod tests;

use bevy::prelude::*;

pub use host::Remote;
use link::{Event, Link};
use protocol::{HOST, Inputs, Peer, Refusal, Rules, Seat, Snapshot, TICKS, Tick, ToHost, ToPlayer, decode, encode};

use crate::kart::{self, Controls, Player};
#[cfg(test)]
use crate::kart::Kart;
use crate::menu::{Circuits, Screen, Settings};
use crate::{items, rules};

/// What this game is to the race.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// Racing alone, as the original is raced.
    #[default]
    Offline,
    Host,
    Client,
}

pub fn online(role: Res<Role>) -> bool {
    *role != Role::Offline
}

pub fn hosting(role: Res<Role>) -> bool {
    *role == Role::Host
}

pub fn joined(role: Res<Role>) -> bool {
    *role == Role::Client
}

/// The way to the other games. There while the game is online.
#[derive(Resource)]
pub struct Wire(pub Box<dyn Link>);

/// The step of the race this game has reached.
#[derive(Resource, Default)]
pub struct Clock {
    pub tick: Tick,
}

/// Buttons pressed since the race was last stepped. The keys are read once a frame
/// and the race stepped sixty times a second, so a press is kept here until a step
/// takes it: none is missed in a frame with no step, or taken twice in one with two.
#[derive(Resource, Default)]
pub struct Pending {
    use_item: bool,
    start_boost: Option<u8>,
}

/// A car shown as the host says it is, and not driven here.
#[derive(Component)]
pub struct Puppet;

/// Who has a place on the grid, as this game sees it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Who {
    /// The player at this game.
    Local,
    /// A player at another.
    Remote(Peer),
    Computer,
}

/// Who sits where in the race being run online, which the host decides.
#[derive(Resource, Clone, Debug)]
pub struct Lineup {
    pub seats: Vec<Seat>,
    pub you: Peer,
}

impl Lineup {
    /// Who has the grid slot, and the name they go by if it isn't the game's own for
    /// that slot.
    pub fn driver(&self, slot: usize) -> Option<(Who, Option<String>)> {
        let seat = self.seats.iter().find(|seat| seat.slot as usize == slot)?;
        Some(match seat.peer {
            Some(peer) if peer == self.you => (Who::Local, Some(seat.name.clone())),
            Some(peer) => (Who::Remote(peer), Some(seat.name.clone())),
            None => (Who::Computer, None),
        })
    }

    /// Players take the grid from the back, the host last of all as a player alone
    /// is, and the computer's cars fill it from the front.
    pub fn seat(players: &[(Peer, String)], opponents: usize) -> Vec<Seat> {
        let slots = kart::PLAYER_SLOT + 1;
        let humans = players.iter().take(slots).enumerate().map(|(i, (peer, name))| Seat { slot: (slots - 1 - i) as u8, peer: Some(*peer), name: name.clone() });
        let computers = (0..opponents.min(slots.saturating_sub(players.len()))).map(|slot| Seat { slot: slot as u8, peer: None, name: String::new() });
        humans.chain(computers).collect()
    }
}

/// Someone in the session other than the host.
pub struct Member {
    pub peer: Peer,
    pub name: String,
    /// Their game has the race loaded.
    pub loaded: bool,
}

/// The session this game is hosting or has joined.
#[derive(Resource, Default)]
pub struct Session {
    /// What the session is called on the lobby's list.
    pub title: String,
    /// The name the player here goes by.
    pub name: String,
    /// What a player must give to join; empty if anyone may.
    pub password: String,
    /// The host's number for this game.
    pub you: Peer,
    /// The host's list of who else is here.
    pub members: Vec<Member>,
    /// Why the session ended, for the menu to say.
    pub notice: Option<String>,
}

/// What has arrived since the game last looked.
#[derive(Resource, Default)]
pub struct Inbox {
    pub joined: Vec<Peer>,
    pub left: Vec<Peer>,
    /// The link has broken: why.
    pub failed: Option<String>,
    pub to_host: Vec<(Peer, ToHost)>,
    pub to_player: Vec<ToPlayer>,
    pub inputs: Vec<(Peer, Inputs)>,
    pub snapshots: Vec<Snapshot>,
}

/// Reads what the link has brought. A message that can't be read is dropped: it is a
/// game of another version, or not a game.
fn pump(role: Res<Role>, wire: Option<ResMut<Wire>>, mut inbox: ResMut<Inbox>) {
    let Some(mut wire) = wire else { return };
    let hosting = *role == Role::Host;
    while let Some(event) = wire.0.poll() {
        let read = match event {
            Event::Joined(peer) => {
                inbox.joined.push(peer);
                Ok(())
            }
            Event::Left(peer) => {
                inbox.left.push(peer);
                Ok(())
            }
            Event::Failed(why) => {
                inbox.failed = Some(why);
                Ok(())
            }
            Event::Message(peer, bytes) if hosting => decode(&bytes).map(|message| inbox.to_host.push((peer, message))),
            Event::Message(_, bytes) => decode(&bytes).map(|message| inbox.to_player.push(message)),
            Event::Datagram(peer, bytes) if hosting => decode(&bytes).map(|inputs| inbox.inputs.push((peer, inputs))),
            Event::Datagram(_, bytes) => decode(&bytes).map(|snapshot| inbox.snapshots.push(snapshot)),
        };
        if let Err(error) = read {
            warn!("{error}");
        }
    }
}

/// Starts the race for everyone in the session, by the host's settings.
pub fn start(commands: &mut Commands, session: &mut Session, wire: &mut Wire, settings: &Settings, circuits: &Circuits, next: &mut NextState<Screen>) {
    let mut players = vec![(session.you, session.name.clone())];
    players.extend(session.members.iter().map(|member| (member.peer, member.name.clone())));
    let seats = Lineup::seat(&players, settings.opponents);
    let start = encode(&ToPlayer::Start { rules: Rules::of(settings, circuits), seats: seats.clone() });
    for member in &mut session.members {
        member.loaded = false;
        wire.0.send(member.peer, start.clone());
    }
    commands.insert_resource(Lineup { seats, you: session.you });
    next.set(Screen::Loading);
}

/// Begins hosting a session, which the lobby will list as `title`.
pub fn host_session(commands: &mut Commands, session: &mut Session, title: &str, name: &str, password: &str) {
    *session = Session { title: title.into(), name: name.into(), password: password.into(), you: HOST, ..default() };
    commands.insert_resource(Role::Host);
    commands.insert_resource(Wire(Box::new(transport::Transport::host())));
}

/// Dials the host of a session on the lobby's list.
pub fn join_session(commands: &mut Commands, session: &mut Session, listed: &lobby_api::Session, name: &str, password: &str) {
    *session = Session { title: listed.name.clone(), name: name.into(), password: password.into(), ..default() };
    commands.insert_resource(Role::Client);
    commands.insert_resource(Wire(Box::new(transport::Transport::join(&listed.endpoint))));
}

/// Ends the session here: the game is its own again.
fn leave(commands: &mut Commands, session: &mut Session, why: Option<&str>, next: &mut NextState<Screen>) {
    commands.insert_resource(Role::Offline);
    commands.remove_resource::<Wire>();
    commands.remove_resource::<Lineup>();
    session.members.clear();
    session.notice = why.map(str::to_string);
    next.set(Screen::Menu);
}

/// The session's comings and goings, in a race or out of one.
fn session(
    mut commands: Commands,
    role: Res<Role>,
    screen: Res<State<Screen>>,
    circuits: Res<Circuits>,
    mut inbox: ResMut<Inbox>,
    mut session: ResMut<Session>,
    mut wire: ResMut<Wire>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<Screen>>,
    remotes: Query<(Entity, &Remote)>,
) {
    let inbox = &mut *inbox;
    // Nothing is listening for the race's own traffic outside a race.
    if *screen.get() != Screen::Race {
        inbox.inputs.clear();
        inbox.snapshots.clear();
    }
    for _ in inbox.joined.drain(..) {
        // A player's game has reached its host, and says who it is.
        if *role == Role::Client {
            wire.0.send(HOST, encode(&ToHost::Hello { protocol: lobby_api::PROTOCOL, name: session.name.clone(), password: session.password.clone() }));
        }
    }
    if let Some(why) = inbox.failed.take() {
        warn!("the session is over: {why}");
        leave(&mut commands, &mut session, Some("The connection failed"), &mut next);
        return;
    }
    if *role == Role::Host {
        for (peer, message) in inbox.to_host.drain(..) {
            match message {
                ToHost::Hello { protocol, name, password } => {
                    let refusal = if protocol != lobby_api::PROTOCOL {
                        Some(Refusal::Version)
                    } else if password != session.password {
                        Some(Refusal::Password)
                    } else if session.members.len() + 1 >= lobby_api::MAX_PLAYERS as usize {
                        Some(Refusal::Full)
                    } else {
                        None
                    };
                    let answer = match refusal {
                        Some(refusal) => ToPlayer::Refused(refusal),
                        None => {
                            info!("{name} has joined");
                            session.members.retain(|member| member.peer != peer);
                            session.members.push(Member { peer, name: name.chars().take(lobby_api::MAX_NAME).collect(), loaded: false });
                            ToPlayer::Welcome { you: peer }
                        }
                    };
                    wire.0.send(peer, encode(&answer));
                }
                ToHost::Loaded => {
                    if let Some(member) = session.members.iter_mut().find(|member| member.peer == peer) {
                        member.loaded = true;
                    }
                }
            }
        }
        for peer in inbox.left.drain(..) {
            info!("player {peer} has gone");
            session.members.retain(|member| member.peer != peer);
            // A car whose player has gone is the computer's to drive.
            for (entity, _) in remotes.iter().filter(|(_, remote)| remote.peer == peer) {
                commands.entity(entity).remove::<Remote>();
            }
        }
    } else {
        if !inbox.left.is_empty() {
            inbox.left.clear();
            leave(&mut commands, &mut session, Some("The host has gone"), &mut next);
            return;
        }
        for message in inbox.to_player.drain(..) {
            match message {
                ToPlayer::Welcome { you } => {
                    info!("joined {} as player {you}", session.title);
                    session.you = you;
                }
                ToPlayer::Refused(refusal) => {
                    let why = match refusal {
                        Refusal::Version => "The host has another version of the game",
                        Refusal::Password => "Wrong password",
                        Refusal::Full => "The session is full",
                    };
                    leave(&mut commands, &mut session, Some(why), &mut next);
                    return;
                }
                ToPlayer::Start { rules, seats } => {
                    if rules.apply(&mut settings, &circuits) {
                        info!("racing {} with {} cars", rules.circuit, seats.len());
                        commands.insert_resource(Lineup { seats, you: session.you });
                        next.set(Screen::Loading);
                    } else {
                        leave(&mut commands, &mut session, Some("The host chose a circuit this game hasn't got"), &mut next);
                        return;
                    }
                }
                ToPlayer::Over { .. } => next.set(Screen::Menu),
            }
        }
    }
}

/// `BRICK_NET=host` (or `host:3`, for three players) hosts a session and starts its
/// race once that many are in it; `BRICK_NET=join:<title>` joins the session of that
/// title once the lobby lists it. `BRICK_SESSION` is the title hosted under,
/// `BRICK_NAME` the player's name and `BRICK_PASSWORD` the password set or given.
#[derive(Resource)]
pub enum Auto {
    Host { players: usize, started: bool },
    Join { title: String, asked: f32, dialled: bool },
}

impl Auto {
    pub fn from_env() -> Option<Self> {
        let wanted = std::env::var("BRICK_NET").ok()?;
        match wanted.split_once(':').unwrap_or((&wanted, "")) {
            ("host", players) => Some(Auto::Host { players: players.parse().unwrap_or(2), started: false }),
            ("join", title) => Some(Auto::Join { title: title.to_string(), asked: 0.0, dialled: false }),
            _ => None,
        }
    }
}

fn auto(
    mut commands: Commands,
    time: Res<Time<Real>>,
    role: Res<Role>,
    screen: Res<State<Screen>>,
    (settings, circuits): (Res<Settings>, Res<Circuits>),
    lobby: Res<lobby::Lobby>,
    mut auto: ResMut<Auto>,
    mut session: ResMut<Session>,
    mut wire: Option<ResMut<Wire>>,
    mut next: ResMut<NextState<Screen>>,
) {
    let var = |name: &str, otherwise: &str| std::env::var(name).unwrap_or_else(|_| otherwise.to_string());
    match &mut *auto {
        Auto::Host { .. } if *role == Role::Offline => {
            host_session(&mut commands, &mut session, &var("BRICK_SESSION", "Demo"), &var("BRICK_NAME", "Host"), &var("BRICK_PASSWORD", ""));
        }
        Auto::Host { players, started } => {
            if let Some(wire) = wire.as_mut().filter(|_| !*started && session.members.len() + 1 >= *players && *screen.get() == Screen::Menu) {
                *started = true;
                start(&mut commands, &mut session, wire, &settings, &circuits, &mut next);
            }
        }
        Auto::Join { title, asked, dialled } => {
            if *dialled {
                return;
            }
            if let Some(listed) = lobby.sessions.iter().find(|listed| listed.name == *title) {
                *dialled = true;
                join_session(&mut commands, &mut session, listed, &var("BRICK_NAME", "Guest"), &var("BRICK_PASSWORD", ""));
                return;
            }
            *asked -= time.delta_secs();
            if *asked <= 0.0 {
                *asked = 1.0;
                lobby.list();
            }
        }
    }
}

/// Keeps what was pressed this frame for the step that will take it.
fn latch(mut pending: ResMut<Pending>, own: Query<&Controls, With<Player>>) {
    if let Ok(c) = own.single() {
        pending.use_item |= c.use_item;
        pending.start_boost = pending.start_boost.or(c.start_boost);
    }
}

/// A step begins: it is counted, and the buttons pressed since the last are its own.
fn begin_tick(mut clock: ResMut<Clock>, mut pending: ResMut<Pending>, mut own: Query<&mut Controls, With<Player>>) {
    clock.tick += 1;
    let pressed = std::mem::take(&mut *pending);
    if let Ok(mut c) = own.single_mut() {
        (c.use_item, c.start_boost) = (pressed.use_item, pressed.start_boost);
    }
}

/// A step ends: its buttons are not the next one's too.
fn end_tick(mut own: Query<&mut Controls, With<Player>>) {
    if let Ok(mut c) = own.single_mut() {
        (c.use_item, c.start_boost) = (false, None);
    }
}

/// A race online begins from nothing: no steps counted, nothing heard or guessed.
pub fn enter_race(mut commands: Commands) {
    commands.insert_resource(Clock::default());
    commands.insert_resource(Pending::default());
    commands.insert_resource(host::Flow::default());
    commands.insert_resource(client::Prediction::default());
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Role>()
        .init_resource::<Inbox>()
        .init_resource::<Session>()
        .init_resource::<lobby::Lobby>()
        .init_resource::<Clock>()
        .init_resource::<Pending>()
        .init_resource::<host::Flow>()
        .init_resource::<client::Prediction>()
        .insert_resource(Time::<Fixed>::from_hz(TICKS))
        .add_systems(PreUpdate, pump)
        // The cars are where the race has them while it is stepped, and between
        // steps while a frame is drawn (`display`).
        .add_systems(RunFixedMainLoop, display::unblend.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop).run_if(online))
        .add_systems(Update, (session.run_if(online), lobby::keep, auto.run_if(resource_exists::<Auto>)))
        .add_systems(
            Update,
            (
                (display::blend, kart::player_input, latch).chain().before(kart::sync_karts).before(crate::racer_sounds::racer_sounds),
                client::smooth.after(kart::sync_karts).run_if(joined),
            )
                .run_if(in_state(Screen::Race))
                .run_if(online),
        )
        // The race itself, as a game alone runs it each frame: the host all of it,
        // a player's game only its own car and what it is told of the rest.
        .add_systems(
            FixedUpdate,
            (
                begin_tick,
                client::receive.run_if(joined),
                (host::receive, host::flow, kart::ai_drive, host::drive_remotes).chain().run_if(hosting),
                client::send.run_if(joined),
                (items::use_items, items::actions).chain().run_if(hosting),
                kart::kart_physics,
                (kart::kart_collisions, kart::update_places, rules::elimination, items::pickups).chain().run_if(hosting),
                client::puppets.run_if(joined),
                host::send.run_if(hosting),
                (end_tick, display::note),
            )
                .chain()
                .run_if(in_state(Screen::Race))
                .run_if(online),
        );
}
