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
    let players = [
        (HOST, "Host".to_string(), Ride::Slot),
        (1, "Guest".to_string(), Ride::Driver("RR".into())),
    ];
    let members = if role == Role::Host {
        vec![Member {
            peer: 1,
            name: "Guest".into(),
            car: Ride::Driver("RR".into()),
            loaded: true,
        }]
    } else {
        Vec::new()
    };
    app.add_plugins((MinimalPlugins, StatesPlugin, plugin))
        // Each update is one step of the race, exactly.
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::from_hz(TICKS).timestep() / frames,
        ))
        .insert_state(Screen::Race)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::input::Actions>()
        .init_resource::<crate::input::Devices>()
        .add_systems(PreUpdate, crate::input::read)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<crate::audio::Sfx>()
        .init_resource::<crate::variant::Variant>()
        .init_resource::<crate::cheats::Raced>()
        .init_resource::<Pause>()
        .init_resource::<crate::replay::Photo>()
        .insert_resource(Track::new())
        .insert_resource(crate::meshgen::Rng(7))
        .insert_resource(Race {
            phase: Phase::Intro,
            intro: 0.0,
            countdown: 0.0,
            time: 0.0,
            demo: false,
            quick: true,
        })
        .insert_resource(circuits)
        .insert_resource(settings)
        .insert_resource(role)
        .insert_resource(Wire(Box::new(link)))
        .insert_resource(Session {
            name: "Me".into(),
            you,
            members,
            ..default()
        })
        .insert_resource(Room {
            voters: vec![Voter {
                peer: HOST,
                name: "Me".into(),
                ready: false,
                ballot: None,
                link: None,
                points: 0,
            }],
            ..default()
        })
        .insert_resource(Lineup {
            seats: Lineup::seat(&players, opponents),
            you,
        })
        .add_systems(Startup, (kart::spawn_karts, items::setup_items).chain());
    // With no renderer there is nothing to say that what has a mesh can be seen or
    // hidden, which is how a brick comes and goes.
    app.register_required_components::<items::Pickup, Visibility>();
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
        Pair {
            host: game(Role::Host, host, HOST, opponents),
            guest: game(Role::Client, guest, 1, opponents),
            hub,
        }
    }

    /// One step of time for the network and for both games.
    fn step(&mut self) {
        self.hub.step();
        for app in [&mut self.host, &mut self.guest] {
            app.update();
            // No window is there to say the keys' presses are a frame old.
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
        }
    }
}

fn press(app: &mut App, key: KeyCode, down: bool) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if down {
        keys.press(key)
    } else {
        keys.release(key)
    }
}

/// The car in a grid slot, in one of the games.
fn car(app: &mut App, slot: usize) -> &Kart {
    app.world_mut()
        .query::<&Kart>()
        .iter(app.world())
        .find(|k| k.slot == slot)
        .expect("a car in the slot")
}

fn car_mut(app: &mut App, slot: usize) -> Mut<'_, Kart> {
    app.world_mut()
        .query::<&mut Kart>()
        .iter_mut(app.world_mut())
        .find(|k| k.slot == slot)
        .expect("a car in the slot")
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
        press(
            &mut pair.guest,
            KeyCode::ShiftLeft,
            (280..330).contains(&step),
        );
        pair.step();
        let corrected = std::mem::take(
            &mut pair
                .guest
                .world_mut()
                .resource_mut::<Prediction>()
                .corrected,
        );
        worst = worst.max(corrected);
        corrections += (corrected > 0.01) as usize;
    }
    let (there, here) = (
        car(&mut pair.host, GUESTS).pos,
        car(&mut pair.guest, GUESTS).pos,
    );
    // The host has the car where the player's game had it a moment ago.
    assert!(
        there.distance(here) < 12.0,
        "the host has the car {} away",
        there.distance(here)
    );
    assert!(pair.guest.world().resource::<Race>().phase == Phase::Racing);
    (here.distance(from), worst, corrections)
}

/// Driving alone on the road, a player's game steps its car exactly as the host
/// does, however late the word between them: nothing is ever corrected.
#[test]
fn a_players_own_car_needs_no_correcting() {
    let (went, worst, _) = drive_about(&mut Pair::new(0, 0.0));
    assert!(
        went > 40.0,
        "the car should have been driven somewhere, and went {went}"
    );
    assert!(worst < 0.01, "the car was corrected by {worst}");
}

/// When a step's pressing is late, the host drives the car on without it, and the
/// player's game is put right by a step's travel: a little, and seldom, since the
/// host then has a step in hand.
#[test]
fn a_poor_connection_costs_a_step_now_and_then() {
    let (went, worst, corrections) = drive_about(&mut Pair::new(0, LOSS));
    assert!(
        went > 40.0,
        "the car should have been driven somewhere, and went {went}"
    );
    assert!(worst < 0.6, "the car was corrected by {worst}");
    assert!(corrections < 8, "the car was corrected {corrections} times");
}

