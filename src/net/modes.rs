//! The ways of racing online that laps don't end: a battle, a game of tag, and three
//! that come of tag, infection, keep-away and king of the hill. The port's own, and
//! the host's to choose (`menu::Mode`, in `protocol::Rules`).
//!
//! In a battle every car has a few lives. Anything that spins a car round or
//! blows it into the air takes one (`Kart::hits`), and for a moment after nothing
//! takes another: a blast throws a car for as long as it is in reach, and that is
//! one hit. A car with none left is out of the race; the last car in it wins, or
//! when its time is up whoever has most left. How many lives, how long a car is
//! spared and how long the battle may go on are the host's to say
//! (`Settings::lives`, `spared`, `battle_time`).
//!
//! In a game of tag one car, drawn at the start, is "it", and a clock runs down
//! whoever is: `TAG_TIME` seconds for the last two cars, and `TAG_MORE` more for
//! each car besides that is in the round (`round`). The car that is "it" stops being so by bumping
//! another, which is "it" from then on and can't hand it straight back for a
//! moment; the clock is not put back, but a car caught has a few seconds of it at
//! the least (the host's to say: `Settings::tag_back`, `tag_least`). The car that is "it" when the
//! clock runs out is out of the race, the clock is put back and another of those
//! left is drawn. The car that is "it" is `IT_PACE` times as fast as the rest, so
//! that it can catch them, the computer's on their recorded lines too. The last
//! car in it wins, and the rest come in the order they went out, last out first.
//! So that the car to keep away from can be found, it glows red, through whatever
//! stands between (`glow`), and the display says where it is while it is out of
//! the picture (`hud_original`).
//!
//! In infection a car that is "it" stays so, and so does every car one of them
//! bumps: the drawn car has the whole field to catch, and is no quicker than the
//! rest, so that it catches by cornering. A car's `stake` is how long it has gone uncaught, and the
//! game is over when one car is left that hasn't been, or its time is up; the
//! cars come in the order of how long they lasted.
//!
//! In keep-away one car, drawn at the start, has the flag (it is "it"), and has
//! it until another bumps it, which has it then and can't lose it straight back
//! for a moment (`Settings::tag_back`). A car's `stake` is how long it has held
//! the flag in all; the first to have held it as long as the host says
//! (`Settings::goal`) wins, or whoever has held it longest when the time is up.
//!
//! In king of the hill the hill is the highest of the arena's line
//! (`Track::hill`), marked by a beacon (`beacon`). A car on it alone is its king
//! (it is "it"), and its `stake` is how long it has been; with two cars on it
//! nobody is. It is won as keep-away is.
//!
//! Each has something more to it, said where it is run: cars that come back to a
//! battle, hits that run the clock of tag down, rounds of infection, a flag
//! knocked loose, a hill that moves. All but infection may be played by two teams
//! (`team`), and what happens in any of them is said on the display as it does
//! (`Feed`).
//!
//! In all three nobody goes out. All are run on the host, as the rest of the race
//! is, and what each car has (`Kart::stake`, `Kart::it`) goes to the players with
//! its `state::Standing`.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderDefVal;

use crate::kart::{Bumps, Kart};
use crate::menu::{Mode, Settings};
use crate::meshgen::Rng;
use crate::rules::put_out;
use crate::{Phase, Race};

/// The seconds on the clock of a game of tag when a car is made "it" by the draw
/// with two cars left in it, and how many more there are for each car besides.
pub const TAG_TIME: f32 = 30.0;
pub const TAG_MORE: f32 = 5.0;
/// How many times as fast as the rest the car that is "it" is.
pub const IT_PACE: f32 = 1.15;
/// Cars that end a game together are a thousandth of a second apart for each of
/// what they had left, so that the places they are given are in that order.
const APART: f32 = 1e-3;

/// How the game in hand is getting on, as its host has it.
#[derive(Resource, Default)]
pub struct Play {
    /// Every car has been given what it begins with.
    begun: bool,
    /// How many hits each car, by grid slot, had taken at the last step, and what
    /// is left of the time it is spared in: another life in a battle, catching
    /// anyone in infection, being king in king of the hill.
    hits: [u32; SLOTS],
    spared: [f32; SLOTS],
    /// What is left of the time "it" can't be passed on in.
    held: f32,
    /// The rounds of infection played, and the cars drawn to begin them.
    rounds: u8,
    drawn: Vec<usize>,
    /// Where the flag of keep-away lies while nobody has it, and for how long.
    loose: Option<(Vec3, f32)>,
}

const SLOTS: usize = crate::kart::PLAYER_SLOT + 1;

/// Where the thing a game is played for lies while it is nobody's: the flag of
/// keep-away. The host's to say, and told to the players with the cars.
#[derive(Resource, Default)]
pub struct Mark(pub Option<Vec3>);

/// Whether the cars are being raced: the race goes on for the rest after the car
/// driven on the host's screen is done.
fn on(race: &Race) -> bool {
    matches!(race.phase, Phase::Racing | Phase::Finished)
}

/// The team a car is of, where the game is played by two: whichever the host
/// seated it in (`Settings::sides`, from `Lineup::seat`), which is the one its
/// player picked in the room if they picked. Infection makes its own sides.
pub fn team(settings: &Settings, slot: usize) -> Option<usize> {
    let sided = settings.teams && settings.lapless() && settings.mode != Mode::Infection;
    sided.then_some((settings.sides >> slot) as usize & 1)
}

/// Whether a game's coloured bricks are each a colour for the car coming to it
/// (`items`): those of the tag family are, a battle's being as its arena lays them.
pub fn suits(mode: Mode) -> bool {
    matches!(mode, Mode::Tag | Mode::Infection | Mode::KeepAway | Mode::Hill)
}

/// Whether a car in a game of the tag family is after another, and wants bricks
/// to catch it with, or is being got away from, and wants them to keep off with:
/// in tag and infection the car that is "it" is after the rest, and in keep-away
/// and king of the hill the rest are after it, and not its own team. `it` is the
/// grid slots of the cars that are. Nothing in any other game.
pub fn chasing(settings: &Settings, kart: &Kart, it: &[usize]) -> Option<bool> {
    let with = |&slot: &usize| slot == kart.slot || allies(settings, slot, kart.slot);
    match settings.mode {
        Mode::Tag | Mode::Infection => Some(kart.it),
        Mode::KeepAway | Mode::Hill => Some(!it.iter().any(with)),
        Mode::Race | Mode::Elimination | Mode::Battle => None,
    }
}

fn allies(settings: &Settings, a: usize, b: usize) -> bool {
    team(settings, a).is_some() && team(settings, a) == team(settings, b)
}

/// What a car has of what its game counts, more being better: a knock-out is
/// worse than any lives lost.
pub fn worth(kart: &Kart) -> f32 {
    kart.stake - DOWN_COST * kart.downs as f32
}

/// What a knock-out costs beside a life, and how much more a team's standing
/// counts than a car's own when the game is over.
const DOWN_COST: f32 = 10.0;
const TEAM_WEIGHT: f32 = 10.0;

/// What each team has between its cars still in the game.
pub fn totals<'a>(settings: &Settings, karts: impl Iterator<Item = &'a Kart>) -> [f32; 2] {
    let mut totals = [0.0; 2];
    for kart in karts.filter(|kart| playing(kart)) {
        if let Some(team) = team(settings, kart.slot) {
            totals[team] += worth(kart);
        }
    }
    totals
}

