//! A host and a player's game, each a whole `App` without a window, joined by a
//! network in memory that is as late and as lossy as a poor connection.

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;

use super::client::Prediction;
use super::link::Hub;
use super::protocol::{HOST, Peer, Rules, TICKS};
use super::*;
use crate::items::Power;
use crate::track::Track;
use crate::{Pause, Phase, Race};

/// Eighty milliseconds each way, and one in twenty of what can be lost, lost.
const DELAY: u32 = 5;
const LOSS: f32 = 0.05;

fn game(role: Role, link: impl Link, you: Peer, opponents: usize) -> App {
    game_drawn(role, link, you, opponents, 1)
}

/// A game whose screen is drawn `frames` times for each step of the race.
fn game_drawn(role: Role, link: impl Link, you: Peer, opponents: usize, frames: u32) -> App {
    let mut app = App::new();
    let circuits = Circuits::find();
    let settings = Settings::new(&circuits);
    let players = [(HOST, "Host".to_string()), (1, "Guest".to_string())];
    let members = if role == Role::Host { vec![Member { peer: 1, name: "Guest".into(), loaded: true }] } else { Vec::new() };
    app.add_plugins((MinimalPlugins, StatesPlugin, plugin))
        // Each update is one step of the race, exactly.
        .insert_resource(TimeUpdateStrategy::ManualDuration(Time::<Fixed>::from_hz(TICKS).timestep() / frames))
        .insert_state(Screen::Race)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<crate::audio::Sfx>()
        .init_resource::<crate::variant::Variant>()
        .init_resource::<Pause>()
        .init_resource::<crate::replay::Photo>()
        .insert_resource(Track::new())
        .insert_resource(crate::meshgen::Rng(7))
        .insert_resource(Race { phase: Phase::Intro, intro: 0.0, countdown: 0.0, time: 0.0, demo: false, quick: true })
        .insert_resource(circuits)
        .insert_resource(settings)
        .insert_resource(role)
        .insert_resource(Wire(Box::new(link)))
        .insert_resource(Session { name: "Me".into(), you, members, ..default() })
        .insert_resource(Room { voters: vec![Voter { peer: HOST, name: "Me".into(), ready: false, ballot: None }], ..default() })
        .insert_resource(Lineup { seats: Lineup::seat(&players, opponents), you })
        .add_systems(Startup, (kart::spawn_karts, items::setup_items).chain());
    app
}

struct Pair {
    hub: Hub,
    host: App,
    guest: App,
}

impl Pair {
    fn new(opponents: usize, loss: f32) -> Self {
        let hub = Hub::new(DELAY, loss);
        let (host, guest) = (hub.host(), hub.join());
        Pair { host: game(Role::Host, host, HOST, opponents), guest: game(Role::Client, guest, 1, opponents), hub }
    }

    /// One step of time for the network and for both games.
    fn step(&mut self) {
        self.hub.step();
        for app in [&mut self.host, &mut self.guest] {
            app.update();
            // No window is there to say the keys' presses are a frame old.
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        }
    }
}

fn press(app: &mut App, key: KeyCode, down: bool) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if down { keys.press(key) } else { keys.release(key) }
}

/// The car in a grid slot, in one of the games.
fn car(app: &mut App, slot: usize) -> &Kart {
    app.world_mut().query::<&Kart>().iter(app.world()).find(|k| k.slot == slot).expect("a car in the slot")
}

fn car_mut(app: &mut App, slot: usize) -> Mut<'_, Kart> {
    app.world_mut().query::<&mut Kart>().iter_mut(app.world_mut()).find(|k| k.slot == slot).expect("a car in the slot")
}

const HOSTS: usize = 5;
const GUESTS: usize = 4;

/// Drives the player's car about for eight seconds. Returns how far it went, the
/// biggest correction it was given and how many it was given.
fn drive_about(pair: &mut Pair) -> (f32, f32, usize) {
    pair.step();
    let from = car(&mut pair.guest, GUESTS).pos;
    let (mut worst, mut corrections) = (0.0f32, 0);
    for step in 0..480 {
        press(&mut pair.guest, KeyCode::KeyW, true);
        press(&mut pair.guest, KeyCode::KeyA, (120..200).contains(&step));
        press(&mut pair.guest, KeyCode::KeyD, (260..330).contains(&step));
        press(&mut pair.guest, KeyCode::ShiftLeft, (280..330).contains(&step));
        pair.step();
        let corrected = std::mem::take(&mut pair.guest.world_mut().resource_mut::<Prediction>().corrected);
        worst = worst.max(corrected);
        corrections += (corrected > 0.01) as usize;
    }
    let (there, here) = (car(&mut pair.host, GUESTS).pos, car(&mut pair.guest, GUESTS).pos);
    // The host has the car where the player's game had it a moment ago.
    assert!(there.distance(here) < 12.0, "the host has the car {} away", there.distance(here));
    assert!(pair.guest.world().resource::<Race>().phase == Phase::Racing);
    (here.distance(from), worst, corrections)
}

