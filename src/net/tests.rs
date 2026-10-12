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
            token: 0,
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
    // No longer than the start straight lasts a car that isn't steered.
    for step in 0..270 {
        press(&mut pair.host, KeyCode::KeyW, true);
        pair.step();
        trail.push(car(&mut pair.host, HOSTS).pos);
        if step == 240 {
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
        .skip(240)
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
    // The player's car a little way up the road, the host's behind it, and no gate
    // near enough ahead of either for being knocked through it to change its place.
    let track = Track::new();
    car_mut(&mut pair.host, GUESTS).place(&track, 36.0, 0.0);
    car_mut(&mut pair.host, HOSTS).place(&track, 18.0, 0.0);
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

/// A game of tag is the host's to run: the player's game is told who is "it" and
/// what the clock has left, and the clock running out on one of two ends it for
/// everyone, the car that was "it" last.
#[test]
fn a_game_of_tag_is_told_to_the_player_and_ends_in_the_room() {
    let mut pair = Pair::new(0, 0.0);
    for app in [&mut pair.host, &mut pair.guest] {
        app.world_mut().resource_mut::<Settings>().mode = crate::menu::Mode::Tag;
    }
    for _ in 0..60 {
        pair.step();
    }
    let it = [HOSTS, GUESTS]
        .into_iter()
        .find(|&slot| car(&mut pair.host, slot).it)
        .expect("one of the two to be it");
    let other = HOSTS + GUESTS - it;
    for app in [&mut pair.host, &mut pair.guest] {
        assert!(car(app, it).it && !car(app, other).it);
        assert!(car(app, it).stake < modes::TAG_TIME);
        let clock = car(app, it).stake;
        assert_eq!(car(app, other).stake, clock);
        // And is driven the quicker on both.
        assert!(car(app, it).pace > car(app, other).pace);
    }
    // The clock all but run out.
    car_mut(&mut pair.host, it).stake = 0.2;
    for _ in 0..400 {
        pair.step();
    }
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    assert_eq!(
        (state(&pair.host), state(&pair.guest)),
        (Screen::Menu, Screen::Menu)
    );
    let names = ["Guest", "Host"];
    let (loser, winner) = if it == HOSTS { (names[1], names[0]) } else { (names[0], names[1]) };
    for app in [&pair.host, &pair.guest] {
        let told: Vec<&str> = app
            .world()
            .resource::<Room>()
            .results
            .iter()
            .map(|finish| &*finish.name)
            .collect();
        assert_eq!(told, [winner, loser]);
    }
}

/// In a battle the lives a car has left are the host's to count and everyone's to
/// see, and laps don't end it.
#[test]
fn a_battles_lives_are_told_to_the_player() {
    let mut pair = Pair::new(0, 0.0);
    for app in [&mut pair.host, &mut pair.guest] {
        app.world_mut().resource_mut::<Settings>().mode = crate::menu::Mode::Battle;
    }
    for _ in 0..30 {
        pair.step();
    }
    // The player's car is spun round, as a missile spins one, and has done every lap.
    car_mut(&mut pair.host, GUESTS).spin_round(1.0);
    car_mut(&mut pair.host, GUESTS).lap = 99;
    for _ in 0..30 {
        pair.step();
    }
    for app in [&mut pair.host, &mut pair.guest] {
        assert_eq!(car(app, GUESTS).stake, 2.0);
        assert_eq!(car(app, HOSTS).stake, 3.0);
        assert!(car(app, GUESTS).finished.is_none());
    }
    // Its last life gone, it is out, and the host's car, the only one left, has won.
    car_mut(&mut pair.host, GUESTS).stake = 1.0;
    for _ in 0..120 {
        pair.step();
    }
    car_mut(&mut pair.host, GUESTS).spin_round(1.0);
    for _ in 0..30 {
        pair.step();
    }
    for app in [&mut pair.host, &mut pair.guest] {
        assert!(car(app, GUESTS).out.is_some());
        assert_eq!((car(app, HOSTS).place, car(app, GUESTS).place), (1, 2));
    }
    assert!(car(&mut pair.host, HOSTS).finished.is_some());
}

/// A cannonball takes a life in a battle, as anything that throws a car does, and
/// one only, though its blast throws the car for as long as it is in reach.
#[test]
fn a_cannonball_takes_one_life_in_a_battle() {
    let mut pair = Pair::new(0, 0.0);
    for app in [&mut pair.host, &mut pair.guest] {
        app.world_mut().resource_mut::<Settings>().mode = crate::menu::Mode::Battle;
    }
    for _ in 0..30 {
        pair.step();
    }
    let track = Track::new();
    car_mut(&mut pair.host, GUESTS).place(&track, 30.0, 0.0);
    car_mut(&mut pair.host, HOSTS).place(&track, 12.0, 0.0);
    for _ in 0..30 {
        pair.step();
    }
    (
        car_mut(&mut pair.host, HOSTS).held,
        car_mut(&mut pair.host, HOSTS).whites,
    ) = (Some(Power::Red), 0);
    press(&mut pair.host, KeyCode::Space, true);
    for _ in 0..240 {
        pair.step();
        press(&mut pair.host, KeyCode::Space, false);
    }
    for app in [&mut pair.host, &mut pair.guest] {
        assert_eq!(car(app, GUESTS).stake, 2.0);
        assert_eq!(car(app, HOSTS).stake, 3.0);
    }
}

/// Elimination online counts every car, players' and the computer's alike, and the
/// host's own car going out is not the end of the race for the rest.
#[test]
fn elimination_goes_on_without_the_hosts_car() {
    let mut pair = Pair::new(1, 0.0);
    for app in [&mut pair.host, &mut pair.guest] {
        let mut settings = app.world_mut().resource_mut::<Settings>();
        (settings.mode, settings.rivals) = (crate::menu::Mode::Elimination, Some(2));
        // Three cars are two laps, whatever the host's own game is set to.
        settings.opponents = 0;
        assert!(settings.eliminating());
        assert_eq!(settings.laps(), 2);
    }
    // The computer's car drives off and the player's after it; the host's, which
    // hasn't moved, is last as the leader's first lap ends.
    for _ in 0..180 {
        press(&mut pair.guest, KeyCode::KeyW, true);
        pair.step();
    }
    press(&mut pair.guest, KeyCode::KeyW, false);
    car_mut(&mut pair.host, 0).lap = 2;
    for _ in 0..30 {
        pair.step();
    }
    assert!(car(&mut pair.host, HOSTS).out.is_some());
    assert!(car(&mut pair.host, GUESTS).out.is_none());
    // Long after, the race is still on for the two left in it.
    for _ in 0..800 {
        pair.step();
    }
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    assert_eq!(
        (state(&pair.host), state(&pair.guest)),
        (Screen::Race, Screen::Race)
    );
    assert!(car(&mut pair.host, HOSTS).finished.is_none());
    assert!(car(&mut pair.host, GUESTS).out.is_none());
    // The leader's second lap done, the other car goes too and the race is won.
    car_mut(&mut pair.host, 0).lap = 3;
    for _ in 0..30 {
        pair.step();
    }
    assert!(car(&mut pair.host, 0).finished.is_some());
    assert!(car(&mut pair.host, GUESTS).out.is_some());
    for _ in 0..400 {
        pair.step();
    }
    assert_eq!((state(&pair.host), state(&pair.guest)), (Screen::Menu, Screen::Menu));
    let room = pair.host.world().resource::<Room>();
    let told: Vec<&str> = room.results.iter().map(|finish| &*finish.name).collect();
    assert_eq!(told[1..], ["Guest", "Host"]);
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

/// The points a player has in the host's room, and who has their place on the grid.
fn held(host: &App, peer: Peer, name: &str) -> (Option<u32>, Option<Peer>) {
    let room = host.world().resource::<Room>();
    let voter = room.voters.iter().find(|voter| voter.peer == peer);
    let lineup = host.world().resource::<Lineup>();
    let seat = lineup.seats.iter().find(|seat| seat.name == name);
    (voter.map(|voter| voter.points), seat.and_then(|seat| seat.peer))
}

/// A pair whose guest has fifty points, and what the guest was given to come back by.
fn scored() -> (Pair, u64) {
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
    let token = pair.guest.world().resource::<Session>().token;
    assert!(token != 0, "the host should have given the player something to come back by");
    (pair, token)
}

/// Someone new to the link, who says they are called `name` and were given `token`.
fn comer(pair: &mut Pair, name: &str, token: u64) {
    let mut back = game(Role::Client, pair.hub.join(), 0, 0);
    back.insert_state(Screen::Menu);
    *back.world_mut().resource_mut::<Session>() = Session {
        name: name.into(),
        token,
        ..default()
    };
    for _ in 0..30 {
        pair.step();
        back.update();
    }
}

/// A player who drops out and comes back with what the host gave them has their
/// points, and their place in the race they left, whatever they are called now.
#[test]
fn a_player_who_comes_back_has_what_they_had() {
    let (mut pair, token) = scored();
    // The player's connection goes.
    pair.host.world_mut().resource_mut::<Inbox>().left.push(1);
    for _ in 0..10 {
        pair.step();
    }
    let absent = &pair.host.world().resource::<Session>().absent;
    assert_eq!(
        absent[..],
        [Absent {
            token,
            peer: 1,
            name: "Guest".into(),
            points: 50
        }]
    );
    // They dial again, and are someone new to the link.
    comer(&mut pair, "Guest again", token);
    let session = pair.host.world().resource::<Session>();
    assert!(session.absent.is_empty() && session.members.iter().any(|member| member.peer == 2));
    assert_eq!(held(&pair.host, 2, "Guest"), (Some(50), Some(2)));
}

/// Someone who takes the name of a player who has gone is not that player: they
/// have nothing of theirs, and it is kept for whoever it belongs to.
#[test]
fn a_name_is_not_who_someone_is() {
    let (mut pair, token) = scored();
    pair.host.world_mut().resource_mut::<Inbox>().left.push(1);
    for _ in 0..10 {
        pair.step();
    }
    for guess in [0, token ^ 2] {
        comer(&mut pair, "Guest", guess);
    }
    for peer in [2, 3] {
        assert_eq!(held(&pair.host, peer, "Guest"), (Some(0), Some(1)));
    }
    assert_eq!(pair.host.world().resource::<Session>().absent.len(), 1);
}

/// A player whose way to the host broke at their own end is back before the host
/// has missed them: they are let go of where they were, and are who they were.
#[test]
fn someone_back_before_they_were_missed_has_their_own_place() {
    let (mut pair, token) = scored();
    comer(&mut pair, "Guest", token);
    let session = pair.host.world().resource::<Session>();
    let here: Vec<Peer> = session.members.iter().map(|member| member.peer).collect();
    assert!(here == [2] && session.absent.is_empty(), "{here:?} are here");
    assert_eq!(held(&pair.host, 2, "Guest"), (Some(50), Some(2)));
    assert_eq!(held(&pair.host, 1, "Guest").0, None);
}

/// A player whose way to the host breaks in a race dials again without being
/// asked, is known for who they were, and goes back to the race they were in.
#[test]
fn a_player_whose_way_to_the_host_breaks_dials_again() {
    let (mut pair, _) = scored();
    let circuits = Circuits::find();
    pair.host.world_mut().resource_mut::<Session>().racing =
        Some(Rules::of(&Settings::new(&circuits), &circuits));
    let state = |app: &App| *app.world().resource::<State<Screen>>().get();
    let driven = |app: &mut App| {
        let mut remotes = app.world_mut().query::<&Remote>();
        remotes.iter(app.world()).map(|remote| remote.peer).collect::<Vec<Peer>>()
    };
    pair.hub.cut(1);
    for _ in 0..8 {
        pair.step();
    }
    // Until it is through again the player waits in the room, and is told why.
    assert!(pair.guest.world().resource::<Session>().retrying.is_some());
    assert_eq!(state(&pair.guest), Screen::Menu);
    for _ in 0..40 {
        pair.step();
    }
    let session = pair.guest.world().resource::<Session>();
    assert_eq!((session.you, session.retrying), (2, None));
    assert_eq!(*pair.guest.world().resource::<Role>(), Role::Client);
    assert_eq!(held(&pair.host, 2, "Guest"), (Some(50), Some(2)));
    // The host has sent the race again, and the player's game is loading it.
    assert_eq!(state(&pair.guest), Screen::Loading);
    assert_eq!(driven(&mut pair.host), Vec::<Peer>::new());
    pair.guest.insert_state(Screen::Race);
    pair.guest
        .world_mut()
        .run_system_cached(client::loaded)
        .unwrap();
    for _ in 0..30 {
        pair.step();
    }
    assert_eq!(driven(&mut pair.host), [2]);
}

/// A host that can't be got through to again is given up in the end.
#[test]
fn a_host_that_is_never_reached_again_is_given_up() {
    let (mut pair, _) = scored();
    pair.hub.cut(1);
    // Whoever dials from here on is cut off too: nothing gets through.
    for peer in 2..40 {
        pair.hub.cut(peer);
    }
    for _ in 0..(REDIAL_FOR * 60.0) as usize + 60 {
        pair.hub.step();
        pair.guest.update();
    }
    assert_eq!(*pair.guest.world().resource::<Role>(), Role::Offline);
    assert_eq!(
        pair.guest.world().resource::<Session>().notice.as_deref(),
        Some("The host has gone")
    );
}

/// A host that says nothing of a race for long enough is taken to be out of
/// reach, though nothing has said the way to it is broken.
#[test]
fn a_race_gone_silent_is_a_host_lost() {
    let (mut pair, _) = scored();
    for _ in 0..(SILENT * 60.0) as usize + 30 {
        pair.hub.step();
        pair.guest.update();
    }
    assert!(pair.guest.world().resource::<Session>().retrying.is_some());
}

/// A player whose pressing stops arriving is driven on as they last pressed for
/// a moment, then by the computer, and by themselves again once it arrives.
#[test]
fn a_car_nothing_is_heard_of_is_the_computers_until_something_is() {
    let mut pair = Pair::new(0, 0.0);
    for _ in 0..60 {
        pair.step();
    }
    let stalled = |app: &mut App| {
        let mut remotes = app.world_mut().query::<&Remote>();
        remotes.iter(app.world()).map(Remote::stalled).collect::<Vec<bool>>()
    };
    // Nothing pressed, the car has stayed on the grid.
    let grid = car(&mut pair.host, GUESTS).pos;
    assert_eq!(stalled(&mut pair.host), [false]);
    for _ in 0..240 {
        pair.hub.step();
        pair.host.update();
    }
    assert_eq!(stalled(&mut pair.host), [true]);
    let went = car(&mut pair.host, GUESTS).pos.distance(grid);
    assert!(went > 20.0, "the computer should have driven the car on, and drove it {went}");
    for _ in 0..30 {
        pair.step();
    }
    assert_eq!(stalled(&mut pair.host), [false]);
}

/// Everything a player is told of a full grid thirty times a second goes in one
/// datagram, with room to spare: one too big for the link is never sent at all.
#[test]
fn a_full_grid_is_told_in_one_datagram() {
    use super::protocol::{DATAGRAM, Snapshot, Stage};
    use super::state::{Sight, State};
    let mut pair = Pair::new(5, 0.0);
    pair.step();
    // Every car with all it can have to tell, in numbers as long as they come.
    let mut karts = pair.host.world_mut().query::<&mut Kart>();
    for mut k in karts.iter_mut(pair.host.world_mut()) {
        (k.finished, k.out, k.held, k.whites) = (Some(123.456), Some(99.9), Some(Power::Green), 3);
        (k.lap, k.progress, k.checkpoint) = (i32::MAX, 98765.43, Some(u32::MAX as usize));
        k.warp_to = Some((Vec3::ONE, Vec3::ONE, Vec3::ONE));
    }
    let karts: Vec<&Kart> = karts.iter(pair.host.world()).collect();
    assert_eq!(karts.len(), 6);
    let snapshot = Snapshot {
        tick: u32::MAX,
        used: u32::MAX,
        stage: Stage::Racing,
        clocks: [1.0, 2.0, 3.0],
        own: Some(State::of(karts[0])),
        karts: karts.iter().map(|k| Sight::of(k)).collect(),
    };
    let length = encode(&snapshot).len();
    assert!(
        length <= DATAGRAM * 3 / 4,
        "a snapshot of {length} bytes leaves too little of {DATAGRAM}"
    );
    println!("a snapshot of a full grid is {length} bytes");
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
        back: None,
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

#[test]
fn a_test_drive_in_a_session_is_driven_alone_and_ended_by_a_start() {
    use bevy::ecs::system::RunSystemOnce;
    let mut circuits = Circuits::find();
    let mut settings = Settings::new(&circuits);
    let (count, chosen) = (circuits.0.len(), settings.circuit);
    circuits.begin_test_drive(&mut settings);
    let mut world = World::new();
    world.insert_resource(Role::Client);
    world.insert_resource(Apart::default());
    world.insert_resource(State::new(Screen::Race));
    world.insert_resource(settings);
    // The menu left for the drive, the race is one alone and the session is kept.
    world.run_system_once(part).unwrap();
    assert_eq!(*world.resource::<Role>(), Role::Offline);
    assert_eq!(world.resource::<Apart>().role(Role::Offline), Role::Client);
    assert!(world.run_system_once(wired).unwrap() && !world.run_system_once(online).unwrap());
    // Its own loading leaves it so.
    world.run_system_once(rejoin).unwrap();
    assert_eq!(*world.resource::<Role>(), Role::Offline);
    // A map the host sends with a start comes after the test track, which is
    // the one to go, and the race loaded is the session's.
    let brick = circuits.0.iter().find(|c| c.key() == "BRICK").unwrap().map().text.clone();
    circuits.promise("THEIRS", "Their Circuit", "", false);
    assert!(circuits.end_test_drive(&mut world.resource_mut::<Settings>()));
    assert!(circuits.lend("THEIRS", &brick).is_ok());
    let keys: Vec<&str> = circuits.0.iter().map(|c| c.key()).collect();
    assert!(keys.len() == count + 1 && keys[count] == "THEIRS" && !keys.contains(&crate::menu::TEST_TRACK));
    let settings = world.resource::<Settings>();
    assert_eq!((settings.circuit, settings.test_drive), (chosen, None));
    world.run_system_once(rejoin).unwrap();
    assert_eq!(*world.resource::<Role>(), Role::Client);
    assert!(world.run_system_once(online).unwrap());
}

/// A host alone in an arena with five of the computer's cars, its own car the
/// computer's to drive too, as a demo's is.
fn fought(map: &crate::map::Map, mode: crate::menu::Mode, seed: u32) -> App {
    let mut app = App::new();
    let circuits = Circuits::find();
    let mut settings = Settings::new(&circuits);
    // With lives enough that a battle is not over before it has been looked at.
    (settings.mode, settings.lives) = (mode, crate::menu::MAX_LIVES);
    let players = [(HOST, "Host".to_string(), Ride::Slot)];
    app.add_plugins((MinimalPlugins, StatesPlugin, plugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::from_hz(TICKS).timestep(),
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
        .insert_resource(Track::made(map, false))
        .insert_resource(crate::meshgen::Rng(seed))
        .insert_resource(Race {
            phase: Phase::Intro,
            intro: 0.0,
            countdown: 0.0,
            time: 0.0,
            demo: true,
            quick: true,
        })
        .insert_resource(circuits)
        .insert_resource(settings)
        .insert_resource(Role::Host)
        .insert_resource(Wire(Box::new(Hub::new(0, 0.0).host())))
        .insert_resource(Session {
            name: "Me".into(),
            you: HOST,
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
            seats: Lineup::seat(&players, 5),
            you: HOST,
        })
        .add_systems(Startup, (kart::spawn_karts, items::setup_items).chain());
    app.register_required_components::<items::Pickup, Visibility>();
    app
}

/// What came of a game among the computer's cars.
#[derive(Debug, Default)]
struct Played {
    /// How long it went on, and whether it was over by then.
    time: f32,
    over: bool,
    /// Lives taken, in a battle.
    hits: u32,
    /// Times "it" was passed on by a bump, in a game of tag, and when each car
    /// went out.
    passes: u32,
    outs: Vec<f32>,
    /// The share of the cars' time spent on high ground, and going nowhere.
    high: f32,
    slow: f32,
    /// The share of the open floor some car went over.
    covered: f32,
    /// How far each car was from the nearest other, on the whole.
    apart: f32,
    /// Where the cars went nowhere, a place ten across and how many steps, the
    /// worst first.
    stops: Vec<((i32, i32, i32), u32)>,
}

fn played(map: &crate::map::Map, mode: crate::menu::Mode, seed: u32, seconds: f32) -> Played {
    let plan = map.plan.as_ref().expect("an arena");
    let mut app = fought(map, mode, seed);
    let reach = plan.reach();
    const CELL: f32 = 10.0;
    let side = (2.0 * reach / CELL).ceil() as usize + 1;
    let cell = |p: Vec3| {
        let (x, z) = (((p.x + reach) / CELL) as usize, ((p.z + reach) / CELL) as usize);
        (x.min(side - 1), z.min(side - 1))
    };
    let mut open = vec![false; side * side];
    for x in 0..side {
        for z in 0..side {
            let middle = Vec3::new((x as f32 + 0.5) * CELL - reach, 0.0, (z as f32 + 0.5) * CELL - reach);
            open[x * side + z] = plan.open(middle, 2.0);
        }
    }
    let mut been = vec![false; side * side];
    let mut out = Played::default();
    let (mut car_steps, mut high, mut slow, mut apart) = (0u32, 0u32, 0u32, 0.0f64);
    let mut it: Option<usize> = None;
    let mut lives: Option<f32> = None;
    let steps = (seconds * TICKS as f32) as usize;
    for _ in 0..steps {
        app.update();
        if *app.world().resource::<State<Screen>>().get() != Screen::Race {
            out.over = true;
            break;
        }
        let race = app.world().resource::<Race>();
        if !matches!(race.phase, Phase::Racing | Phase::Finished) {
            continue;
        }
        out.time = race.time;
        let now = race.time;
        let world = app.world_mut();
        let cars: Vec<(usize, Vec3, f32, bool, bool, f32)> = world
            .query::<&Kart>()
            .iter(world)
            .map(|k| (k.slot, k.pos, k.vel.length(), k.it, k.out.is_some() || k.finished.is_some(), k.stake))
            .collect();
        let playing: Vec<_> = cars.iter().filter(|car| !car.4).collect();
        if playing.len() < 2 {
            out.over = true;
            break;
        }
        for car in &playing {
            car_steps += 1;
            high += (car.1.y > 2.0) as u32;
            slow += (car.2 < 5.0) as u32;
            if car.2 < 5.0 {
                let place = ((car.1.x / 10.0).round() as i32 * 10, (car.1.y / 3.0).round() as i32 * 3, (car.1.z / 10.0).round() as i32 * 10);
                match out.stops.iter_mut().find(|stop| stop.0 == place) {
                    Some(stop) => stop.1 += 1,
                    None => out.stops.push((place, 1)),
                }
            }
            let (x, z) = cell(car.1);
            been[x * side + z] = true;
            let nearest = playing
                .iter()
                .filter(|other| other.0 != car.0)
                .map(|other| other.1.distance(car.1))
                .fold(f32::MAX, f32::min);
            apart += nearest as f64;
        }
        match mode {
            crate::menu::Mode::Battle => {
                let left: f32 = cars.iter().map(|car| car.5).sum();
                if let Some(before) = lives {
                    out.hits += (before - left).max(0.0) as u32;
                }
                lives = Some(left);
            }
            _ => {
                let now_it = cars.iter().find(|car| car.3).map(|car| car.0);
                if let (Some(was), Some(is)) = (it, now_it)
                    && was != is
                {
                    let gone = cars.iter().any(|car| car.0 == was && car.4);
                    if gone {
                        out.outs.push(now);
                    } else {
                        out.passes += 1;
                    }
                }
                it = now_it.or(it);
            }
        }
    }
    let floor = open.iter().filter(|open| **open).count().max(1);
    let trodden = open.iter().zip(&been).filter(|(open, been)| **open && **been).count();
    out.covered = trodden as f32 / floor as f32;
    let car_steps = car_steps.max(1) as f32;
    (out.high, out.slow, out.apart) =
        (high as f32 / car_steps, slow as f32 / car_steps, (apart / car_steps as f64) as f32);
    out
}

/// What an arena is laid out for, as far as the computer's cars can show it in a
/// minute of a battle and a minute of a game of tag among themselves: they are on
/// its high ground a fair part of the time and over a fair part of its floor, they
/// are seldom stopped, lives are taken and "it" is passed on.
fn fought_over(key: &str) {
    let map = crate::map::named(key);
    for mode in [crate::menu::Mode::Battle, crate::menu::Mode::Tag] {
        let game = played(map, mode, 7, 60.0);
        assert!(game.time > 30.0, "{key} {mode:?} was over in {}s", game.time);
        assert!(game.high > 0.08, "{key} {mode:?}: {} of the time on high ground", game.high);
        assert!(game.covered > 0.25, "{key} {mode:?}: over {} of the floor", game.covered);
        assert!(game.slow < 0.15, "{key} {mode:?}: stopped {} of the time", game.slow);
        let done = game.hits.max(game.passes) as f32 * 60.0 / game.time;
        assert!(done >= 2.0, "{key} {mode:?}: {done} a minute");
    }
}

#[test]
fn the_yard_is_fought_all_over() {
    fought_over("YARD");
}

#[test]
fn the_crossroads_is_fought_all_over() {
    fought_over("CROSSROADS");
}

#[test]
fn the_ring_is_fought_all_over() {
    fought_over("RING");
}

#[test]
fn the_town_is_fought_all_over() {
    fought_over("TOWN");
}

/// What the arenas are like to fight in, as the computer's cars find them over
/// several whole games: not a test of anything, but something to read while one is
/// laid out. `BRICK_OLD` is a folder of other files of the same names to measure
/// beside them.
#[test]
#[ignore]
fn the_arenas_as_the_computers_cars_find_them() {
    let old = std::env::var("BRICK_OLD").ok();
    for map in crate::map::arenas() {
        let was = old.as_ref().and_then(|folder| {
            let file = format!("{folder}/{}.toml", map.key.to_lowercase());
            crate::map::Map::read(&map.key, &std::fs::read_to_string(file).ok()?).ok()
        });
        for (which, map) in [("old", was.as_ref()), ("new", Some(map))] {
            let Some(map) = map else { continue };
            for mode in [crate::menu::Mode::Battle, crate::menu::Mode::Tag] {
                let seeds = [7, 11, 23, 31, 43, 57];
                let games: Vec<Played> = seeds.iter().map(|&seed| played(map, mode, seed, 240.0)).collect();
                let count = games.len() as f32;
                let mean = |of: &dyn Fn(&Played) -> f32| games.iter().map(of).sum::<f32>() / count;
                let minutes = games.iter().map(|p| p.time).sum::<f32>() / 60.0;
                let mut stops: Vec<((i32, i32, i32), u32)> = Vec::new();
                for stop in games.iter().flat_map(|p| &p.stops) {
                    match stops.iter_mut().find(|known| known.0 == stop.0) {
                        Some(known) => known.1 += stop.1,
                        None => stops.push(*stop),
                    }
                }
                stops.sort_by_key(|stop| std::cmp::Reverse(stop.1));
                stops.truncate(6);
                println!(
                    "{:10} {which} {:6?}: {:4.0}s a game, {:4.1} hits and {:4.1} passes a minute, high {:.2} slow {:.2} covered {:.2} apart {:.0}, stopped most at {stops:?}",
                    map.key,
                    mode,
                    mean(&|p| p.time),
                    games.iter().map(|p| p.hits).sum::<u32>() as f32 / minutes,
                    games.iter().map(|p| p.passes).sum::<u32>() as f32 / minutes,
                    mean(&|p| p.high),
                    mean(&|p| p.slow),
                    mean(&|p| p.covered),
                    mean(&|p| p.apart),
                );
            }
        }
    }
}

/// What came of a game that nobody goes out of, played among the computer's cars
/// for `seconds` or until it was won: how long it went on, how many cars were ever
/// "it", how often that changed, and the most any car had of what it counts.
fn tallied(key: &str, mode: crate::menu::Mode, seconds: f32) -> (f32, usize, u32, f32) {
    let mut app = fought(crate::map::named(key), mode, 7);
    let (mut time, mut ever, mut changes, mut most) = (0.0, [false; 6], 0, 0.0f32);
    let mut was: Vec<usize> = Vec::new();
    for _ in 0..(seconds * TICKS as f32) as usize {
        app.update();
        if *app.world().resource::<State<Screen>>().get() != Screen::Race {
            break;
        }
        let race = app.world().resource::<Race>();
        if !matches!(race.phase, Phase::Racing | Phase::Finished) {
            continue;
        }
        time = race.time;
        let world = app.world_mut();
        let cars: Vec<&Kart> = world.query::<&Kart>().iter(world).collect();
        if cars.iter().any(|k| k.finished.is_some()) {
            break;
        }
        let mut its: Vec<usize> = cars.iter().filter(|k| k.it).map(|k| k.slot).collect();
        its.sort();
        for &slot in &its {
            ever[slot] = true;
        }
        changes += (its != was) as u32;
        was = its;
        most = cars.iter().map(|k| k.stake).fold(most, f32::max);
    }
    (time, ever.iter().filter(|ever| **ever).count(), changes, most)
}

/// The three games that come of tag are games among the computer's cars in every
/// arena: in infection cars are caught, in keep-away the flag changes hands and is
/// held, and in king of the hill the hill is found and held.
#[test]
fn the_games_nobody_goes_out_of_are_played_in_every_arena() {
    use crate::menu::Mode;
    for key in ["YARD", "CROSSROADS", "RING", "TOWN"] {
        let (time, caught, _, _) = tallied(key, Mode::Infection, 90.0);
        println!("{key} infection: {caught} caught in {time:.0}s");
        assert!(caught >= 3, "{key}: {caught} were ever \"it\" in {time}s of infection");
        let (time, held, changes, most) = tallied(key, Mode::KeepAway, 90.0);
        println!("{key} keep-away: {held} held it, {changes} changes, most {most:.0}s in {time:.0}s");
        assert!(held >= 2 && most > 5.0, "{key}: {held} held the flag, for {most}s at the most");
        let (time, kings, changes, most) = tallied(key, Mode::Hill, 90.0);
        println!("{key} hill: {kings} kings, {changes} changes, most {most:.0}s in {time:.0}s");
        assert!(kings >= 1 && most > 3.0, "{key}: {kings} were king, for {most}s at the most");
    }
}