/// The cars still in it are home, in the order of what they have: by teams, the
/// team that has more first.
fn end(karts: &mut Query<&mut Kart>, settings: &Settings, time: f32) {
    let totals = totals(settings, karts.iter());
    for mut kart in karts.iter_mut().filter(|kart| playing(kart)) {
        let side = team(settings, kart.slot).map_or(0.0, |team| totals[team] * TEAM_WEIGHT);
        (kart.finished, kart.it) = (Some(time - (side + worth(&kart)) * APART), false);
    }
}

/// Whether a car has been spun round or thrown since it was last asked of.
fn struck(play: &mut Play, kart: &Kart) -> bool {
    let seen = play.hits.get_mut(kart.slot);
    seen.is_some_and(|seen| std::mem::replace(seen, kart.hits) != kart.hits)
}

/// The car of whoever leads the session's series, if someone does alone: the
/// games give it the worst of the draw.
fn leader(room: Option<&super::room::Room>, lineup: Option<&super::Lineup>) -> Option<usize> {
    let (room, lineup) = (room?, lineup?);
    let most = room.voters.iter().map(|voter| voter.points).max().filter(|most| *most > 0)?;
    let mut leading = room.voters.iter().filter(|voter| voter.points == most);
    let (first, None) = (leading.next()?, leading.next()) else {
        return None;
    };
    let seat = lineup.seats.iter().find(|seat| seat.peer == Some(first.peer))?;
    Some(seat.slot as usize)
}

/// How long a car knocked out of a battle that cars come back to is held where it
/// is, and how long such a battle goes on if its host sets it no limit.
const DOWN: f32 = 4.0;
const RETURNS_TIME: f32 = 180.0;

/// A battle: a car hit loses a life, and one with none left is out; or, where the
/// host has cars come back, is held a moment, has its lives again and is one
/// knock-out the worse.
pub fn battle(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    mut play: ResMut<Play>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::Battle {
        return;
    }
    if !play.begun {
        play.begun = true;
        for mut kart in &mut karts {
            (kart.stake, kart.downs) = (settings.lives as f32, 0);
        }
    }
    if !on(&race) {
        return;
    }
    let dt = time.delta_secs();
    let play = &mut *play;
    for mut kart in &mut karts {
        let hit = struck(play, &kart);
        let Some(spared) = play.spared.get_mut(kart.slot) else {
            continue;
        };
        *spared = (*spared - dt).max(0.0);
        if !hit || *spared > 0.0 || !playing(&kart) {
            continue;
        }
        *spared = settings.spared();
        kart.stake = (kart.stake - 1.0).max(0.0);
        debug!("{} has {} lives left", kart.name, kart.stake);
        if kart.stake > 0.0 {
            continue;
        }
        if settings.returns {
            kart.downs = kart.downs.saturating_add(1);
            (kart.stake, kart.magnet) = (settings.lives as f32, DOWN);
            *spared += DOWN;
        } else {
            put_out(&mut kart, race.time);
        }
    }
    let left: Vec<usize> = karts.iter().filter(|kart| playing(kart)).map(|kart| kart.slot).collect();
    // By teams it is over when one of them has nobody left.
    let one_side = left.iter().all(|slot| allies(&settings, *slot, left[0]));
    let limit = match settings.battle_time() {
        limit if settings.returns => limit.min(RETURNS_TIME),
        limit => limit,
    };
    let fought_out = !settings.returns && (left.len() == 1 || (left.len() > 1 && one_side));
    if fought_out || (!left.is_empty() && race.time >= limit) {
        end(&mut karts, &settings, race.time);
    }
}

/// What the clock of a game of tag begins a round at, with `cars` in it.
pub fn round(cars: usize) -> f32 {
    TAG_TIME + TAG_MORE * cars.saturating_sub(2) as f32
}

/// Whether a car is still in the game.
fn playing(kart: &Kart) -> bool {
    kart.out.is_none() && kart.finished.is_none()
}

/// One of the cars still in the game is made "it": `first` if that is one of
/// them, and otherwise any but those of `spared`, unless they are all there are.
fn draw(karts: &mut Query<&mut Kart>, dice: &mut Rng, first: Option<usize>, spared: &[usize]) -> Option<usize> {
    let mut slots: Vec<usize> = karts.iter().filter(|kart| playing(kart)).map(|kart| kart.slot).collect();
    slots.sort();
    let free: Vec<usize> = slots.iter().copied().filter(|slot| !spared.contains(slot)).collect();
    let among = if free.is_empty() { &slots } else { &free };
    let drawn = first
        .filter(|first| slots.contains(first))
        .or_else(|| among.get((dice.f() * among.len() as f32) as usize % among.len().max(1)).copied())?;
    for mut kart in karts.iter_mut().filter(|kart| kart.slot == drawn) {
        kart.it = true;
    }
    Some(drawn)
}

/// How much comes off the clock of a game of tag when the car that is "it" is
/// spun round or thrown by something fired at it.
pub const TAG_HIT: f32 = 5.0;

/// A game of tag: "it" goes to whichever car the one that is "it" bumps, the car
/// that is when the clock runs out is out, and the last car left wins. The cars
/// that aren't have their say too: what they fire at the one that is, and hit it
/// with, takes time off its clock.
pub fn tag(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    bumps: Res<Bumps>,
    (room, lineup): (Option<Res<super::room::Room>>, Option<Res<super::Lineup>>),
    mut play: ResMut<Play>,
    mut dice: ResMut<Rng>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::Tag {
        return;
    }
    if !play.begun {
        play.begun = true;
        let clock = round(karts.iter().count());
        for mut kart in &mut karts {
            (kart.stake, kart.it) = (clock, false);
        }
        // Whoever leads the series is "it" to begin with.
        let first = leader(room.as_deref(), lineup.as_deref());
        draw(&mut karts, &mut dice, first, &[]);
    }
    // Over once one car is left.
    if !on(&race) || karts.iter().filter(|kart| playing(kart)).count() < 2 {
        return;
    }
    let dt = time.delta_secs();
    play.held = (play.held - dt).max(0.0);
    let mut hit = false;
    for kart in &karts {
        hit |= struck(&mut play, kart) && kart.it;
    }
    let it = karts.iter().find(|kart| kart.it).map(|kart| kart.slot);
    let caught = it.and_then(|it| {
        bumps.0.iter().find_map(|&(a, b)| match (a == it, b == it) {
            (true, false) => Some(b),
            (false, true) => Some(a),
            _ => None,
        })
    });
    let caught = caught.filter(|&caught| {
        karts
            .iter()
            .any(|kart| kart.slot == caught && playing(kart))
    });
    // By teams a car is caught by one of the other team's.
    let caught = caught.filter(|&caught| !it.is_some_and(|it| allies(&settings, it, caught)));
    let caught = caught.filter(|_| play.held <= 0.0);
    if let Some(caught) = caught {
        play.held = settings.tag_back();
        for mut kart in &mut karts {
            kart.it = kart.slot == caught;
            // The driver caught starts at it.
            kart.cues.hit |= kart.it;
        }
    }
    // The one clock is every car's `stake`, and the car that is "it" has the say.
    let clock = karts
        .iter()
        .find(|kart| kart.it)
        .map_or(TAG_TIME, |kart| kart.stake);
    // A car caught with the clock all but out has a moment to pass it on in.
    let least = if caught.is_some() { settings.tag_least() } else { 0.0 };
    let lost = dt + if hit && caught.is_none() { TAG_HIT } else { 0.0 };
    let mut clock = (clock - lost).max(least);
    let ran_out = clock <= 0.0;
    if ran_out {
        for mut kart in karts.iter_mut().filter(|kart| kart.it) {
            debug!("{} is out at {:.1}s", kart.name, race.time);
            (kart.it, kart.stake) = (false, 0.0);
            put_out(&mut kart, race.time);
        }
        let left = karts.iter().filter(|kart| playing(kart)).count();
        (clock, play.held) = (round(left), 0.0);
    }
    for mut kart in karts.iter_mut().filter(|kart| playing(kart)) {
        kart.stake = clock;
    }
    if !ran_out {
        return;
    }
    if karts.iter().filter(|kart| playing(kart)).count() < 2 {
        end(&mut karts, &settings, race.time);
    } else {
        draw(&mut karts, &mut dice, None, &[]);
    }
}