/// The cars a player doesn't drive are shown where they will be by the moment the
/// player's own car is at: a little ahead of where the host has them as it says so.
#[test]
fn the_other_cars_are_shown_where_they_are_about_to_be() {
    let mut pair = Pair::new(2, LOSS);
    let mut trail: Vec<Vec3> = Vec::new();
    let mut shown = Vec3::ZERO;
    for step in 0..360 {
        press(&mut pair.host, KeyCode::KeyW, true);
        pair.step();
        trail.push(car(&mut pair.host, HOSTS).pos);
        if step == 330 {
            shown = car(&mut pair.guest, HOSTS).pos;
            // Ahead of where the host has it at this moment, the way it is going.
            let (there, going) = (
                car(&mut pair.host, HOSTS).pos,
                car(&mut pair.host, HOSTS).vel,
            );
            assert!(
                (shown - there).dot(going) > 0.0,
                "the host's car should be shown ahead of where the host has it"
            );
        }
    }
    assert!(
        trail[0].distance(*trail.last().unwrap()) > 30.0,
        "the host's car should have been driven somewhere"
    );
    // And the host's car duly goes through where it was shown.
    let nearest = trail
        .iter()
        .skip(330)
        .map(|at| at.distance(shown))
        .reduce(f32::min)
        .unwrap();
    assert!(
        nearest < 0.5,
        "the host's car was shown {nearest} from anywhere it then went"
    );
    // The computer's cars too, which only the host drives, and the order they are in.
    for slot in 0..2 {
        let (there, here) = (
            car(&mut pair.host, slot).pos,
            car(&mut pair.guest, slot).pos,
        );
        assert!(
            here.distance(Kart::new(&Track::new(), slot).pos) > 5.0,
            "the computer's car {slot} should be seen to have moved"
        );
        assert!(there.distance(here) < 15.0);
        assert_eq!(
            car(&mut pair.host, slot).place,
            car(&mut pair.guest, slot).place
        );
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
    assert!(
        car(&mut pair.guest, GUESTS)
            .pos
            .distance(car(&mut pair.host, GUESTS).pos)
            < 0.5,
        "the player's game should have its car where the host put it"
    );
    (
        car_mut(&mut pair.host, HOSTS).held,
        car_mut(&mut pair.host, HOSTS).whites,
    ) = (Some(Power::Red), 0);
    press(&mut pair.host, KeyCode::Space, true);
    let (mut struck_there, mut struck_here, mut seen_flying, mut whooshed) =
        (false, false, false, false);
    let mut path: Vec<Vec3> = Vec::new();
    let before = car(&mut pair.guest, GUESTS).pos;
    let flying = |app: &mut App| {
        app.world_mut()
            .query::<&crate::items::Action>()
            .iter(app.world())
            .filter(|action| matches!(action, crate::items::Action::Cannonball { .. }))
            .count()
    };
    for _ in 0..180 {
        pair.step();
        press(&mut pair.host, KeyCode::Space, false);
        seen_flying |= flying(&mut pair.guest) == 1;
        // Where the player's game shows the cannonball, each frame it is there, and
        // the flight loop the host says is sounding.
        let shown = pair
            .guest
            .world_mut()
            .query::<(&crate::items::Action, &Transform)>()
            .iter(pair.guest.world())
            .find(|(action, _)| matches!(action, crate::items::Action::Cannonball { .. }))
            .map(|(_, at)| at.translation);
        path.extend(shown);
        whooshed |= !pair
            .guest
            .world()
            .resource::<scene::Shown>()
            .loops()
            .is_empty();
        let knocked =
            |k: &Kart| k.spin > 0.0 || k.spin_out > 0.0 || k.contacts == 0 || k.vel.length() > 3.0;
        struck_there |= knocked(car(&mut pair.host, GUESTS));
        struck_here |= knocked(car(&mut pair.guest, GUESTS));
    }
    assert!(
        car(&mut pair.host, HOSTS).held.is_none(),
        "the host should have fired"
    );
    assert!(
        struck_there,
        "the cannonball should have hit the car on the host"
    );
    assert!(struck_here, "and the player's game should have seen it");
    // The cannonball itself was there to be seen on its way, and is gone now.
    assert!(
        seen_flying,
        "the player's game should have shown the cannonball in flight"
    );
    assert_eq!(flying(&mut pair.guest), 0);
    // It moved every frame, though the host tells of it only every other step, and
    // its flight was heard.
    let still = path
        .windows(2)
        .skip(2)
        .filter(|pair| pair[0] == pair[1])
        .count();
    assert!(
        path.len() > 6 && still == 0,
        "the cannonball stood still {still} frames of {}",
        path.len()
    );
    assert!(whooshed, "the cannonball's flight should have been heard");
    // And the driver it hit is owed a grumble on the player's game as on the host.
    assert_eq!(car(&mut pair.guest, GUESTS).cues.reaction, Some(false));
    assert!(car(&mut pair.guest, GUESTS).pos.distance(before) > 0.2);
    assert!(
        car(&mut pair.guest, GUESTS)
            .pos
            .distance(car(&mut pair.host, GUESTS).pos)
            < 0.5
    );
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
    assert!(
        car(&mut pair.host, GUESTS).pos.distance(left) > 20.0,
        "the computer should have driven the car on"
    );
}

#[test]
fn players_take_the_grid_from_the_back() {
    let players: Vec<(Peer, String, Ride)> = (0..3)
        .map(|peer| {
            (
                peer,
                format!("P{peer}"),
                if peer == 1 {
                    Ride::Driver("RR".into())
                } else {
                    Ride::Slot
                },
            )
        })
        .collect();
    let seats = Lineup::seat(&players, 5);
    let at = |slot: u8| {
        seats
            .iter()
            .find(|seat| seat.slot == slot)
            .map(|seat| seat.peer)
    };
    // Three players at the back, and only three of the five computer's cars fit.
    assert_eq!(
        [at(5), at(4), at(3)],
        [Some(Some(0)), Some(Some(1)), Some(Some(2))]
    );
    assert_eq!([at(0), at(1), at(2)], [Some(None); 3]);
    assert_eq!(seats.len(), 6);
    let lineup = Lineup { seats, you: 1 };
    // Only the player who chose who to race as is cast as anyone.
    assert_eq!(
        lineup.cast().collect::<Vec<_>>(),
        [(4, &Ride::Driver("RR".into()))]
    );
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
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            // Where the car is drawn: where it is when the frame's work is done.
            let at = car(&mut app, HOSTS).pos;
            moved.extend(last.map(|last: Vec3| at.distance(last)));
            last = Some(at);
        }
        // Where the race has it: where it is once put back for the next step.
        app.world_mut().run_system_cached(display::unblend).unwrap();
        (
            moved,
            car(&mut app, HOSTS).pos,
            app.world().resource::<Clock>().tick,
        )
    };
    let (moved, drawn_twice, steps) = run(2);
    let late: Vec<f32> = moved[moved.len() - 60..].to_vec();
    let (least, most) = (
        late.iter().copied().fold(f32::MAX, f32::min),
        late.iter().copied().fold(0.0, f32::max),
    );
    assert!(
        most > 0.05,
        "the car should be moving, and moves {most} a frame"
    );
    assert!(
        least > most * 0.8,
        "the car moves between {least} and {most} a frame"
    );
    // The same race drawn once a step ends in the same place.
    let (_, drawn_once, steps_once) = run(1);
    assert!(
        steps.abs_diff(steps_once) <= 1,
        "{steps} steps against {steps_once}"
    );
    assert!(
        drawn_twice.distance(drawn_once) < 0.5,
        "the race went {} differently",
        drawn_twice.distance(drawn_once)
    );
}

