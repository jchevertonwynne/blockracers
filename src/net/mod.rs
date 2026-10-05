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
pub mod room;
pub mod scene;
pub mod state;
#[cfg(test)]
mod tests;
pub mod transport;

use bevy::prelude::*;

pub use host::Remote;
pub use link::Quality;
use link::{Event, Link};
use protocol::{
    HOST, Inputs, Peer, Refusal, Ride, Rules, Seat, Snapshot, TICKS, Tick, ToHost, ToPlayer,
    decode, encode,
};
use room::{Room, Voter};

use crate::garage::Garage;
#[cfg(test)]
use crate::kart::Kart;
use crate::kart::{self, Controls, Player};
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

impl Pending {
    /// The power-up button is pressed, by something other than the keys.
    pub fn use_item(&mut self) {
        self.use_item = true;
    }
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

    /// What players have chosen to race as, by grid slot.
    pub fn cast(&self) -> impl Iterator<Item = (usize, &Ride)> {
        self.seats
            .iter()
            .filter(|seat| seat.car != Ride::Slot)
            .map(|seat| (seat.slot as usize, &seat.car))
    }

    /// Players take the grid from the back, the host last of all as a player alone
    /// is, and the computer's cars fill it from the front.
    pub fn seat(players: &[(Peer, String, Ride)], opponents: usize) -> Vec<Seat> {
        let slots = kart::PLAYER_SLOT + 1;
        let humans = players
            .iter()
            .take(slots)
            .enumerate()
            .map(|(i, (peer, name, car))| Seat {
                slot: (slots - 1 - i) as u8,
                peer: Some(*peer),
                name: name.clone(),
                car: car.clone(),
            });
        let computers = (0..opponents.min(slots.saturating_sub(players.len()))).map(|slot| Seat {
            slot: slot as u8,
            peer: None,
            name: String::new(),
            car: Ride::Slot,
        });
        humans.chain(computers).collect()
    }
}

/// Someone in the session other than the host.
pub struct Member {
    pub peer: Peer,
    pub name: String,
    /// What they race as (`Seat::car`).
    pub car: Ride,
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
    /// What the player here races as (`Seat::car`).
    pub car: Ride,
    /// Why the session ended, for the menu to say, and that one has just been left.
    pub notice: Option<String>,
    pub left: bool,
    /// The player's own settings, put by while the session's are raced by.
    pub own: Option<Settings>,
    /// The most players the host will have, if fewer than the game allows.
    pub limit: Option<u8>,
    /// Players the host wants out of the session.
    pub removing: Vec<Peer>,
    /// The rules of the race that is on, and the players in the room who are
    /// watching it without a car in it.
    pub racing: Option<Rules>,
    pub watching: Vec<Peer>,
    /// How many races the session's series is of; none if it is not run as one.
    pub series: u8,
    /// Players who have gone, and the points they had, should they come back.
    pub absent: Vec<(String, u32)>,
    /// Kept off the lobby's list, and the code the lobby has given the session.
    pub unlisted: bool,
    pub code: String,
}

impl Session {
    /// The most players the session takes, the host among them.
    pub fn most(&self) -> u8 {
        self.limit
            .unwrap_or(lobby_api::MAX_PLAYERS)
            .clamp(2, lobby_api::MAX_PLAYERS)
    }
}

/// The car the screen is on when that isn't the player's own: another's, once the
/// player's race is run, or any at all for someone come only to watch.
#[derive(Resource, Default)]
pub struct Watching {
    /// The grid slot of the car followed; the player's own if none.
    pub slot: Option<usize>,
    /// The player has no car to drive, and may look at whichever they like.
    pub free: bool,
}