/// The cars that bumped a car that is "it" this step, and are still in the game.
fn bumped(karts: &Query<&mut Kart>, bumps: &Bumps) -> Vec<(usize, usize)> {
    let is = |slot: usize, it: bool| {
        karts
            .iter()
            .any(|kart| kart.slot == slot && kart.it == it && playing(kart))
    };
    let met = bumps.0.iter().flat_map(|&(a, b)| [(a, b), (b, a)]);
    met.filter(|&(it, other)| is(it, true) && is(other, false)).collect()
}

/// How long the cars of a round of infection have to get away from the one drawn,
/// which catches nobody until then: they start side by side. How long a car just
/// caught whirls and can catch nobody itself, how many seconds a catch is worth
/// to the car that made it, and how many rounds a game is of.
const HEAD_START: f32 = 3.0;
const STUNNED: f32 = 2.0;
pub const CATCH: f32 = 5.0;
pub const ROUNDS: u8 = 3;

/// Infection: every car a car that is "it" bumps is "it" from then on, and a
/// round is over when one car is left that isn't. A game is of a few rounds, each
/// begun by a car that hasn't begun one, and what a car has at the end of them
/// is how long it went uncaught in all and what its catches were worth.
pub fn infection(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    bumps: Res<Bumps>,
    (room, lineup): (Option<Res<super::room::Room>>, Option<Res<super::Lineup>>),
    mut play: ResMut<Play>,
    mut dice: ResMut<Rng>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::Infection {
        return;
    }
    let play = &mut *play;
    if !play.begun {
        play.begun = true;
        for mut kart in &mut karts {
            (kart.stake, kart.it) = (0.0, false);
        }
        // Whoever leads the series begins the first round.
        let first = leader(room.as_deref(), lineup.as_deref());
        play.drawn.extend(draw(&mut karts, &mut dice, first, &[]));
        play.held = HEAD_START;
    }
    if !on(&race) || !karts.iter().any(|kart| playing(kart)) {
        return;
    }
    let dt = time.delta_secs();
    play.held = (play.held - dt).max(0.0);
    for spared in &mut play.spared {
        *spared = (*spared - dt).max(0.0);
    }
    let able = |by: usize| play.held <= 0.0 && play.spared.get(by).is_none_or(|left| *left <= 0.0);
    let mut caught: Vec<(usize, usize)> = bumped(&karts, &bumps).into_iter().filter(|met| able(met.0)).collect();
    caught.sort_by_key(|met| met.1);
    caught.dedup_by_key(|met| met.1);
    for mut kart in &mut karts {
        if caught.iter().any(|met| met.1 == kart.slot) {
            debug!("{} is caught at {:.1}s", kart.name, race.time);
            // The driver caught starts at it, and whirls where it is.
            (kart.it, kart.cues.hit) = (true, true);
            kart.spin_round(STUNNED * crate::physics::SPIN_RATE / std::f32::consts::TAU);
            if let Some(spared) = play.spared.get_mut(kart.slot) {
                *spared = STUNNED;
            }
        }
        kart.stake += CATCH * caught.iter().filter(|met| met.0 == kart.slot).count() as f32;
        if playing(&kart) && !kart.it {
            kart.stake += dt;
        }
    }
    let cars = karts.iter().filter(|kart| playing(kart)).count();
    let free = karts.iter().filter(|kart| playing(kart) && !kart.it).count();
    let timed_out = race.time >= settings.battle_time();
    if free > 1 && !timed_out {
        return;
    }
    play.rounds += 1;
    if timed_out || play.rounds >= ROUNDS || cars < 3 {
        end(&mut karts, &settings, race.time);
        return;
    }
    for mut kart in &mut karts {
        kart.it = false;
    }
    let drawn = play.drawn.clone();
    play.drawn.extend(draw(&mut karts, &mut dice, None, &drawn));
    play.held = HEAD_START;
}

/// The game is whoever's has most of what it counts, or whichever team's, once
/// one has all it takes or its time is up.
fn scored(karts: &mut Query<&mut Kart>, settings: &Settings, time: f32) {
    let sides = totals(settings, karts.iter());
    let most = karts
        .iter()
        .filter(|kart| playing(kart))
        .map(|kart| team(settings, kart.slot).map_or(kart.stake, |team| sides[team]))
        .fold(0.0, f32::max);
    if most >= settings.goal() || time >= settings.battle_time() {
        end(karts, settings, time);
    }
}

/// How near a car must come to a flag lying loose to have it, and how long it
/// lies before the nearest car has it anyway.
pub const FLAG_REACH: f32 = 4.0;
const LOOSE_FOR: f32 = 8.0;

/// Keep-away: the car with the flag has it until another bumps it, and whoever
/// has held it longest wins. A car that has just got it can't lose it at once,
/// and has a shield for as long; after that, something fired at it that spins it
/// round or throws it knocks the flag out of its hands, to lie where it fell
/// for whoever is first to it.
pub fn keep_away(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    bumps: Res<Bumps>,
    (room, lineup): (Option<Res<super::room::Room>>, Option<Res<super::Lineup>>),
    mut play: ResMut<Play>,
    mut mark: ResMut<Mark>,
    mut dice: ResMut<Rng>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::KeepAway {
        return;
    }
    let play = &mut *play;
    if !play.begun {
        play.begun = true;
        for mut kart in &mut karts {
            (kart.stake, kart.it) = (0.0, false);
        }
        // Whoever leads the series is not the one to begin with it.
        let leads: Vec<usize> = leader(room.as_deref(), lineup.as_deref()).into_iter().collect();
        draw(&mut karts, &mut dice, None, &leads);
    }
    if !on(&race) || !karts.iter().any(|kart| playing(kart)) {
        return;
    }
    let dt = time.delta_secs();
    play.held = (play.held - dt).max(0.0);
    // Whose it is now, if anyone's is to change.
    let mut taker = None;
    let mut dropped = None;
    for kart in &karts {
        if struck(play, kart) && kart.it && play.held <= 0.0 {
            dropped = Some(kart.pos);
        }
    }
    if let Some(at) = dropped {
        play.loose = Some((at, 0.0));
        for mut kart in karts.iter_mut().filter(|kart| kart.it) {
            (kart.it, kart.cues.hit) = (false, true);
        }
    } else if let Some((at, lain)) = &mut play.loose {
        *lain += dt;
        let off = |kart: &Kart| (kart.pos - *at).with_y(0.0).length();
        let level = |kart: &Kart| (kart.pos.y - at.y).abs() < HILL_LEVEL;
        let nearest = karts
            .iter()
            .filter(|kart| playing(kart) && (*lain > LOOSE_FOR || (level(kart) && off(kart) < FLAG_REACH)))
            .min_by(|a, b| off(a).total_cmp(&off(b)));
        taker = nearest.map(|kart| kart.slot);
    } else if !karts.iter().any(|kart| kart.it) {
        // Whoever had it has gone: it is drawn for again.
        draw(&mut karts, &mut dice, None, &[]);
    } else {
        taker = bumped(&karts, &bumps).first().map(|met| met.1).filter(|_| play.held <= 0.0);
    }
    if let Some(taker) = taker {
        (play.held, play.loose) = (settings.tag_back(), None);
        for mut kart in &mut karts {
            // The driver it is taken from starts at it.
            kart.cues.hit |= kart.it;
            kart.it = kart.slot == taker;
            if kart.it && settings.tag_back() > 0.0 {
                (kart.shield, kart.shield_level) = (settings.tag_back(), 0);
            }
        }
    }
    for mut kart in karts.iter_mut().filter(|kart| kart.it) {
        kart.stake += dt;
    }
    let lies = play.loose.map(|(at, _)| at);
    if mark.0 != lies {
        mark.0 = lies;
    }
    scored(&mut karts, &settings, race.time);
}