/// A player joins a session, giving its password in capitals where the host set it
/// in small letters, is let into its room, says what they want and that they
/// are ready; the host is ready too, and the race the vote settles on begins for both.
#[test]
fn a_room_votes_and_its_race_begins_for_everyone() {
    let hub = Hub::new(DELAY, 0.0);
    let (host_link, guest_link) = (hub.host(), hub.join());
    let mut pair = Pair {
        host: game(Role::Host, host_link, HOST, 0),
        guest: game(Role::Client, guest_link, 0, 0),
        hub,
    };
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    for (app, name, password) in [
        (&mut pair.host, "HOSTY", "bricks"),
        (&mut pair.guest, "GUESTY", "BRICKS"),
    ] {
        // Both are at the menu, in no race, and the host alone in its room.
        app.insert_state(Screen::Menu);
        app.world_mut().remove_resource::<Lineup>();
        let hosting = name == "HOSTY";
        *app.world_mut().resource_mut::<Session>() = Session {
            title: "Friday".into(),
            name: name.into(),
            password: password.into(),
            ..default()
        };
        let voters = if hosting {
            vec![Voter {
                peer: HOST,
                name: name.into(),
                ready: false,
                ballot: None,
                link: None,
                points: 0,
            }]
        } else {
            Vec::new()
        };
        *app.world_mut().resource_mut::<Room>() = Room {
            voters,
            ..default()
        };
    }
    let circuits = Circuits::find();
    let wish = |circuit: usize, laps: u8, opponents: u8| {
        let settings = Settings {
            circuit,
            ..Settings::new(&circuits)
        };
        Rules {
            lap_choice: laps,
            opponents,
            ..Rules::of(&settings, &circuits)
        }
    };
    let (hosts, guests) = (wish(0, 0, 1), wish(circuits.0.len() - 1, 2, 3));
    pair.host.world_mut().resource_mut::<Room>().ballot = Some(hosts.clone());
    for _ in 0..40 {
        pair.step();
    }
    // The player has been let in and told who is in the room.
    assert_eq!(pair.guest.world().resource::<Session>().you, 1);
    let names = |app: &App| {
        app.world()
            .resource::<Room>()
            .voters
            .iter()
            .map(|voter| voter.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&pair.guest), ["HOSTY", "GUESTY"]);
    assert_eq!(names(&pair.host), names(&pair.guest));
    // And how good each player's way to the host is, the host having none to itself.
    let links: Vec<Option<Quality>> = pair
        .guest
        .world()
        .resource::<Room>()
        .voters
        .iter()
        .map(|voter| voter.link)
        .collect();
    assert_eq!(
        links,
        [
            None,
            Some(Quality {
                ping: 165,
                direct: true
            })
        ]
    );

    // The player votes and is ready: the host hears, and nobody is hurried by it.
    (
        pair.guest.world_mut().resource_mut::<Room>().ballot,
        pair.guest.world_mut().resource_mut::<Room>().ready,
    ) = (Some(guests.clone()), true);
    for _ in 0..40 {
        pair.step();
    }
    let heard = pair.host.world().resource::<Room>().voters[1].clone();
    assert_eq!((heard.ballot.as_ref(), heard.ready), (Some(&guests), true));
    assert!(
        pair.guest.world().resource::<Room>().closing.is_none(),
        "one of two being ready should start no clock"
    );
    assert_eq!(
        (state(&pair.host), state(&pair.guest)),
        (Screen::Menu, Screen::Menu)
    );

    // The host is ready too: the vote closes and the race begins for both.
    pair.host.world_mut().resource_mut::<Room>().ready = true;
    for _ in 0..40 {
        pair.step();
    }
    assert_eq!(
        (state(&pair.host), state(&pair.guest)),
        (Screen::Loading, Screen::Loading)
    );
    let raced = pair
        .host
        .world()
        .resource::<Room>()
        .last
        .clone()
        .expect("a race decided on");
    // One of the two circuits voted for, they being level, and the host's wishes for
    // the rest.
    assert!(raced.circuit == hosts.circuit || raced.circuit == guests.circuit);
    assert_eq!((raced.lap_choice, raced.opponents), (0, 1));
    for app in [&pair.host, &pair.guest] {
        let lineup = app.world().resource::<Lineup>();
        assert_eq!(
            lineup
                .seats
                .iter()
                .filter(|seat| seat.peer.is_some())
                .count(),
            2
        );
        assert_eq!(lineup.seats.len(), 3);
        let settings = app.world().resource::<Settings>();
        assert_eq!((settings.opponents, settings.lap_choice), (1, 0));
    }
    assert_eq!(
        pair.guest.world().resource::<Settings>().circuit,
        pair.host.world().resource::<Settings>().circuit
    );
}