/// Lets a player with no car to drive follow the others: left and right go from car
/// to car. Someone with no car in the race begins on the leader. On their game the
/// car followed is marked as the player's, so that everything a screen does for its
/// own car (what is heard, what the display says) is done for that one.
fn watch(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    role: Res<Role>,
    lineup: Option<Res<Lineup>>,
    mut watching: ResMut<Watching>,
    karts: Query<(Entity, &kart::Kart, Has<Player>)>,
) {
    let Some(lineup) = lineup else { return };
    let seat = lineup
        .seats
        .iter()
        .find(|seat| seat.peer == Some(lineup.you))
        .map(|seat| seat.slot as usize);
    let own = seat
        .and_then(|slot| karts.iter().find(|(_, kart, _)| kart.slot == slot))
        .map(|(_, kart, _)| kart);
    let free = match (seat, own) {
        (None, _) => true,
        (Some(_), Some(own)) => own.finished.is_some() || own.out.is_some(),
        (Some(_), None) => false,
    };
    let mut running: Vec<&kart::Kart> = karts
        .iter()
        .map(|(_, kart, _)| kart)
        .filter(|kart| kart.out.is_none())
        .collect();
    running.sort_by_key(|kart| kart.slot);
    let step = keys.just_pressed(KeyCode::ArrowRight) as usize
        + keys.just_pressed(KeyCode::ArrowLeft) as usize * running.len().saturating_sub(1);
    let followed = watching.slot.or(seat);
    let at = running.iter().position(|kart| Some(kart.slot) == followed);
    let slot = match (free, at) {
        (false, _) => None,
        (true, Some(at)) if step > 0 => Some(running[(at + step) % running.len()].slot),
        (true, Some(_)) => watching.slot,
        // The car followed is out of the race, or none is yet: on to the leader.
        (true, None) => running
            .iter()
            .min_by_key(|kart| kart.place)
            .map(|kart| kart.slot),
    };
    // The player's own car followed is nothing out of the ordinary.
    let slot = slot.filter(|slot| Some(*slot) != seat);
    if (watching.slot, watching.free) != (slot, free) {
        (watching.slot, watching.free) = (slot, free);
    }
    if *role == Role::Client && seat.is_none() {
        for (entity, kart, marked) in &karts {
            match (Some(kart.slot) == slot, marked) {
                (true, false) => drop(commands.entity(entity).insert(Player)),
                (false, true) => drop(commands.entity(entity).remove::<Player>()),
                _ => {}
            }
        }
    }
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
    pub scenes: Vec<scene::Scene>,
    pub events: Vec<scene::EventNote>,
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
            Event::Message(peer, bytes) if hosting => {
                decode(&bytes).map(|message| inbox.to_host.push((peer, message)))
            }
            Event::Message(_, bytes) => decode(&bytes).map(|message| inbox.to_player.push(message)),
            Event::Datagram(peer, bytes) if hosting => {
                decode(&bytes).map(|inputs| inbox.inputs.push((peer, inputs)))
            }
            Event::Datagram(_, bytes) => {
                decode(&bytes).map(|snapshot| inbox.snapshots.push(snapshot))
            }
        };
        if let Err(error) = read {
            warn!("{error}");
        }
    }
}

/// Starts the race for everyone in the session, to be run by `rules`.
pub fn start(
    commands: &mut Commands,
    session: &mut Session,
    wire: &mut Wire,
    rules: &Rules,
    settings: &mut Settings,
    circuits: &Circuits,
    next: &mut NextState<Screen>,
) {
    if !rules.apply(settings, circuits) {
        warn!("no circuit {} to race", rules.circuit);
        return;
    }
    let mut players = vec![(session.you, session.name.clone(), session.car.clone())];
    players.extend(
        session
            .members
            .iter()
            .map(|member| (member.peer, member.name.clone(), member.car.clone())),
    );
    let seats = Lineup::seat(&players, settings.opponents);
    (session.racing, session.watching) = (Some(rules.clone()), Vec::new());
    let start = encode(&ToPlayer::Start {
        rules: rules.clone(),
        seats: seats.clone(),
    });
    for member in &mut session.members {
        member.loaded = false;
        wire.0.send(member.peer, start.clone());
    }
    commands.insert_resource(Lineup {
        seats,
        you: session.you,
    });
    next.set(Screen::Loading);
}