/// Driving alone on the road, a player's game steps its car exactly as the host
/// does, however late the word between them: nothing is ever corrected.
#[test]
fn a_players_own_car_needs_no_correcting() {
    let (went, worst, _) = drive_about(&mut Pair::new(0, 0.0));
    assert!(went > 40.0, "the car should have been driven somewhere, and went {went}");
    assert!(worst < 0.01, "the car was corrected by {worst}");
}

/// When a step's pressing is late, the host drives the car on without it, and the
/// player's game is put right by a step's travel: a little, and seldom, since the
/// host then has a step in hand.
#[test]
fn a_poor_connection_costs_a_step_now_and_then() {
    let (went, worst, corrections) = drive_about(&mut Pair::new(0, LOSS));
    assert!(went > 40.0, "the car should have been driven somewhere, and went {went}");
    assert!(worst < 0.6, "the car was corrected by {worst}");
    assert!(corrections < 8, "the car was corrected {corrections} times");
}

/// The cars a player doesn't drive are shown where the host had them a moment ago.
#[test]
fn the_other_cars_are_shown_where_the_host_had_them() {
    let mut pair = Pair::new(2, LOSS);
    let mut trail: Vec<Vec3> = Vec::new();
    for _ in 0..360 {
        press(&mut pair.host, KeyCode::KeyW, true);
        pair.step();
        trail.push(car(&mut pair.host, HOSTS).pos);
    }
    let shown = car(&mut pair.guest, HOSTS).pos;
    assert!(trail[0].distance(*trail.last().unwrap()) > 30.0, "the host's car should have been driven somewhere");
    let nearest = trail.iter().rev().take(40).map(|at| at.distance(shown)).reduce(f32::min).unwrap();
    assert!(nearest < 0.3, "the host's car is shown {nearest} from anywhere it lately was");
    // The computer's cars too, which only the host drives, and the order they are in.
    for slot in 0..2 {
        let (there, here) = (car(&mut pair.host, slot).pos, car(&mut pair.guest, slot).pos);
        assert!(here.distance(Kart::new(&Track::new(), slot).pos) > 5.0, "the computer's car {slot} should be seen to have moved");
        assert!(there.distance(here) < 15.0);
        assert_eq!(car(&mut pair.host, slot).place, car(&mut pair.guest, slot).place);
    }
}

/// What the host does to a player's car, the player's game is told: a cannonball
/// fired on the host knocks the car about on the player's own screen.
#[test]
fn a_cannonball_fired_on_the_host_hits_the_players_car() {
    let mut pair = Pair::new(0, LOSS);
    for _ in 0..30 {
        pair.step();
    }
    // The player's car a little way up the road, the host's behind it.
    let track = Track::new();
    car_mut(&mut pair.host, GUESTS).place(&track, 30.0, 0.0);
    car_mut(&mut pair.host, HOSTS).place(&track, 12.0, 0.0);
    for _ in 0..30 {
        pair.step();
    }
    assert!(car(&mut pair.guest, GUESTS).pos.distance(car(&mut pair.host, GUESTS).pos) < 0.5, "the player's game should have its car where the host put it");
    (car_mut(&mut pair.host, HOSTS).held, car_mut(&mut pair.host, HOSTS).whites) = (Some(Power::Red), 0);
    press(&mut pair.host, KeyCode::Space, true);
    let (mut struck_there, mut struck_here) = (false, false);
    let before = car(&mut pair.guest, GUESTS).pos;
    for _ in 0..180 {
        pair.step();
        press(&mut pair.host, KeyCode::Space, false);
        let knocked = |k: &Kart| k.spin > 0.0 || k.spin_out > 0.0 || k.contacts == 0 || k.vel.length() > 3.0;
        struck_there |= knocked(car(&mut pair.host, GUESTS));
        struck_here |= knocked(car(&mut pair.guest, GUESTS));
    }
    assert!(car(&mut pair.host, HOSTS).held.is_none(), "the host should have fired");
    assert!(struck_there, "the cannonball should have hit the car on the host");
    assert!(struck_here, "and the player's game should have seen it");
    assert!(car(&mut pair.guest, GUESTS).pos.distance(before) > 0.2);
    assert!(car(&mut pair.guest, GUESTS).pos.distance(car(&mut pair.host, GUESTS).pos) < 0.5);
}