/// The wrong password is turned away, and the game is its own again.
#[test]
fn the_wrong_password_is_turned_away() {
    let hub = Hub::new(1, 0.0);
    let (host_link, guest_link) = (hub.host(), hub.join());
    let mut pair = Pair {
        host: game(Role::Host, host_link, HOST, 0),
        guest: game(Role::Client, guest_link, 0, 0),
        hub,
    };
    for (app, password) in [(&mut pair.host, "bricks"), (&mut pair.guest, "studs")] {
        app.insert_state(Screen::Menu);
        *app.world_mut().resource_mut::<Session>() = Session {
            name: "X".into(),
            password: password.into(),
            ..default()
        };
    }
    for _ in 0..20 {
        pair.step();
    }
    assert_eq!(*pair.guest.world().resource::<Role>(), Role::Offline);
    assert_eq!(
        pair.guest.world().resource::<Session>().notice.as_deref(),
        Some("Wrong password")
    );
    assert!(pair.guest.world().get_resource::<Wire>().is_none());
    assert!(pair.host.world().resource::<Session>().members.is_empty());
}

/// A brick taken on the host is taken on a player's screen too, and heard there, and
/// comes back when the host has it back.
#[test]
fn a_brick_taken_on_the_host_goes_from_the_players_screen() {
    use crate::items::Pickup;
    let mut pair = Pair::new(0, 0.0);
    for _ in 0..90 {
        pair.step();
    }
    let bricks = |app: &mut App| {
        app.world_mut()
            .query::<&Pickup>()
            .iter(app.world())
            .map(|brick| (brick.number(), brick.at()))
            .collect::<Vec<_>>()
    };
    let (theirs, ours) = (bricks(&mut pair.host), bricks(&mut pair.guest));
    assert!(!theirs.is_empty(), "the circuit should have bricks");
    // The same bricks, numbered alike, on both.
    assert_eq!(
        theirs.iter().map(|b| (b.0, b.1.0)).collect::<Vec<_>>(),
        ours.iter().map(|b| (b.0, b.1.0)).collect::<Vec<_>>()
    );
    let idle = theirs.iter().filter(|brick| brick.1.1).count();
    let &(number, (at, _)) = theirs
        .iter()
        .find(|brick| brick.1.1)
        .unwrap_or_else(|| panic!("a brick should be there to be taken, of {}", theirs.len()));
    assert!(idle > 1);
    let there = |app: &mut App| {
        bricks(app)
            .iter()
            .find(|brick| brick.0 == number)
            .map(|brick| brick.1.1)
    };
    assert_eq!(there(&mut pair.guest), Some(true));

    // The host's car is put on the brick. The player's game is listened to.
    pair.guest
        .world_mut()
        .resource_mut::<crate::audio::Sfx>()
        .listening = true;
    (
        car_mut(&mut pair.host, HOSTS).pos,
        car_mut(&mut pair.host, HOSTS).vel,
    ) = (at, Vec3::ZERO);
    for _ in 0..40 {
        pair.step();
    }
    assert!(
        car(&mut pair.host, HOSTS).held.is_some() || car(&mut pair.host, HOSTS).whites > 0,
        "the host's car should have taken the brick"
    );
    assert_eq!(there(&mut pair.host), Some(false));
    assert_eq!(
        there(&mut pair.guest),
        Some(false),
        "the brick should be gone from the player's screen"
    );
    let heard = pair
        .guest
        .world()
        .resource::<crate::audio::Sfx>()
        .heard
        .clone();
    assert!(
        heard
            .iter()
            .any(|(_, emitter)| emitter.pos.distance(at) < 3.0),
        "the player should have heard the brick taken"
    );
    // The car is shown holding what it took.
    assert_eq!(
        car(&mut pair.guest, HOSTS).held,
        car(&mut pair.host, HOSTS).held
    );

    // Moved off it, the brick comes back, on both.
    car_mut(&mut pair.host, HOSTS).pos = at + Vec3::X * 30.0;
    for _ in 0..400 {
        pair.step();
        car_mut(&mut pair.host, HOSTS).vel = Vec3::ZERO;
    }
    assert_eq!(
        (there(&mut pair.host), there(&mut pair.guest)),
        (Some(true), Some(true))
    );
}