/// The room a session begins with: the player here in it, wanting the race their own
/// settings would give.
fn fresh_room(session: &Session, hosting: bool, settings: &Settings, circuits: &Circuits) -> Room {
    // The host's say is how the race is run; a player has no vote until they cast one.
    let ballot = hosting.then(|| Rules::of(settings, circuits));
    let voters = if hosting {
        vec![Voter {
            peer: HOST,
            name: session.name.clone(),
            ready: false,
            ballot: ballot.clone(),
            link: None,
            points: 0,
        }]
    } else {
        Vec::new()
    };
    Room {
        voters,
        ballot,
        revision: 1,
        ..default()
    }
}

/// Begins hosting a session, which the lobby will list as `title`.
pub fn host_session(
    commands: &mut Commands,
    session: &mut Session,
    settings: &Settings,
    circuits: &Circuits,
    title: &str,
    password: &str,
    ride: Ride,
) {
    *session = Session {
        title: title.into(),
        name: settings.name.clone(),
        car: ride,
        password: password.into(),
        you: HOST,
        own: Some(settings.clone()),
        ..default()
    };
    commands.insert_resource(fresh_room(session, true, settings, circuits));
    commands.insert_resource(Role::Host);
    commands.insert_resource(Wire(Box::new(transport::Transport::host())));
}

/// Dials the host of a session on the lobby's list.
pub fn join_session(
    commands: &mut Commands,
    session: &mut Session,
    settings: &Settings,
    circuits: &Circuits,
    listed: &lobby_api::Session,
    password: &str,
    ride: Ride,
) {
    *session = Session {
        title: listed.name.clone(),
        name: settings.name.clone(),
        car: ride,
        password: password.into(),
        own: Some(settings.clone()),
        ..default()
    };
    commands.insert_resource(fresh_room(session, false, settings, circuits));
    commands.insert_resource(Role::Client);
    commands.insert_resource(Wire(Box::new(transport::Transport::join(&listed.endpoint))));
}

/// The host calls the race off: everyone is back in the room, the session as it was,
/// to vote on another.
pub fn call_off(session: &mut Session, wire: &mut Wire, next: &mut NextState<Screen>) {
    (session.racing, session.watching) = (None, Vec::new());
    let over = encode(&ToPlayer::Over);
    for member in &mut session.members {
        wire.0.send(member.peer, over.clone());
        member.loaded = false;
    }
    next.set(Screen::Menu);
}

/// Says something to the room.
pub fn say(role: Role, session: &Session, room: &mut Room, wire: &mut Wire, words: &str) {
    let words: String = words.trim().chars().take(room::CHAT_LENGTH).collect();
    if words.is_empty() {
        return;
    }
    if role == Role::Host {
        let line = format!("{}: {words}", session.name);
        for member in &session.members {
            wire.0
                .send(member.peer, encode(&ToPlayer::Said(line.clone())));
        }
        room.hear(line);
    } else {
        wire.0.send(HOST, encode(&ToHost::Say(words)));
    }
}

/// A player in the room goes to the race that is on.
pub fn enter(wire: &mut Wire) {
    wire.0.send(HOST, encode(&ToHost::Enter));
}

/// A player gives the race up and goes back to the room, still in the session: the
/// host is told, and has the computer drive their car for what is left of the race.
pub fn retire(wire: &mut Wire, next: &mut NextState<Screen>) {
    wire.0.send(HOST, encode(&ToHost::Back));
    next.set(Screen::Menu);
}

/// Ends the session here: the game is its own again, with its own settings. Letting
/// go of the link is what tells the others.
pub fn leave(
    commands: &mut Commands,
    session: &mut Session,
    settings: &mut Settings,
    why: Option<&str>,
    next: &mut NextState<Screen>,
) {
    commands.insert_resource(Role::Offline);
    commands.remove_resource::<Wire>();
    commands.remove_resource::<Lineup>();
    commands.insert_resource(Room::default());
    session.members.clear();
    (session.notice, session.left) = (why.map(str::to_string), true);
    if let Some(own) = session.own.take() {
        *settings = own;
    }
    next.set(Screen::Menu);
}