/// A player who drops out leaves a car for the computer to drive.
#[test]
fn a_car_whose_player_has_gone_is_the_computers() {
    let mut pair = Pair::new(0, LOSS);
    for _ in 0..60 {
        press(&mut pair.guest, KeyCode::KeyW, true);
        pair.step();
    }
    let remotes = |app: &mut App| app.world_mut().query::<&Remote>().iter(app.world()).count();
    assert_eq!(remotes(&mut pair.host), 1);
    // The connection goes, and with it the player's game.
    pair.host.world_mut().resource_mut::<Inbox>().left.push(1);
    let left = car(&mut pair.host, GUESTS).pos;
    for _ in 0..240 {
        pair.hub.step();
        pair.host.update();
    }
    assert_eq!(remotes(&mut pair.host), 0);
    assert!(pair.host.world().resource::<Session>().members.is_empty());
    assert!(car(&mut pair.host, GUESTS).pos.distance(left) > 20.0, "the computer should have driven the car on");
}

#[test]
fn players_take_the_grid_from_the_back() {
    let players: Vec<(Peer, String)> = (0..3).map(|peer| (peer, format!("P{peer}"))).collect();
    let seats = Lineup::seat(&players, 5);
    let at = |slot: u8| seats.iter().find(|seat| seat.slot == slot).map(|seat| seat.peer);
    // Three players at the back, and only three of the five computer's cars fit.
    assert_eq!([at(5), at(4), at(3)], [Some(Some(0)), Some(Some(1)), Some(Some(2))]);
    assert_eq!([at(0), at(1), at(2)], [Some(None); 3]);
    assert_eq!(seats.len(), 6);
    let lineup = Lineup { seats, you: 1 };
    assert_eq!(lineup.driver(4), Some((Who::Local, Some("P1".into()))));
    assert_eq!(lineup.driver(5), Some((Who::Remote(0), Some("P0".into()))));
    assert_eq!(lineup.driver(0), Some((Who::Computer, None)));
}

/// On a screen drawn twice as often as the race is stepped, a car moves every frame
/// and by much the same each time, and the race is none the different for it.
#[test]
fn a_car_moves_evenly_on_a_screen_faster_than_the_race() {
    let run = |frames: u32| {
        let hub = Hub::new(0, 0.0);
        let mut app = game_drawn(Role::Host, hub.host(), HOST, 0, frames);
        app.world_mut().resource_mut::<Session>().members.clear();
        let mut moved: Vec<f32> = Vec::new();
        let mut last = None;
        for _ in 0..(240 * frames) {
            press(&mut app, KeyCode::KeyW, true);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
            // Where the car is drawn: where it is when the frame's work is done.
            let at = car(&mut app, HOSTS).pos;
            moved.extend(last.map(|last: Vec3| at.distance(last)));
            last = Some(at);
        }
        // Where the race has it: where it is once put back for the next step.
        app.world_mut().run_system_cached(display::unblend).unwrap();
        (moved, car(&mut app, HOSTS).pos, app.world().resource::<Clock>().tick)
    };
    let (moved, drawn_twice, steps) = run(2);
    let late: Vec<f32> = moved[moved.len() - 60..].to_vec();
    let (least, most) = (late.iter().copied().fold(f32::MAX, f32::min), late.iter().copied().fold(0.0, f32::max));
    assert!(most > 0.05, "the car should be moving, and moves {most} a frame");
    assert!(least > most * 0.8, "the car moves between {least} and {most} a frame");
    // The same race drawn once a step ends in the same place.
    let (_, drawn_once, steps_once) = run(1);
    assert!(steps.abs_diff(steps_once) <= 1, "{steps} steps against {steps_once}");
    assert!(drawn_twice.distance(drawn_once) < 0.5, "the race went {} differently", drawn_twice.distance(drawn_once));
}