/// A player who drives into another car is stopped by it on their own screen, and
/// doesn't pass through it while waiting to hear from the host.
#[test]
fn a_players_car_does_not_drive_through_another() {
    let mut pair = Pair::new(0, 0.0);
    for _ in 0..30 {
        pair.step();
    }
    // The host's car stands in the road, and the player's some way behind it.
    let track = Track::new();
    car_mut(&mut pair.host, HOSTS).place(&track, 60.0, 0.0);
    car_mut(&mut pair.host, GUESTS).place(&track, 20.0, 0.0);
    for _ in 0..40 {
        pair.step();
    }
    // The cars' middles are 2.4 apart at the nearest they can be without overlapping.
    let (mut nearest, mut through) = (f32::MAX, 0);
    for step in 0..300 {
        press(&mut pair.guest, KeyCode::KeyW, true);
        pair.step();
        let apart = car(&mut pair.guest, GUESTS)
            .pos
            .distance(car(&mut pair.guest, HOSTS).pos);
        through += (apart < 2.0) as usize;
        // The first time they meet, before anything has been shunted anywhere.
        if step < 120 {
            nearest = nearest.min(apart);
        }
    }
    let moved = car(&mut pair.host, HOSTS)
        .pos
        .distance(track.surface_point(60.0, 0.0));
    assert!(
        moved > 1.0,
        "the player's car should have run into the host's and shunted it, which moved {moved}"
    );
    assert!(
        nearest > 2.4,
        "the player's car first met the other {nearest} from its middle"
    );
    // Shunting it on down the road, the two are shown overlapping for a frame now and
    // then, when the host's word and the guess at it part company.
    assert!(
        through <= 3,
        "the player's car was shown inside the other for {through} frames"
    );
}