/// The session's comings and goings, in a race or out of one, and the room's vote.
fn session(
    mut commands: Commands,
    time: Res<Time<Real>>,
    role: Res<Role>,
    screen: Res<State<Screen>>,
    circuits: Res<Circuits>,
    mut inbox: ResMut<Inbox>,
    mut session: ResMut<Session>,
    mut room: ResMut<Room>,
    mut wire: ResMut<Wire>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<Screen>>,
    karts: Query<(Entity, &kart::Kart, Option<&Remote>)>,
    (mut lineup, mut told): (Option<ResMut<Lineup>>, ResMut<scene::Told>),
    mut measured: Local<f32>,
) {
    let remotes = || {
        karts
            .iter()
            .filter_map(|(entity, _, remote)| Some((entity, remote?)))
    };
    let (inbox, room, session) = (&mut *inbox, &mut *room, &mut *session);
    // Nothing is listening for the race's own traffic outside a race.
    if *screen.get() != Screen::Race {
        inbox.inputs.clear();
        inbox.snapshots.clear();
        inbox.scenes.clear();
        inbox.events.clear();
    }
    for _ in inbox.joined.drain(..) {
        // A player's game has reached its host, and says who it is.
        if *role == Role::Client {
            wire.0.send(
                HOST,
                encode(&ToHost::Hello {
                    protocol: lobby_api::PROTOCOL,
                    name: session.name.clone(),
                    password: session.password.clone(),
                    car: session.car.clone(),
                }),
            );
        }
    }
    if let Some(why) = inbox.failed.take() {
        warn!("the session is over: {why}");
        leave(
            &mut commands,
            session,
            &mut settings,
            Some("The connection failed"),
            &mut next,
        );
        return;
    }
    if *role == Role::Host {
        for (peer, message) in inbox.to_host.drain(..) {
            match message {
                ToHost::Hello {
                    protocol,
                    name,
                    password,
                    car,
                } => {
                    let refusal = if protocol != lobby_api::PROTOCOL {
                        Some(Refusal::Version)
                    // The game's lettering is all capitals, so a password can't be told
                    // from itself in another case, and isn't asked to be.
                    } else if !password.eq_ignore_ascii_case(&session.password) {
                        Some(Refusal::Password)
                    } else if session.members.len() + 1 >= session.most() as usize {
                        Some(Refusal::Full)
                    } else {
                        None
                    };
                    let answer = match refusal {
                        Some(refusal) => ToPlayer::Refused(refusal),
                        None => {
                            info!("{name} has joined");
                            let name: String = name.chars().take(lobby_api::MAX_NAME).collect();
                            session.members.retain(|member| member.peer != peer);
                            session.members.push(Member {
                                peer,
                                name: name.clone(),
                                car: car.checked(),
                                loaded: false,
                            });
                            // Someone who was here before has what they had: their
                            // points, and their car if the race they left is still on.
                            let had = session.absent.iter().position(|gone| gone.0 == name);
                            let points = had.map_or(0, |had| session.absent.remove(had).1);
                            if let Some(lineup) = &mut lineup {
                                let here = |seat: &Seat| {
                                    seat.peer.is_some_and(|sat| {
                                        sat == HOST
                                            || session.members.iter().any(|member| {
                                                member.peer == sat && member.peer != peer
                                            })
                                    })
                                };
                                if let Some(seat) = lineup.seats.iter_mut().find(|seat| {
                                    seat.peer.is_some() && seat.name == name && !here(seat)
                                }) {
                                    seat.peer = Some(peer);
                                }
                            }
                            room.voters.retain(|voter| voter.peer != peer);
                            room.voters.push(Voter {
                                peer,
                                name,
                                ready: false,
                                ballot: None,
                                link: None,
                                points,
                            });
                            ToPlayer::Welcome { you: peer }
                        }
                    };
                    wire.0.send(peer, encode(&answer));
                }
                ToHost::Loaded => {
                    if let Some(member) = session
                        .members
                        .iter_mut()
                        .find(|member| member.peer == peer)
                    {
                        member.loaded = true;
                    }
                    // Someone come to a race already on drives their car if they
                    // have one in it that nobody is driving, and otherwise watches;
                    // either way they are told what they missed.
                    if *screen.get() == Screen::Race
                        && session.members.iter().any(|member| member.peer == peer)
                    {
                        let seat = lineup.as_ref().and_then(|lineup| {
                            lineup.seats.iter().find(|seat| seat.peer == Some(peer))
                        });
                        let car = seat.and_then(|seat| {
                            karts
                                .iter()
                                .find(|(_, kart, _)| kart.slot == seat.slot as usize)
                        });
                        match car {
                            Some((_, _, Some(_))) => continue,
                            Some((entity, _, None)) => {
                                commands.entity(entity).insert(Remote::new(peer));
                            }
                            None if !session.watching.contains(&peer) => {
                                session.watching.push(peer)
                            }
                            None => {}
                        }
                        wire.0.send(peer, encode(&ToPlayer::Events(told.again())));
                    }
                }
                ToHost::Enter => {
                    if let (Screen::Race, Some(rules), Some(lineup)) =
                        (*screen.get(), &session.racing, &lineup)
                    {
                        wire.0.send(
                            peer,
                            encode(&ToPlayer::Start {
                                rules: rules.clone(),
                                seats: lineup.seats.clone(),
                            }),
                        );
                    }
                }
                ToHost::Say(words) => {
                    let name = session
                        .members
                        .iter()
                        .find(|member| member.peer == peer)
                        .map(|member| member.name.clone());
                    if let Some(name) = name {
                        let words: String = words
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(room::CHAT_LENGTH)
                            .collect();
                        let line = format!("{name}: {words}");
                        for member in &session.members {
                            wire.0
                                .send(member.peer, encode(&ToPlayer::Said(line.clone())));
                        }
                        room.hear(line);
                    }
                }
                ToHost::Vote(ballot) => {
                    if let Some(voter) = room.voters.iter_mut().find(|voter| voter.peer == peer) {
                        voter.ballot = Some(ballot);
                    }
                }
                ToHost::Ride(ride) => {
                    if let Some(member) = session.members.iter_mut().find(|m| m.peer == peer) {
                        member.car = ride.checked();
                        info!("{} has changed what they race as", member.name);
                    }
                }
                ToHost::Ready(ready) => {
                    if let Some(voter) = room.voters.iter_mut().find(|voter| voter.peer == peer) {
                        voter.ready = ready;
                    }
                }
                ToHost::Back => {
                    session.watching.retain(|watching| *watching != peer);
                    for (entity, _) in remotes().filter(|(_, remote)| remote.peer == peer) {
                        commands.entity(entity).remove::<Remote>();
                    }
                }
            }
        }
        // A player the host wants gone is told so and let go of.
        for peer in session.removing.drain(..) {
            wire.0
                .send(peer, encode(&ToPlayer::Refused(Refusal::Removed)));
            wire.0.close(peer);
            inbox.left.push(peer);
        }
        for peer in inbox.left.drain(..) {
            info!("player {peer} has gone");
            session.members.retain(|member| member.peer != peer);
            session.watching.retain(|watching| *watching != peer);
            if let Some(voter) = room.voters.iter().find(|voter| voter.peer == peer) {
                session.absent.retain(|gone| gone.0 != voter.name);
                session.absent.push((voter.name.clone(), voter.points));
            }
            room.voters.retain(|voter| voter.peer != peer);
            // A car whose player has gone is the computer's to drive.
            for (entity, _) in remotes().filter(|(_, remote)| remote.peer == peer) {
                commands.entity(entity).remove::<Remote>();
            }
        }
        // How good each player's way here is, looked at once a second (and at once
        // for someone new) and told to the nearest five milliseconds, so that the
        // room isn't forever changing.
        *measured -= time.delta_secs();
        if *measured <= 0.0
            || room
                .voters
                .iter()
                .any(|voter| voter.peer != HOST && voter.link.is_none())
        {
            *measured = 1.0;
            for voter in room.voters.iter_mut().filter(|voter| voter.peer != HOST) {
                voter.link = wire.0.quality(voter.peer).map(|link| Quality {
                    ping: (link.ping + 2) / 5 * 5,
                    ..link
                });
            }
        }
        // The host's own wishes are in the room with everyone else's.
        if let Some(own) = room.voters.iter_mut().find(|voter| voter.peer == HOST) {
            (own.ballot, own.ready) = (room.ballot.clone(), room.ready);
        }
        // The vote is taken between races, and the race it settles on begun.
        if *screen.get() == Screen::Menu {
            let fallback = Rules::of(&settings, &circuits);
            let mut dice = crate::meshgen::Rng(time.elapsed().subsec_nanos() | 1);
            if let Some(rules) = room.tick(time.delta_secs(), &fallback, &mut dice) {
                // A series that has been run to its end is begun again.
                if session.series > 0 && room.raced >= session.series {
                    room.reset();
                }
                start(
                    &mut commands,
                    session,
                    &mut wire,
                    &rules,
                    &mut settings,
                    &circuits,
                    &mut next,
                );
            }
        }
        // The others are told how the room stands whenever it changes, the clock to
        // the second.
        let telling = encode(&ToPlayer::Room {
            voters: room.voters.clone(),
            closing: room.closing.map(|left| left.ceil().max(0.0)),
            racing: *screen.get() != Screen::Menu,
            series: (session.series > 0).then_some((room.raced, session.series)),
            last: room.last.clone(),
            results: room.results.clone(),
        });
        room.series = (session.series > 0).then_some((room.raced, session.series));
        // And what everyone races as, whenever any of that changes or someone comes.
        let mut rides = vec![(HOST, session.car.clone())];
        rides.extend(session.members.iter().map(|m| (m.peer, m.car.clone())));
        if room.rides != rides {
            let telling = encode(&ToPlayer::Rides(rides.clone()));
            for member in &session.members {
                wire.0.send(member.peer, telling.clone());
            }
            room.rides = rides;
            room.revision += 1;
        }
        if room.told != telling {
            for member in &session.members {
                wire.0.send(member.peer, telling.clone());
            }
            room.told = telling;
            room.revision += 1;
        }
    } else {
        // What the host said before it went is heard first: it may be why.
        let gone = !std::mem::take(&mut inbox.left).is_empty();
        for message in std::mem::take(&mut inbox.to_player) {
            match message {
                ToPlayer::Welcome { you } => {
                    info!("joined {} as player {you}", session.title);
                    session.you = you;
                    room.revision += 1;
                }
                ToPlayer::Refused(refusal) => {
                    let why = match refusal {
                        Refusal::Version => "The host has another version of the game",
                        Refusal::Password => "Wrong password",
                        Refusal::Full => "The session is full",
                        Refusal::Removed => "The host removed you from the session",
                    };
                    leave(&mut commands, session, &mut settings, Some(why), &mut next);
                    return;
                }
                ToPlayer::Room {
                    voters,
                    closing,
                    racing,
                    series,
                    last,
                    results,
                } => {
                    room.series = series;
                    room.fresh |= !results.is_empty() && results != room.results;
                    (
                        room.voters,
                        room.closing,
                        room.racing,
                        room.last,
                        room.results,
                    ) = (voters, closing, racing, last, results);
                    room.revision += 1;
                }
                ToPlayer::Start { rules, seats } => {
                    if rules.apply(&mut settings, &circuits) {
                        info!("racing {} with {} cars", rules.circuit, seats.len());
                        room.ready = false;
                        commands.insert_resource(Lineup {
                            seats,
                            you: session.you,
                        });
                        next.set(Screen::Loading);
                    } else {
                        leave(
                            &mut commands,
                            session,
                            &mut settings,
                            Some("The host chose a circuit this game hasn't got"),
                            &mut next,
                        );
                        return;
                    }
                }
                // A player who gave the race up is in the room already.
                ToPlayer::Over if *screen.get() == Screen::Menu => {}
                ToPlayer::Over => next.set(Screen::Menu),
                ToPlayer::Scene(scene) => inbox.scenes.push(scene),
                ToPlayer::Events(notes) => inbox.events.extend(notes),
                ToPlayer::Said(line) => room.hear(line),
                ToPlayer::Rides(rides) => {
                    room.rides = rides;
                    room.revision += 1;
                }
            }
        }
        if gone {
            leave(
                &mut commands,
                session,
                &mut settings,
                Some("The host has gone"),
                &mut next,
            );
            return;
        }
        // The host is told what the player here wants whenever that changes.
        let wishes = (room.ballot.clone(), room.ready);
        if session.you != HOST && room.sent.as_ref() != Some(&wishes) {
            if let Some(ballot) = &wishes.0 {
                wire.0.send(HOST, encode(&ToHost::Vote(ballot.clone())));
            }
            wire.0.send(HOST, encode(&ToHost::Ready(wishes.1)));
            room.sent = Some(wishes);
        }
        // And what they race as, which the host had first when they joined.
        if session.you != HOST && room.rode.as_ref() != Some(&session.car) {
            if room.rode.is_some() {
                wire.0
                    .send(HOST, encode(&ToHost::Ride(session.car.clone())));
            }
            room.rode = Some(session.car.clone());
        }
    }
}