/// How far from the middle of the hill a car is on it, and how far above or below;
/// how often the hill moves, in seconds; and how long a car that has been spun
/// round or thrown is no king of it.
pub const HILL_REACH: f32 = 7.0;
const HILL_LEVEL: f32 = 2.5;
pub const HILL_MOVES: f32 = 30.0;
const KNOCKED: f32 = 3.0;

/// King of the hill: a car alone on the hill is its king, and whoever has been
/// king longest wins. The hill moves every so often (`Track::hill_at`), a car
/// hit by something fired at it is no king for a moment, and by teams a hill
/// with only one team's cars on it is held by the first of them.
pub fn hill(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    track: Res<crate::track::Track>,
    mut play: ResMut<Play>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::Hill {
        return;
    }
    let play = &mut *play;
    if !play.begun {
        play.begun = true;
        for mut kart in &mut karts {
            (kart.stake, kart.it) = (0.0, false);
        }
    }
    if !on(&race) || !karts.iter().any(|kart| playing(kart)) {
        return;
    }
    let dt = time.delta_secs();
    for kart in &karts {
        let hit = struck(play, kart);
        if let Some(spared) = play.spared.get_mut(kart.slot) {
            *spared = if hit { KNOCKED } else { (*spared - dt).max(0.0) };
        }
    }
    let top = track.hill_at(race.time);
    let on_it = |kart: &Kart| {
        let from = kart.pos - top;
        playing(kart) && from.with_y(0.0).length() < HILL_REACH && from.y.abs() < HILL_LEVEL
    };
    let mut there: Vec<usize> = karts.iter().filter(|kart| on_it(kart)).map(|kart| kart.slot).collect();
    there.sort();
    let held = there.len() == 1 || there.iter().all(|slot| allies(&settings, *slot, there[0]));
    let king = there
        .first()
        .copied()
        .filter(|king| held && play.spared.get(*king).is_none_or(|left| *left <= 0.0));
    for mut kart in &mut karts {
        let is = king == Some(kart.slot);
        if kart.it != is {
            kart.it = is;
        }
        if is {
            kart.stake += dt;
        }
    }
    scored(&mut karts, &settings, race.time);
}

/// What the display counts for a car where a race counts its laps: the word for it
/// and how many, if the race is one that laps don't end.
pub fn left(settings: &Settings, kart: &Kart) -> Option<(&'static str, String)> {
    let whole = || (kart.stake as u32).to_string();
    match settings.mode {
        Mode::Battle => Some(("LIVES", whole())),
        Mode::Tag if kart.it => Some(("IT", (kart.stake.ceil() as u32).to_string())),
        Mode::Tag => Some(("SAFE", (kart.stake.ceil() as u32).to_string())),
        Mode::Infection if kart.it => Some(("IT", whole())),
        Mode::Infection => Some(("SAFE", whole())),
        Mode::KeepAway if kart.it => Some(("FLAG", whole())),
        Mode::KeepAway => Some(("HELD", whole())),
        Mode::Hill if kart.it => Some(("KING", whole())),
        Mode::Hill => Some(("HILL", whole())),
        Mode::Race | Mode::Elimination => None,
    }
}

/// What a team is called, and its colour.
pub const TEAMS: [(&str, Color); 2] = [
    ("BLUE", Color::srgb(0.35, 0.65, 1.0)),
    ("GOLD", Color::srgb(1.0, 0.82, 0.2)),
];

/// What the display's list of the cars says of one, after its name.
pub fn tally(settings: &Settings, kart: &Kart) -> String {
    match settings.mode {
        Mode::Battle if settings.returns => format!("{} KO {}", kart.stake as u32, kart.downs),
        Mode::Tag if !kart.it => String::new(),
        _ => (kart.stake.max(0.0).ceil() as u32).to_string(),
    }
}

/// What has just happened in the game, for the display to say: each line and how
/// long it has left to be shown. Worked out on every game from what it is told of
/// the cars, so nothing more is sent for it.
#[derive(Resource, Default)]
pub struct Feed {
    pub lines: Vec<(String, f32)>,
    /// Each car as it was when last looked at, by grid slot: whether it was "it",
    /// what it had, its knock-outs and whether it was out.
    was: [Option<(bool, f32, u8, bool)>; SLOTS],
    /// Which hill it was, and whether a flag lay loose.
    hill: usize,
    loose: bool,
}

/// How long a line of the feed is shown, and how many there are at the most.
const SAID_FOR: f32 = 4.0;
const SAID: usize = 4;

/// Says what has happened in the game as it happens, with a sound: a life lost,
/// a car caught or knocked out, the flag taken or dropped, a new king, the hill
/// moved.
pub fn notice(
    time: Res<Time<Real>>,
    race: Res<Race>,
    settings: Res<Settings>,
    mark: Res<Mark>,
    mut feed: ResMut<Feed>,
    mut sfx: ResMut<crate::audio::Sfx>,
    karts: Query<&Kart>,
) {
    if !settings.lapless() {
        return;
    }
    let dt = time.delta_secs();
    feed.lines.retain_mut(|line| {
        line.1 -= dt;
        line.1 > 0.0
    });
    if !on(&race) {
        feed.was = default();
        return;
    }
    let mut said: Vec<String> = Vec::new();
    for kart in &karts {
        let now = (kart.it, kart.stake, kart.downs, kart.out.is_some());
        let Some(was) = feed.was.get_mut(kart.slot).and_then(|was| was.replace(now)) else {
            continue;
        };
        let name = &kart.name;
        let became = now.0 && !was.0;
        let words = match settings.mode {
            _ if now.3 && !was.3 => Some("IS OUT"),
            Mode::Battle if now.2 > was.2 => Some("IS KNOCKED OUT"),
            Mode::Battle if now.1 < was.1 => Some("LOST A LIFE"),
            Mode::Tag if became => Some("IS IT"),
            Mode::Infection if became => Some("IS CAUGHT"),
            Mode::KeepAway if became => Some("HAS THE FLAG"),
            Mode::Hill if became => Some("IS KING"),
            _ => None,
        };
        said.extend(words.map(|words| format!("{name} {words}")));
    }
    let loose = settings.mode == Mode::KeepAway && mark.0.is_some();
    if std::mem::replace(&mut feed.loose, loose) != loose && loose {
        said.push("THE FLAG IS LOOSE".into());
    }
    let hill = (race.time.max(0.0) / HILL_MOVES) as usize;
    if std::mem::replace(&mut feed.hill, hill) != hill && settings.mode == Mode::Hill && hill > 0 {
        said.push("THE HILL HAS MOVED".into());
    }
    if !said.is_empty() {
        sfx.play(crate::audio::id::BRICK_RESPAWN);
    }
    feed.lines.extend(said.into_iter().map(|line| (line, SAID_FOR)));
    let extra = feed.lines.len().saturating_sub(SAID);
    feed.lines.drain(..extra);
}