/// The host calling a race off takes everyone back to the session's room, still in
/// the session, and not out to the menu on their own.
#[test]
fn a_race_the_host_calls_off_takes_everyone_back_to_the_room() {
    let mut pair = Pair::new(0, 0.0);
    for _ in 0..60 {
        pair.step();
    }
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    assert_eq!(
        (state(&pair.host), state(&pair.guest)),
        (Screen::Race, Screen::Race)
    );
    fn off(
        mut session: ResMut<Session>,
        mut wire: ResMut<Wire>,
        mut next: ResMut<NextState<Screen>>,
    ) {
        call_off(&mut session, &mut wire, &mut next);
    }
    pair.host.world_mut().run_system_cached(off).unwrap();
    for _ in 0..20 {
        pair.step();
    }
    assert_eq!(
        (state(&pair.host), state(&pair.guest)),
        (Screen::Menu, Screen::Menu)
    );
    // Both are still in the session, the player still on the host's list and nothing
    // said about the host having gone.
    assert_eq!(
        (
            *pair.host.world().resource::<Role>(),
            *pair.guest.world().resource::<Role>()
        ),
        (Role::Host, Role::Client)
    );
    assert_eq!(pair.host.world().resource::<Session>().members.len(), 1);
    assert!(
        pair.guest.world().get_resource::<Wire>().is_some()
            && pair.guest.world().resource::<Session>().notice.is_none()
    );
}

/// A player who gives a race up is back in the room and still in the session, and
/// their car is the computer's for the rest of the race, which goes on.
#[test]
fn a_player_who_gives_the_race_up_is_back_in_the_room() {
    let mut pair = Pair::new(0, 0.0);
    for _ in 0..60 {
        pair.step();
    }
    fn give_up(mut wire: ResMut<Wire>, mut next: ResMut<NextState<Screen>>) {
        retire(&mut wire, &mut next);
    }
    pair.guest.world_mut().run_system_cached(give_up).unwrap();
    for _ in 0..30 {
        pair.step();
    }
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    assert_eq!(
        (state(&pair.host), state(&pair.guest)),
        (Screen::Race, Screen::Menu)
    );
    assert_eq!(*pair.guest.world().resource::<Role>(), Role::Client);
    assert!(
        pair.guest.world().resource::<Room>().racing,
        "the room should say a race is on"
    );
    assert_eq!(pair.host.world().resource::<Session>().members.len(), 1);
    assert_eq!(
        pair.host
            .world_mut()
            .query::<&Remote>()
            .iter(pair.host.world())
            .count(),
        0
    );
}

/// A player the host removes is told why and is out of the session.
#[test]
fn a_player_the_host_removes_is_out_of_the_session() {
    let mut pair = Pair::new(0, 0.0);
    for _ in 0..30 {
        pair.step();
    }
    pair.host
        .world_mut()
        .resource_mut::<Session>()
        .removing
        .push(1);
    for _ in 0..30 {
        pair.step();
    }
    assert_eq!(*pair.guest.world().resource::<Role>(), Role::Offline);
    assert_eq!(
        pair.guest.world().resource::<Session>().notice.as_deref(),
        Some("The host removed you from the session")
    );
    assert!(pair.host.world().resource::<Session>().members.is_empty());
    assert_eq!(
        pair.host
            .world_mut()
            .query::<&Remote>()
            .iter(pair.host.world())
            .count(),
        0
    );
}

/// A session that is full by the host's own count takes nobody else.
#[test]
fn a_session_takes_as_many_as_its_host_says() {
    let hub = Hub::new(1, 0.0);
    let (host_link, guest_link) = (hub.host(), hub.join());
    let mut pair = Pair {
        host: game(Role::Host, host_link, HOST, 0),
        guest: game(Role::Client, guest_link, 0, 0),
        hub,
    };
    for app in [&mut pair.host, &mut pair.guest] {
        app.insert_state(Screen::Menu);
    }
    // The host and the one player it has already are the two it will take.
    pair.host.world_mut().resource_mut::<Session>().limit = Some(2);
    for _ in 0..20 {
        pair.step();
    }
    assert_eq!(
        pair.guest.world().resource::<Session>().notice.as_deref(),
        Some("The session is full")
    );
}

/// When a race is run everyone is told how it went: the order, each car's time,
/// and nothing for a car that never came home.
#[test]
fn a_race_run_is_told_to_everyone_with_its_times() {
    let mut pair = Pair::new(1, 0.0);
    for _ in 0..60 {
        pair.step();
    }
    // Both players are home, the guest a moment after the host.
    (
        car_mut(&mut pair.host, HOSTS).finished,
        car_mut(&mut pair.host, GUESTS).finished,
    ) = (Some(0.5), Some(0.8));
    for _ in 0..400 {
        pair.step();
    }
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    assert_eq!(
        (state(&pair.host), state(&pair.guest)),
        (Screen::Menu, Screen::Menu)
    );
    for app in [&pair.host, &pair.guest] {
        let room = app.world().resource::<Room>();
        let told: Vec<(&str, bool, Option<f32>)> = room
            .results
            .iter()
            .map(|finish| (&*finish.name, finish.player, finish.time))
            .collect();
        assert_eq!(
            told[..2],
            [("Host", true, Some(0.5)), ("Guest", true, Some(0.8))]
        );
        // The computer's car was still out, and has a place but no time.
        assert_eq!((told.len(), told[2].1, told[2].2), (3, false, None));
        assert!(room.fresh, "the results should be waiting to be shown");
        // Each place has scored as the original's circuits score them.
        let scored: Vec<u32> = room.results.iter().map(|finish| finish.points).collect();
        assert_eq!(scored, [30, 20, 10]);
    }
    // The host, who is in its own room, has its points to keep.
    assert_eq!(pair.host.world().resource::<Room>().voters[0].points, 30);
}