/// A player joins a session, giving its password in capitals where the host set it
/// in small letters, is let into its room, says what they want and that they
/// are ready; the host is ready too, and the race the vote settles on begins for both.
#[test]
fn a_room_votes_and_its_race_begins_for_everyone() {
    let hub = Hub::new(DELAY, 0.0);
    let (host_link, guest_link) = (hub.host(), hub.join());
    let mut pair = Pair { host: game(Role::Host, host_link, HOST, 0), guest: game(Role::Client, guest_link, 0, 0), hub };
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    for (app, name, password) in [(&mut pair.host, "HOSTY", "bricks"), (&mut pair.guest, "GUESTY", "BRICKS")] {
        // Both are at the menu, in no race, and the host alone in its room.
        app.insert_state(Screen::Menu);
        app.world_mut().remove_resource::<Lineup>();
        let hosting = name == "HOSTY";
        *app.world_mut().resource_mut::<Session>() = Session { title: "Friday".into(), name: name.into(), password: password.into(), ..default() };
        let voters = if hosting { vec![Voter { peer: HOST, name: name.into(), ready: false, ballot: None }] } else { Vec::new() };
        *app.world_mut().resource_mut::<Room>() = Room { voters, ..default() };
    }
    let circuits = Circuits::find();
    let wish = |circuit: usize, laps: u8, opponents: u8| {
        let settings = Settings { circuit, ..Settings::new(&circuits) };
        Rules { lap_choice: laps, opponents, ..Rules::of(&settings, &circuits) }
    };
    let (hosts, guests) = (wish(0, 0, 1), wish(circuits.0.len() - 1, 2, 3));
    pair.host.world_mut().resource_mut::<Room>().ballot = Some(hosts.clone());
    for _ in 0..40 {
        pair.step();
    }
    // The player has been let in and told who is in the room.
    assert_eq!(pair.guest.world().resource::<Session>().you, 1);
    let names = |app: &App| app.world().resource::<Room>().voters.iter().map(|voter| voter.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&pair.guest), ["HOSTY", "GUESTY"]);
    assert_eq!(names(&pair.host), names(&pair.guest));

    // The player votes and is ready: the host hears, and the clock starts for both.
    (pair.guest.world_mut().resource_mut::<Room>().ballot, pair.guest.world_mut().resource_mut::<Room>().ready) = (Some(guests.clone()), true);
    for _ in 0..40 {
        pair.step();
    }
    let heard = pair.host.world().resource::<Room>().voters[1].clone();
    assert_eq!((heard.ballot.as_ref(), heard.ready), (Some(&guests), true));
    assert!(pair.guest.world().resource::<Room>().closing.is_some(), "the player should see the vote closing");
    assert_eq!((state(&pair.host), state(&pair.guest)), (Screen::Menu, Screen::Menu));

    // The host is ready too: the vote closes and the race begins for both.
    pair.host.world_mut().resource_mut::<Room>().ready = true;
    for _ in 0..40 {
        pair.step();
    }
    assert_eq!((state(&pair.host), state(&pair.guest)), (Screen::Loading, Screen::Loading));
    let raced = pair.host.world().resource::<Room>().last.clone().expect("a race decided on");
    // One of the two circuits asked for, and on a tie the host's wishes for the rest.
    assert!(raced.circuit == hosts.circuit || raced.circuit == guests.circuit);
    assert_eq!((raced.lap_choice, raced.opponents), (0, 1));
    for app in [&pair.host, &pair.guest] {
        let lineup = app.world().resource::<Lineup>();
        assert_eq!(lineup.seats.iter().filter(|seat| seat.peer.is_some()).count(), 2);
        assert_eq!(lineup.seats.len(), 3);
        let settings = app.world().resource::<Settings>();
        assert_eq!((settings.opponents, settings.lap_choice), (1, 0));
    }
    assert_eq!(pair.guest.world().resource::<Settings>().circuit, pair.host.world().resource::<Settings>().circuit);
}

/// The wrong password is turned away, and the game is its own again.
#[test]
fn the_wrong_password_is_turned_away() {
    let hub = Hub::new(1, 0.0);
    let (host_link, guest_link) = (hub.host(), hub.join());
    let mut pair = Pair { host: game(Role::Host, host_link, HOST, 0), guest: game(Role::Client, guest_link, 0, 0), hub };
    for (app, password) in [(&mut pair.host, "bricks"), (&mut pair.guest, "studs")] {
        app.insert_state(Screen::Menu);
        *app.world_mut().resource_mut::<Session>() = Session { name: "X".into(), password: password.into(), ..default() };
    }
    for _ in 0..20 {
        pair.step();
    }
    assert_eq!(*pair.guest.world().resource::<Role>(), Role::Offline);
    assert_eq!(pair.guest.world().resource::<Session>().notice.as_deref(), Some("Wrong password"));
    assert!(pair.guest.world().get_resource::<Wire>().is_none());
    assert!(pair.host.world().resource::<Session>().members.is_empty());
}