/// What a car that is "it" is called over it and at the edge of the screen: the
/// car to keep away from, the one to go after, or the one to push off the hill.
pub fn called(mode: Mode) -> &'static str {
    match mode {
        Mode::KeepAway => "FLAG",
        Mode::Hill => "KING",
        _ => "IT",
    }
}

/// And what is said of a player who is, after their name.
pub fn said(mode: Mode) -> &'static str {
    match mode {
        Mode::KeepAway => "HAS THE FLAG",
        Mode::Hill => "IS KING",
        _ => "IS IT",
    }
}

/// The beacon over what a game is played for, and the ring round it on the ground.
#[derive(Component)]
pub struct Beacon {
    ring: bool,
}

/// How high the beacon stands, how wide it is, and its colour.
const BEACON: (f32, f32) = (30.0, 0.6);
const BEACON_COLOUR: Color = Color::srgba(1.0, 0.85, 0.1, 0.45);

/// In king of the hill the hill has a beacon standing on it, to be seen from all
/// over the arena, and a ring round it as far out as a car is on it; they move
/// with the hill. In keep-away a flag lying loose has them, the ring as far out
/// as a car has the flag.
pub fn beacon(
    mut commands: Commands,
    settings: Res<Settings>,
    race: Res<Race>,
    track: Res<crate::track::Track>,
    mark: Option<Res<Mark>>,
    mut beacons: Query<(&Beacon, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (at, reach) = match settings.mode {
        Mode::Hill => (Some(track.hill_at(race.time)), HILL_REACH),
        Mode::KeepAway => (mark.and_then(|mark| mark.0), FLAG_REACH),
        _ => return,
    };
    if beacons.is_empty() {
        let material = materials.add(StandardMaterial {
            base_color: BEACON_COLOUR,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        let shapes = [
            (false, meshes.add(Cylinder::new(BEACON.1, BEACON.0))),
            (true, meshes.add(Torus::new(0.95, 1.0))),
        ];
        for (ring, mesh) in shapes {
            commands.spawn((
                Beacon { ring },
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                Visibility::Hidden,
                bevy::light::NotShadowCaster,
                DespawnOnExit(crate::menu::Screen::Race),
            ));
        }
    }
    for (beacon, mut transform, mut visibility) in &mut beacons {
        let shown = if at.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != shown {
            *visibility = shown;
        }
        let Some(at) = at else { continue };
        let placed = if beacon.ring {
            Transform::from_translation(at + Vec3::Y * 0.15).with_scale(Vec3::new(reach, 0.3, reach))
        } else {
            Transform::from_translation(at + Vec3::Y * BEACON.0 / 2.0)
        };
        if *transform != placed {
            *transform = placed;
        }
    }
}

/// In a game of tag the car that is "it" is quicker than the rest. In infection it
/// is not: a car is caught by being cornered. Run on every game alike, so that a player's game drives their own car as the
/// host does.
pub fn pace(settings: Res<Settings>, mut karts: Query<&mut Kart>) {
    if settings.mode != Mode::Tag {
        return;
    }
    for mut kart in &mut karts {
        let pace = settings.pace() * if kart.it { IT_PACE } else { 1.0 };
        if kart.pace != pace {
            kart.pace = pace;
        }
    }
}

/// What the glow of the car that is "it" is drawn with: a material that nothing
/// in front of it hides.
pub type Seen = ExtendedMaterial<StandardMaterial, Through>;

/// Makes a material one that is drawn whatever is nearer the camera than it, and
/// in its own colour whatever colours its mesh has.
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct Through {}

impl MaterialExtension for Through {
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_compare = Some(CompareFunction::Always);
        }
        let coloured = |def: &ShaderDefVal| matches!(def, ShaderDefVal::Bool(name, _) if name == "VERTEX_COLORS");
        descriptor.vertex.shader_defs.retain(|def| !coloured(def));
        if let Some(fragment) = &mut descriptor.fragment {
            fragment.shader_defs.retain(|def| !coloured(def));
        }
        Ok(())
    }
}

/// A piece of the glow of a car, which is that entity: one of the car's meshes
/// again, in the glow's colour.
#[derive(Component)]
pub struct Glow(Entity);

/// A car that has its glow.
#[derive(Component)]
pub struct Glowing;

/// The glow's colour, how much of what is behind it it hides at its faintest and
/// its strongest, and how fast it goes from one to the other.
const GLOW: Color = Color::srgb(1.0, 0.1, 0.05);
const GLOW_LIGHT: (f32, f32) = (0.45, 0.85);
const GLOW_RATE: f32 = 6.0;

/// The car that is "it" glows, whatever that is in the game being played: the car
/// itself, drawn again in red through whatever is between it and the camera, so
/// that where it is and how far off can be seen.
pub fn glow(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<Settings>,
    karts: Query<(Entity, &Kart, Has<Glowing>)>,
    children: Query<&Children>,
    parts: Query<(&Mesh3d, Option<&SkinnedMesh>), (Without<Glow>, Without<crate::kart::Shield>)>,
    glows: Query<(Entity, &Glow)>,
    mut made: Local<Option<Handle<Seen>>>,
    mut materials: ResMut<Assets<Seen>>,
) {
    for (entity, kart, glowing) in &karts {
        let wanted = settings.lapless() && kart.it && kart.out.is_none();
        if wanted == glowing {
            continue;
        }
        if !wanted {
            commands.entity(entity).remove::<Glowing>();
            for (piece, _) in glows.iter().filter(|(_, glow)| glow.0 == entity) {
                commands.entity(piece).despawn();
            }
            continue;
        }
        let material = made.get_or_insert_with(|| {
            materials.add(ExtendedMaterial {
                base: StandardMaterial {
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                },
                extension: Through {},
            })
        });
        commands.entity(entity).insert(Glowing);
        for part in children.iter_descendants(entity) {
            let Ok((mesh, skin)) = parts.get(part) else {
                continue;
            };
            // Each piece is its mesh's child, to move and to be hidden with it.
            let piece = commands
                .spawn((
                    Glow(entity),
                    mesh.clone(),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                    bevy::light::NotShadowCaster,
                    ChildOf(part),
                ))
                .id();
            if let Some(skin) = skin {
                commands.entity(piece).insert(skin.clone());
            }
        }
    }
    // It swells and dies away.
    if let Some(mut material) = made.as_ref().and_then(|made| materials.get_mut(made)) {
        let swell = 0.5 + 0.5 * (time.elapsed_secs() * GLOW_RATE).sin();
        let light = GLOW_LIGHT.0 + (GLOW_LIGHT.1 - GLOW_LIGHT.0) * swell;
        material.base.base_color = GLOW.with_alpha(light);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Track;
    use bevy::ecs::system::RunSystemOnce;

    fn world(mode: Mode, cars: usize) -> World {
        let track = Track::new();
        let mut world = World::new();
        let mut settings = Settings::new(&crate::menu::Circuits(Vec::new()));
        settings.mode = mode;
        world.insert_resource(settings);
        world.insert_resource(Race {
            phase: Phase::Racing,
            intro: 0.0,
            countdown: 0.0,
            time: 10.0,
            demo: false,
            quick: true,
        });
        world.insert_resource(Play::default());
        world.insert_resource(Mark::default());
        world.insert_resource(Bumps::default());
        world.insert_resource(Rng(7));
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs(1));
        world.insert_resource(time);
        for slot in 0..cars {
            world.spawn(Kart::new(&track, slot));
        }
        world
    }

    /// What the tests look at of a car.
    struct Seen {
        stake: f32,
        it: bool,
        out: Option<f32>,
        finished: Option<f32>,
        place: usize,
        pace: f32,
    }

    /// The cars, in the order of the grid.
    fn cars(world: &mut World) -> Vec<Seen> {
        let mut cars: Vec<&Kart> = world.query::<&Kart>().iter(world).collect();
        cars.sort_by_key(|kart| kart.slot);
        cars.iter()
            .map(|kart| Seen {
                stake: kart.stake,
                it: kart.it,
                out: kart.out,
                finished: kart.finished,
                place: kart.place,
                pace: kart.pace,
            })
            .collect()
    }

    fn car(world: &mut World, slot: usize) -> Mut<'_, Kart> {
        let mut query = world.query::<&mut Kart>();
        query
            .iter_mut(world)
            .find(|kart| kart.slot == slot)
            .expect("a car in the slot")
    }

    #[test]
    fn the_caught_catch_too_and_a_game_is_of_rounds() {
        let mut world = world(Mode::Infection, 4);
        world.run_system_once(infection).unwrap();
        let first = cars(&mut world).iter().position(|kart| kart.it).unwrap();
        let rest: Vec<usize> = (0..4).filter(|slot| *slot != first).collect();
        // For the first moments the car drawn catches nobody.
        world.resource_mut::<Bumps>().0 = vec![(first, rest[0])];
        world.run_system_once(infection).unwrap();
        assert_eq!(cars(&mut world).iter().filter(|kart| kart.it).count(), 1);
        for _ in 0..2 {
            world.run_system_once(infection).unwrap();
        }
        // After that a car it bumps is "it" as well, and the catch is worth
        // something to the car that made it.
        assert!(car(&mut world, rest[0]).it && car(&mut world, first).it);
        assert_eq!(car(&mut world, first).stake, CATCH);
        assert_eq!(car(&mut world, rest[0]).stake, 2.0);
        // The car just caught whirls, and catches nobody for a moment: here that
        // moment is over by the next step, and it catches in its turn.
        assert!(car(&mut world, rest[0]).spin > 0.0);
        world.run_system_once(infection).unwrap();
        world.resource_mut::<Bumps>().0 = vec![(rest[1], rest[0])];
        world.run_system_once(infection).unwrap();
        // One car is left that wasn't caught, and the round is over: another is
        // begun, by a car that hasn't begun one, with what each had kept.
        let all = cars(&mut world);
        assert!(all.iter().all(|kart| kart.finished.is_none()));
        assert_eq!(all.iter().filter(|kart| kart.it).count(), 1);
        assert!(!all[first].it && all[first].stake == CATCH);
        assert!(all[rest[2]].stake > all[rest[1]].stake);
        // When its time is up the game is over, whatever round it is in, and
        // the cars are home in the order of what they have.
        world.resource_mut::<Race>().time = 240.0;
        world.resource_mut::<Bumps>().0.clear();
        world.run_system_once(infection).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        let all = cars(&mut world);
        assert!(all.iter().all(|kart| kart.finished.is_some() && !kart.it));
        let most = all.iter().map(|kart| kart.stake).fold(0.0, f32::max);
        assert!(all.iter().any(|kart| kart.place == 1 && kart.stake == most));
        // A car that is "it" is no quicker than one that isn't.
        let mut world = self::world(Mode::Infection, 3);
        world.run_system_once(infection).unwrap();
        world.run_system_once(pace).unwrap();
        assert!(cars(&mut world).iter().all(|kart| kart.pace == 1.0));
    }

    #[test]
    fn a_flag_knocked_loose_is_whoevers_is_first_to_it() {
        let mut world = world(Mode::KeepAway, 3);
        world.run_system_once(keep_away).unwrap();
        let first = cars(&mut world).iter().position(|kart| kart.it).unwrap();
        let (other, third) = ((first + 1) % 3, (first + 2) % 3);
        let at = car(&mut world, first).pos;
        for (slot, off) in [(other, 30.0), (third, 60.0)] {
            car(&mut world, slot).pos = at + Vec3::X * off;
        }
        // Spun round by something fired at it, the car that has it has it no more:
        // it lies where that car was, and is told to the players.
        car(&mut world, first).spin_round(1.0);
        world.run_system_once(keep_away).unwrap();
        assert!(cars(&mut world).iter().all(|kart| !kart.it));
        assert_eq!(world.resource::<Mark>().0, Some(at));
        // The first car to come to it has it, with a shield for a moment.
        car(&mut world, first).pos = at + Vec3::X * 90.0;
        car(&mut world, third).pos = at + Vec3::X * 2.0;
        world.run_system_once(keep_away).unwrap();
        assert!(car(&mut world, third).it && car(&mut world, third).shield > 0.0);
        assert_eq!(world.resource::<Mark>().0, None);
        // Left lying long enough, it is the nearest car's wherever that is.
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs(3));
        car(&mut world, third).spin = 0.0;
        car(&mut world, third).shield = 0.0;
        car(&mut world, third).spin_round(1.0);
        world.run_system_once(keep_away).unwrap();
        for slot in 0..3 {
            car(&mut world, slot).pos += Vec3::X * 50.0;
        }
        for _ in 0..3 {
            world.run_system_once(keep_away).unwrap();
        }
        assert_eq!(cars(&mut world).iter().filter(|kart| kart.it).count(), 1);
    }

    #[test]
    fn a_car_is_after_another_or_being_got_away_from() {
        // In tag the car that is "it" is after the rest; in keep-away the rest are
        // after the car with the flag, but for its own team; a battle has no such.
        let mut world = world(Mode::Tag, 4);
        car(&mut world, 1).it = true;
        let after = |world: &mut World, slot: usize| {
            let settings = world.resource::<Settings>().clone();
            chasing(&settings, &car(world, slot), &[1])
        };
        assert_eq!([1, 2].map(|slot| after(&mut world, slot)), [Some(true), Some(false)]);
        world.resource_mut::<Settings>().mode = Mode::KeepAway;
        assert_eq!([1, 2, 3].map(|slot| after(&mut world, slot)), [Some(false), Some(true), Some(true)]);
        world.resource_mut::<Settings>().teams = true;
        assert_eq!([1, 2, 3].map(|slot| after(&mut world, slot)), [Some(false), Some(true), Some(false)]);
        world.resource_mut::<Settings>().mode = Mode::Battle;
        assert_eq!(after(&mut world, 1), None);
        assert!(suits(Mode::Hill) && suits(Mode::Infection) && !suits(Mode::Battle) && !suits(Mode::Race));
    }

    #[test]
    fn two_teams_share_what_they_have() {
        // Keep-away by teams: the even slots against the odd. One team's seconds
        // are its cars' together, and the team that gets there first has won.
        let mut world = world(Mode::KeepAway, 4);
        (world.resource_mut::<Settings>().teams, world.resource_mut::<Settings>().goal) = (true, 0);
        world.run_system_once(keep_away).unwrap();
        let holder = cars(&mut world).iter().position(|kart| kart.it).unwrap();
        let mate = (holder + 2) % 4;
        car(&mut world, holder).stake = 9.5;
        car(&mut world, mate).stake = 9.6;
        world.run_system_once(keep_away).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        let all = cars(&mut world);
        assert!(all.iter().all(|kart| kart.finished.is_some()));
        let mut won = [all[holder].place, all[mate].place];
        won.sort();
        assert_eq!(won, [1, 2]);
        // A battle by teams is over when one team has nobody left in it.
        let mut world = self::world(Mode::Battle, 4);
        world.resource_mut::<Settings>().teams = true;
        world.run_system_once(battle).unwrap();
        for slot in [1, 3] {
            car(&mut world, slot).stake = 1.0;
            car(&mut world, slot).spin_round(1.0);
        }
        world.run_system_once(battle).unwrap();
        let all = cars(&mut world);
        assert!(all[1].out.is_some() && all[3].out.is_some());
        assert!(all[0].finished.is_some() && all[2].finished.is_some());
    }

    #[test]
    fn a_car_knocked_out_comes_back_where_the_host_has_it_so() {
        let mut world = world(Mode::Battle, 2);
        world.resource_mut::<Settings>().returns = true;
        world.run_system_once(battle).unwrap();
        car(&mut world, 0).stake = 1.0;
        car(&mut world, 0).spin_round(1.0);
        world.run_system_once(battle).unwrap();
        // It is held where it is a moment, with its lives again and a knock-out
        // against it; nobody is out, and the game goes on until its time is up.
        let all = cars(&mut world);
        assert!(all[0].out.is_none() && all[0].finished.is_none() && all[1].finished.is_none());
        assert_eq!((all[0].stake, car(&mut world, 0).downs), (3.0, 1));
        assert!(car(&mut world, 0).magnet > 0.0);
        car(&mut world, 1).stake = 1.0;
        world.resource_mut::<Race>().time = 240.0;
        world.run_system_once(battle).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        // One life and no knock-outs is better than three and one.
        let all = cars(&mut world);
        assert_eq!((all[1].place, all[0].place), (1, 2));
    }

    #[test]
    fn what_is_fired_at_the_car_that_is_it_runs_its_clock_down() {
        let mut world = world(Mode::Tag, 3);
        world.run_system_once(tag).unwrap();
        let it = cars(&mut world).iter().position(|kart| kart.it).unwrap();
        let clock = car(&mut world, it).stake;
        car(&mut world, (it + 1) % 3).spin_round(1.0);
        world.run_system_once(tag).unwrap();
        assert_eq!(car(&mut world, it).stake, clock - 1.0);
        car(&mut world, it).spin_round(1.0);
        world.run_system_once(tag).unwrap();
        assert_eq!(car(&mut world, it).stake, clock - 2.0 - TAG_HIT);
    }

    #[test]
    fn the_flag_is_taken_by_a_bump_and_won_by_holding_it() {
        let mut world = world(Mode::KeepAway, 3);
        world.resource_mut::<Settings>().goal = 0;
        world.run_system_once(keep_away).unwrap();
        let first = cars(&mut world).iter().position(|kart| kart.it).unwrap();
        let (other, third) = ((first + 1) % 3, (first + 2) % 3);
        // A second of the step has gone to whoever has it.
        assert_eq!(car(&mut world, first).stake, 1.0);
        // Two cars without it bumping take nothing; one bumping its holder has it,
        // and keeps what the car it was taken from had held it for.
        world.resource_mut::<Bumps>().0 = vec![(other, third)];
        world.run_system_once(keep_away).unwrap();
        assert!(car(&mut world, first).it);
        world.resource_mut::<Bumps>().0 = vec![(other, first)];
        world.run_system_once(keep_away).unwrap();
        let all = cars(&mut world);
        assert!(all[other].it && !all[first].it);
        assert_eq!((all[first].stake, all[other].stake), (2.0, 1.0));
        // It can't be taken straight back.
        world.run_system_once(keep_away).unwrap();
        assert!(car(&mut world, other).it);
        // Held for as long as the host says, it is won, and the rest come in the
        // order of how long they held it.
        world.resource_mut::<Bumps>().0.clear();
        car(&mut world, other).stake = 19.5;
        world.run_system_once(keep_away).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        let all = cars(&mut world);
        assert!(all.iter().all(|kart| kart.finished.is_some() && !kart.it));
        assert_eq!((all[other].place, all[first].place, all[third].place), (1, 2, 3));
    }

    #[test]
    fn a_car_alone_on_the_hill_is_its_king() {
        let mut world = world(Mode::Hill, 3);
        world.insert_resource(Track::new());
        let top = world.resource::<Track>().hill_at(10.0);
        for slot in 0..3 {
            car(&mut world, slot).pos = top + Vec3::X * 40.0 * (slot + 1) as f32;
        }
        world.run_system_once(hill).unwrap();
        assert!(cars(&mut world).iter().all(|kart| !kart.it && kart.stake == 0.0));
        // One car on it is its king, and counts the time it is.
        car(&mut world, 0).pos = top + Vec3::X * 2.0;
        world.run_system_once(hill).unwrap();
        assert!(car(&mut world, 0).it);
        assert_eq!(car(&mut world, 0).stake, 1.0);
        // With another there nobody is; one over or under it is not on it.
        car(&mut world, 1).pos = top - Vec3::Z * 3.0;
        world.run_system_once(hill).unwrap();
        assert!(cars(&mut world).iter().all(|kart| !kart.it));
        car(&mut world, 1).pos = top + Vec3::Y * 6.0;
        world.run_system_once(hill).unwrap();
        assert_eq!(car(&mut world, 0).stake, 2.0);
        // When its time is up whoever has been king longest has won.
        world.resource_mut::<Race>().time = 240.0;
        world.run_system_once(hill).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        let all = cars(&mut world);
        assert!(all[0].finished.is_some() && all[0].place == 1);
    }

    #[test]
    fn a_hit_takes_a_life_and_the_last_car_left_wins() {
        let mut world = world(Mode::Battle, 3);
        world.run_system_once(battle).unwrap();
        assert!(cars(&mut world).iter().all(|kart| kart.stake == 3.0));
        // A blast throws a car for as long as it is in reach, and a car that drives
        // for itself is over being thrown by the next step: that is one life all
        // the same, and not one for every step of it.
        for _ in 0..3 {
            assert!(car(&mut world, 0).launch(1.0));
            car(&mut world, 0).spin_out = 0.0;
        }
        world.run_system_once(battle).unwrap();
        assert_eq!(car(&mut world, 0).stake, 2.0);
        // Thrown again at once it is spared; a step of a second on, it isn't, and
        // a spin costs it what a throw does. With no lives it is out.
        car(&mut world, 0).launch(1.0);
        world
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(100));
        world.run_system_once(battle).unwrap();
        assert_eq!(car(&mut world, 0).stake, 2.0);
        world
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(2));
        world.run_system_once(battle).unwrap();
        for _ in 0..2 {
            car(&mut world, 0).spin = 0.0;
            car(&mut world, 0).spin_round(1.0);
            world.run_system_once(battle).unwrap();
        }
        let all = cars(&mut world);
        assert!(all[0].out.is_some() && all[0].finished.is_none());
        assert!(all[1].finished.is_none() && all[2].finished.is_none());
        // A second car out leaves the winner, who is home.
        car(&mut world, 1).stake = 1.0;
        car(&mut world, 1).spin_round(1.0);
        world.run_system_once(battle).unwrap();
        let all = cars(&mut world);
        assert!(all[1].out.is_some() && all[2].out.is_none());
        assert!(all[2].finished.is_some());
    }

    #[test]
    fn a_battle_out_of_time_goes_to_whoever_has_most_left() {
        let mut world = world(Mode::Battle, 3);
        world.run_system_once(battle).unwrap();
        car(&mut world, 1).stake = 1.0;
        car(&mut world, 2).stake = 2.0;
        world.resource_mut::<Race>().time = 240.0;
        world.run_system_once(battle).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        let places: Vec<usize> = cars(&mut world).iter().map(|kart| kart.place).collect();
        assert_eq!(places, [1, 3, 2]);
    }

    #[test]
    fn a_battle_and_a_game_of_tag_are_as_the_host_has_them() {
        // Five lives, and nobody spared: two hits in two steps take two of them.
        let mut world = world(Mode::Battle, 2);
        (world.resource_mut::<Settings>().lives, world.resource_mut::<Settings>().spared) = (5, 0);
        world.run_system_once(battle).unwrap();
        for left in [4.0, 3.0] {
            car(&mut world, 0).spin = 0.0;
            car(&mut world, 0).spin_round(1.0);
            world.run_system_once(battle).unwrap();
            assert_eq!(car(&mut world, 0).stake, left);
        }
        // A minute's battle is over at a minute.
        world.resource_mut::<Settings>().battle_time = 0;
        world.resource_mut::<Race>().time = 60.0;
        world.run_system_once(battle).unwrap();
        assert!(cars(&mut world).iter().all(|kart| kart.finished.is_some()));
        // With no time to catch back in and nothing put on the clock, a car caught
        // at the last is out.
        let mut world = self::world(Mode::Tag, 3);
        (world.resource_mut::<Settings>().tag_back, world.resource_mut::<Settings>().tag_least) =
            (0, 0);
        world.run_system_once(tag).unwrap();
        let it = cars(&mut world).iter().position(|kart| kart.it).unwrap();
        let other = (it + 1) % 3;
        for (from, to) in [(it, other), (other, it)] {
            world.resource_mut::<Bumps>().0 = vec![(from, to)];
            world.run_system_once(tag).unwrap();
            assert!(car(&mut world, to).it);
        }
        for slot in 0..3 {
            car(&mut world, slot).stake = 0.5;
        }
        world.resource_mut::<Bumps>().0 = vec![(it, other)];
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, other).out.is_some());
    }

    #[test]
    fn it_is_passed_on_by_a_bump_and_not_straight_back() {
        let mut world = world(Mode::Tag, 3);
        world.run_system_once(tag).unwrap();
        let all = cars(&mut world);
        assert_eq!(all.iter().filter(|kart| kart.it).count(), 1);
        let it = all.iter().position(|kart| kart.it).unwrap();
        let (other, third) = ((it + 1) % 3, (it + 2) % 3);
        // The one clock has run for the one second of that step, for them all.
        assert!(all.iter().all(|kart| kart.stake == round(3) - 1.0));
        // Two cars that aren't "it" bumping passes nothing.
        world.resource_mut::<Bumps>().0 = vec![(other, third)];
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, it).it);
        // A bump with the one that is does, either way round it is told.
        world.resource_mut::<Bumps>().0 = vec![(other, it)];
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, other).it && !car(&mut world, it).it);
        // A second later the same bump is too soon to pass it back; after two it does.
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, other).it);
        world.run_system_once(tag).unwrap();
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, it).it && !car(&mut world, other).it);
        // The car that is "it" is the quicker.
        world.run_system_once(pace).unwrap();
        let all = cars(&mut world);
        assert_eq!(all[it].pace, all[other].pace * IT_PACE);
    }

    #[test]
    fn the_car_that_is_it_when_the_clock_runs_out_is_out_until_one_is_left() {
        let mut world = world(Mode::Tag, 3);
        world.run_system_once(tag).unwrap();
        let first = cars(&mut world).iter().position(|kart| kart.it).unwrap();
        // The clock runs down whoever is "it": passed on, it is not put back.
        let other = (first + 1) % 3;
        world.resource_mut::<Bumps>().0 = vec![(first, other)];
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, other).it);
        assert_eq!(car(&mut world, other).stake, round(3) - 2.0);
        // Caught with the clock all but out, a car has three seconds of it.
        car(&mut world, other).stake = 2.5;
        world.resource_mut::<Bumps>().0 = vec![(first, other)];
        for _ in 0..2 {
            world.run_system_once(tag).unwrap();
        }
        assert!(car(&mut world, first).it);
        assert_eq!(car(&mut world, other).stake, 3.0);
        world.resource_mut::<Bumps>().0 = vec![(first, other)];
        for _ in 0..2 {
            world.run_system_once(tag).unwrap();
        }
        assert!(car(&mut world, other).it && car(&mut world, other).out.is_none());
        world.resource_mut::<Bumps>().0.clear();
        car(&mut world, other).stake = 0.5;
        world.run_system_once(tag).unwrap();
        // That car is out, the clock is put back and one of the two left is "it".
        let all = cars(&mut world);
        assert!(all[other].out.is_some() && !all[other].it);
        assert_eq!(all.iter().filter(|kart| kart.it).count(), 1);
        assert!(all.iter().all(|kart| kart.finished.is_none()));
        // A round of two is the shortest.
        assert_eq!((round(6), round(3), round(2), round(1)), (50.0, 35.0, 30.0, 30.0));
        assert_eq!(all[first].stake, TAG_TIME);
        let second = all.iter().position(|kart| kart.it).unwrap();
        world.resource_mut::<Race>().time = 40.0;
        car(&mut world, second).stake = 0.5;
        world.run_system_once(tag).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        // The one left has won, and the others are placed last out first.
        let all = cars(&mut world);
        let winner = 3 - other - second;
        assert!(all[second].out.is_some() && all[winner].finished.is_some());
        assert_eq!(
            (all[winner].place, all[second].place, all[other].place),
            (1, 2, 3)
        );
        assert!(all.iter().all(|kart| !kart.it));
        // Nothing more comes of it once it is over.
        world.run_system_once(tag).unwrap();
        assert_eq!(car(&mut world, winner).stake, TAG_TIME);
    }
}