/// What is said in the room is heard by everyone in it, with who said it.
#[test]
fn what_is_said_in_the_room_is_heard_by_everyone() {
    let mut pair = Pair::new(0, 0.0);
    for app in [&mut pair.host, &mut pair.guest] {
        app.insert_state(Screen::Menu);
    }
    fn speak(
        role: Res<Role>,
        session: Res<Session>,
        mut room: ResMut<Room>,
        mut wire: ResMut<Wire>,
    ) {
        say(*role, &session, &mut room, &mut wire, "  good luck  ");
    }
    pair.guest.world_mut().run_system_cached(speak).unwrap();
    pair.host.world_mut().run_system_cached(speak).unwrap();
    for _ in 0..30 {
        pair.step();
    }
    for app in [&pair.host, &pair.guest] {
        let mut said = app.world().resource::<Room>().chat.clone();
        said.sort();
        assert_eq!(said, ["Guest: good luck", "Me: good luck"]);
    }
}

/// A player who gave a race up may go back to it, and has their car again.
#[test]
fn a_player_who_gave_the_race_up_may_go_back_to_it() {
    let mut pair = Pair::new(0, 0.0);
    for _ in 0..60 {
        pair.step();
    }
    fn give_up(mut wire: ResMut<Wire>, mut next: ResMut<NextState<Screen>>) {
        retire(&mut wire, &mut next);
    }
    fn go_back(mut wire: ResMut<Wire>) {
        enter(&mut wire);
    }
    pair.guest.world_mut().run_system_cached(give_up).unwrap();
    for _ in 0..30 {
        pair.step();
    }
    let driven = |app: &mut App| app.world_mut().query::<&Remote>().iter(app.world()).count();
    assert_eq!(driven(&mut pair.host), 0);
    // The host knows what race is on, as it does of one it began.
    let circuits = Circuits::find();
    pair.host.world_mut().resource_mut::<Session>().racing =
        Some(Rules::of(&Settings::new(&circuits), &circuits));
    pair.guest.world_mut().run_system_cached(go_back).unwrap();
    for _ in 0..30 {
        pair.step();
    }
    // The host has sent the race again, and the player's game is loading it.
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    assert_eq!(state(&pair.guest), Screen::Loading);
    // Loaded (here, the cars are still there from before), it says so and drives.
    pair.guest.insert_state(Screen::Race);
    pair.guest
        .world_mut()
        .run_system_cached(client::loaded)
        .unwrap();
    for _ in 0..30 {
        pair.step();
    }
    assert_eq!(driven(&mut pair.host), 1);
    assert!(pair.host.world().resource::<Session>().watching.is_empty());
}

/// Someone with no car in the race watches it: they are told where every car is,
/// and their screen is on the leader's.
#[test]
fn someone_with_no_car_in_the_race_watches_it() {
    let mut pair = Pair::new(1, 0.0);
    // The race is the host's and two of the computer's cars: the player isn't in it.
    let alone = Lineup::seat(&[(HOST, "Host".to_string(), Ride::Slot)], 2);
    pair.host.insert_resource(Lineup {
        seats: alone.clone(),
        you: HOST,
    });
    pair.guest.insert_resource(Lineup {
        seats: alone,
        you: 1,
    });
    pair.step();
    pair.guest
        .world_mut()
        .run_system_cached(client::loaded)
        .unwrap();
    press(&mut pair.host, KeyCode::KeyW, true);
    for _ in 0..180 {
        pair.step();
    }
    assert_eq!(pair.host.world().resource::<Session>().watching, [1]);
    let watching = pair.guest.world().resource::<Watching>();
    assert!(watching.free && watching.slot.is_some());
    // The car watched is marked as this screen's, and is shown as the host has it.
    let marked: Vec<usize> = pair
        .guest
        .world_mut()
        .query_filtered::<&Kart, (With<Player>, With<Puppet>)>()
        .iter(pair.guest.world())
        .map(|kart| kart.slot)
        .collect();
    assert_eq!(marked, [watching_slot(&pair.guest)]);
    let (there, shown) = (
        car(&mut pair.host, HOSTS).pos,
        car(&mut pair.guest, HOSTS).pos,
    );
    assert!(
        there.distance(car(&mut pair.host, 0).pos) > 1.0,
        "the host's car should have moved off"
    );
    // Not driving, they see it as the host had it a moment ago, and no guess at now.
    assert!(
        shown.distance(there) < 8.0,
        "the watcher should see the host's car near where it is: {shown} against {there}"
    );
    // A car followed must move evenly from step to step, or the screen shakes.
    let mut steps = Vec::new();
    let mut was = shown;
    for _ in 0..120 {
        pair.step();
        let now = car(&mut pair.guest, HOSTS).pos;
        steps.push(now.distance(was));
        was = now;
    }
    // The car's laps are as the host has timed them, whenever the watching began.
    car_mut(&mut pair.host, HOSTS).lap = 1;
    for _ in 0..30 {
        pair.step();
    }
    let (timed, told) = (
        car(&mut pair.host, HOSTS).laps,
        car(&mut pair.guest, HOSTS).laps,
    );
    assert!(
        timed.lap == 1 && timed.began > 0.0,
        "the host should have timed the lap: {timed:?}"
    );
    assert_eq!(told, timed);
    let uneven = steps
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);
    println!(
        "steps of {:.3} to {:.3}, uneven by {uneven:.4}",
        steps.iter().copied().fold(f32::MAX, f32::min),
        steps.iter().copied().fold(0.0, f32::max)
    );
    assert!(
        uneven < 0.01,
        "the car watched should move evenly: {uneven}"
    );
}