/// `BRICK_NET=host` (or `host:3`, for three players) hosts a session and starts its
/// race, by the host's own settings and without a vote, once that many are in it;
/// `BRICK_NET=join:<title>` joins the session of that title once the lobby lists it.
/// `BRICK_SESSION` is the title hosted under, `BRICK_NAME` the player's name,
/// `BRICK_CAR` who they race as and `BRICK_PASSWORD` the password set or given.
#[derive(Resource)]
pub enum Auto {
    Host {
        players: usize,
        started: bool,
    },
    Join {
        title: String,
        asked: f32,
        dialled: bool,
    },
}

impl Auto {
    pub fn from_env() -> Option<Self> {
        let wanted = std::env::var("BRICK_NET").ok()?;
        match wanted.split_once(':').unwrap_or((&wanted, "")) {
            ("host", players) => Some(Auto::Host {
                players: players.parse().unwrap_or(2),
                started: false,
            }),
            ("join", title) => Some(Auto::Join {
                title: title.to_string(),
                asked: 0.0,
                dialled: false,
            }),
            _ => None,
        }
    }
}

fn auto(
    mut commands: Commands,
    time: Res<Time<Real>>,
    role: Res<Role>,
    screen: Res<State<Screen>>,
    (mut settings, circuits, garage): (ResMut<Settings>, Res<Circuits>, Res<Garage>),
    lobby: Res<lobby::Lobby>,
    mut auto: ResMut<Auto>,
    mut session: ResMut<Session>,
    mut wire: Option<ResMut<Wire>>,
    mut next: ResMut<NextState<Screen>>,
) {
    let var =
        |name: &str, otherwise: &str| std::env::var(name).unwrap_or_else(|_| otherwise.to_string());
    if let Ok(name) = std::env::var("BRICK_NAME") {
        settings.name = name;
    }
    // `BRICK_CAR=PH`: who to race as, by the game's code for the driver; or
    // `BRICK_CAR=3`, as the third of the garage's racers.
    if let Some(car) = std::env::var("BRICK_CAR").ok().and_then(|code| {
        let built = code.parse::<usize>().ok().filter(|&n| n > 0);
        built.map(|n| crate::roster::NAMES.len() + n).or_else(|| {
            let driver = crate::roster::NAMES.iter().position(|d| d.0 == code);
            driver.map(|n| n + 1)
        })
    }) {
        settings.car = car;
    }
    match &mut *auto {
        Auto::Host { .. } if *role == Role::Offline => {
            host_session(
                &mut commands,
                &mut session,
                &settings,
                &circuits,
                &var("BRICK_SESSION", "Demo"),
                &var("BRICK_PASSWORD", ""),
                garage.ride(&settings),
            );
        }
        Auto::Host { players, started } => {
            if let Some(wire) = wire.as_mut().filter(|_| {
                !*started && session.members.len() + 1 >= *players && *screen.get() == Screen::Menu
            }) {
                *started = true;
                let rules = Rules::of(&settings, &circuits);
                start(
                    &mut commands,
                    &mut session,
                    wire,
                    &rules,
                    &mut settings,
                    &circuits,
                    &mut next,
                );
            }
        }
        Auto::Join {
            title,
            asked,
            dialled,
        } => {
            if *dialled {
                return;
            }
            if let Some(listed) = lobby.sessions.iter().find(|listed| listed.name == *title) {
                *dialled = true;
                join_session(
                    &mut commands,
                    &mut session,
                    &settings,
                    &circuits,
                    listed,
                    &var("BRICK_PASSWORD", ""),
                    garage.ride(&settings),
                );
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
fn begin_tick(
    mut clock: ResMut<Clock>,
    mut pending: ResMut<Pending>,
    mut own: Query<&mut Controls, With<Player>>,
) {
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
pub fn enter_race(
    mut commands: Commands,
    role: Res<Role>,
    mut pause: ResMut<crate::Pause>,
    mut sfx: ResMut<crate::audio::Sfx>,
    events: Option<ResMut<crate::events::TrackEvents>>,
) {
    // No question is left hanging over from the race before.
    pause.0 = None;
    // The circuit's events are the host's: it logs them and its players follow.
    if let Some(mut events) = events {
        (events.logging, events.following) = (*role == Role::Host, *role == Role::Client);
    }
    sfx.listening = false;
    sfx.heard.clear();
    sfx.looping.clear();
    commands.insert_resource(Watching::default());
    commands.insert_resource(scene::Told::default());
    commands.insert_resource(scene::Shown::default());
    commands.insert_resource(Clock::default());
    commands.insert_resource(Pending::default());
    commands.insert_resource(host::Flow::default());
    commands.insert_resource(client::Prediction::default());
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Role>()
        .init_resource::<Inbox>()
        .init_resource::<Session>()
        .init_resource::<Room>()
        .init_resource::<lobby::Lobby>()
        .init_resource::<Clock>()
        .init_resource::<Watching>()
        .init_resource::<Pending>()
        .init_resource::<host::Flow>()
        .init_resource::<client::Prediction>()
        .init_resource::<scene::Told>()
        .init_resource::<scene::Shown>()
        .insert_resource(Time::<Fixed>::from_hz(TICKS))
        .add_systems(PreUpdate, pump)
        // The cars are where the race has them while it is stepped, and between
        // steps while a frame is drawn (`display`).
        .add_systems(
            RunFixedMainLoop,
            display::unblend
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop)
                .run_if(online),
        )
        .add_systems(
            Update,
            (
                session.run_if(online),
                lobby::keep,
                auto.run_if(resource_exists::<Auto>),
            ),
        )
        .add_systems(Last, lobby::farewell)
        .add_systems(
            Update,
            (
                (display::blend, kart::player_input, latch)
                    .chain()
                    .before(kart::sync_karts)
                    .before(crate::racer_sounds::racer_sounds),
                watch.before(kart::sync_karts),
                client::smooth.after(kart::sync_karts).run_if(joined),
                scene::follow_events
                    .before(crate::events::track_events)
                    .run_if(joined),
                (
                    scene::glide.before(crate::item_models::dress_actions),
                    scene::sound_loops.before(crate::racer_sounds::racer_sounds),
                )
                    .run_if(joined),
                scene::tell_events
                    .after(crate::hazards::hazards)
                    .run_if(hosting),
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
                (
                    scene::listen,
                    host::receive,
                    host::flow,
                    kart::ai_drive,
                    host::drive_remotes,
                )
                    .chain()
                    .run_if(hosting),
                client::send.run_if(joined),
                (items::use_items, items::actions).chain().run_if(hosting),
                kart::kart_physics,
                client::bump.run_if(joined),
                (
                    kart::kart_collisions,
                    kart::update_places,
                    rules::elimination,
                    items::pickups,
                )
                    .chain()
                    .run_if(hosting),
                (client::puppets, scene::take, items::show_pickups)
                    .chain()
                    .run_if(joined),
                (host::send, scene::tell).chain().run_if(hosting),
                (end_tick, display::note),
            )
                .chain()
                .run_if(in_state(Screen::Race))
                .run_if(online),
        );
}