fn watching_slot(app: &App) -> usize {
    app.world()
        .resource::<Watching>()
        .slot
        .expect("a car being watched")
}

/// A player who drops out and comes back under the same name has their points, and
/// their place in the race they left.
#[test]
fn a_player_who_comes_back_has_what_they_had() {
    let mut pair = Pair::new(0, 0.0);
    pair.guest.world_mut().resource_mut::<Session>().name = "Guest".into();
    for _ in 0..30 {
        pair.step();
    }
    let mut room = pair.host.world_mut().resource_mut::<Room>();
    room.voters
        .iter_mut()
        .find(|voter| voter.peer == 1)
        .expect("the player in the room")
        .points = 50;
    // The player's connection goes.
    pair.host.world_mut().resource_mut::<Inbox>().left.push(1);
    for _ in 0..10 {
        pair.step();
    }
    assert_eq!(
        pair.host.world().resource::<Session>().absent,
        [("Guest".to_string(), 50)]
    );
    // They dial again, and are someone new to the link.
    let mut back = game(Role::Client, pair.hub.join(), 0, 0);
    back.insert_state(Screen::Menu);
    *back.world_mut().resource_mut::<Session>() = Session {
        name: "Guest".into(),
        ..default()
    };
    for _ in 0..30 {
        pair.step();
        back.update();
    }
    let session = pair.host.world().resource::<Session>();
    assert!(session.absent.is_empty() && session.members.iter().any(|member| member.peer == 2));
    let voter = pair
        .host
        .world()
        .resource::<Room>()
        .voters
        .iter()
        .find(|voter| voter.peer == 2)
        .cloned();
    assert_eq!(voter.map(|voter| voter.points), Some(50));
    let lineup = pair.host.world().resource::<Lineup>();
    assert_eq!(
        lineup
            .seats
            .iter()
            .find(|seat| seat.name == "Guest")
            .and_then(|seat| seat.peer),
        Some(2)
    );
}

/// A racer someone built goes to the host and comes back in the lineup whole, and
/// nothing a host is sent of one is longer than the game would make it.
#[test]
fn a_built_racer_goes_over_the_wire_whole() {
    use crate::assets::lrs::{Cosmetics, Racer};
    let racer = Racer {
        name: "Brickbeard".into(),
        cosmetics: Cosmetics {
            hat: 4,
            face: 5,
            torso: 25,
            legs: 3,
            expression: 0,
        },
        chassis: "crchas0".into(),
        car: vec![0, 1, 0, 15, 0, 0, 0, 3, 0, 0],
        stock: false,
        trophies: 0,
    };
    let hello = ToHost::Hello {
        protocol: lobby_api::PROTOCOL,
        name: "P1".into(),
        password: String::new(),
        car: Ride::Built(racer.clone()),
    };
    assert_eq!(decode::<ToHost>(&encode(&hello)).unwrap(), hello);
    let seats = Lineup::seat(&[(1, "P1".to_string(), Ride::Built(racer.clone()))], 0);
    let lineup = Lineup { seats, you: 1 };
    assert_eq!(
        lineup.cast().collect::<Vec<_>>(),
        [(5, &Ride::Built(racer.clone()))]
    );
    let long = Racer {
        name: "N".repeat(40),
        chassis: "c".repeat(40),
        car: vec![0; 4000],
        ..racer
    };
    let Ride::Built(kept) = Ride::Built(long).checked() else {
        panic!("a built racer is still one once checked");
    };
    assert_eq!(
        (kept.name.len(), kept.chassis.len(), kept.car.len()),
        (14, 8, 514)
    );
}
