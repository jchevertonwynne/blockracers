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
    /// is left of the time it is spared another life in.
    hits: [u32; crate::kart::PLAYER_SLOT + 1],
    spared: [f32; crate::kart::PLAYER_SLOT + 1],
    /// What is left of the time "it" can't be passed on in.
    held: f32,
}

/// Whether the cars are being raced: the race goes on for the rest after the car
/// driven on the host's screen is done.
fn on(race: &Race) -> bool {
    matches!(race.phase, Phase::Racing | Phase::Finished)
}

/// The cars still in it are home, in the order of what they have left.
fn end<'a>(karts: impl Iterator<Item = Mut<'a, Kart>>, time: f32) {
    for mut kart in karts.filter(|kart| kart.out.is_none() && kart.finished.is_none()) {
        kart.finished = Some(time - kart.stake * APART);
    }
}

/// A battle: a car hit loses a life, and one with none left is out.
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
            kart.stake = settings.lives as f32;
        }
    }
    if !on(&race) {
        return;
    }
    let dt = time.delta_secs();
    let play = &mut *play;
    for mut kart in &mut karts {
        let (Some(seen), Some(spared)) =
            (play.hits.get_mut(kart.slot), play.spared.get_mut(kart.slot))
        else {
            continue;
        };
        let hit = std::mem::replace(seen, kart.hits) != kart.hits;
        *spared = (*spared - dt).max(0.0);
        if !hit || *spared > 0.0 || kart.out.is_some() || kart.finished.is_some() {
            continue;
        }
        *spared = settings.spared();
        kart.stake = (kart.stake - 1.0).max(0.0);
        debug!("{} has {} lives left", kart.name, kart.stake);
        if kart.stake <= 0.0 {
            put_out(&mut kart, race.time);
        }
    }
    let left = karts
        .iter()
        .filter(|kart| kart.out.is_none() && kart.finished.is_none())
        .count();
    if left == 1 || (left > 1 && race.time >= settings.battle_time()) {
        end(karts.iter_mut(), race.time);
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

/// One of the cars still in a game of tag is made "it".
fn draw(karts: &mut Query<&mut Kart>, dice: &mut Rng) {
    let count = karts.iter().filter(|kart| playing(kart)).count();
    let drawn = (dice.f() * count as f32) as usize % count.max(1);
    let mut left = karts.iter_mut().filter(|kart| playing(kart));
    if let Some(mut kart) = left.nth(drawn) {
        kart.it = true;
    }
}

/// A game of tag: "it" goes to whichever car the one that is "it" bumps, the car
/// that is "it" when the clock runs out is out, and the last car left wins.
pub fn tag(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    bumps: Res<Bumps>,
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
        draw(&mut karts, &mut dice);
    }
    // Over once one car is left.
    if !on(&race) || karts.iter().filter(|kart| playing(kart)).count() < 2 {
        return;
    }
    let dt = time.delta_secs();
    play.held = (play.held - dt).max(0.0);
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
    let mut clock = (clock - dt).max(least);
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
        end(karts.iter_mut(), race.time);
    } else {
        draw(&mut karts, &mut dice);
    }
}

/// The cars that bumped a car that is "it" this step, and are still in the game.
fn bumped(karts: &Query<&mut Kart>, bumps: &Bumps) -> Vec<usize> {
    let is = |slot: usize, it: bool| {
        karts
            .iter()
            .any(|kart| kart.slot == slot && kart.it == it && playing(kart))
    };
    let met = bumps.0.iter().flat_map(|&(a, b)| [(a, b), (b, a)]);
    met.filter(|&(it, other)| is(it, true) && is(other, false))
        .map(|(_, other)| other)
        .collect()
}

/// How long the cars of a game of infection have to get away from the one drawn,
/// which catches nobody until then: they start side by side.
const HEAD_START: f32 = 3.0;

/// Infection: every car a car that is "it" bumps is "it" from then on, and the
/// last car that isn't has won.
pub fn infection(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    bumps: Res<Bumps>,
    mut play: ResMut<Play>,
    mut dice: ResMut<Rng>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::Infection {
        return;
    }
    if !play.begun {
        play.begun = true;
        for mut kart in &mut karts {
            (kart.stake, kart.it) = (0.0, false);
        }
        draw(&mut karts, &mut dice);
        play.held = HEAD_START;
    }
    if !on(&race) || !karts.iter().any(|kart| playing(kart)) {
        return;
    }
    let dt = time.delta_secs();
    play.held = (play.held - dt).max(0.0);
    let caught = if play.held > 0.0 {
        Vec::new()
    } else {
        bumped(&karts, &bumps)
    };
    for mut kart in karts.iter_mut().filter(|kart| caught.contains(&kart.slot)) {
        debug!("{} is caught at {:.1}s", kart.name, race.time);
        // The driver caught starts at it.
        (kart.it, kart.cues.hit) = (true, true);
    }
    for mut kart in karts.iter_mut().filter(|kart| playing(kart) && !kart.it) {
        kart.stake += dt;
    }
    let free = karts.iter().filter(|kart| playing(kart) && !kart.it).count();
    if free <= 1 || race.time >= settings.battle_time() {
        end(karts.iter_mut(), race.time);
    }
}

/// The game is whoever's has most of what it counts, once someone has all it
/// takes or its time is up.
fn scored(karts: &mut Query<&mut Kart>, settings: &Settings, time: f32) {
    let won = karts
        .iter()
        .any(|kart| playing(kart) && kart.stake >= settings.goal());
    if won || time >= settings.battle_time() {
        for mut kart in karts.iter_mut() {
            kart.it = false;
        }
        end(karts.iter_mut(), time);
    }
}

/// Keep-away: the car with the flag has it until another bumps it, and whoever
/// has held it longest wins.
pub fn keep_away(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    bumps: Res<Bumps>,
    mut play: ResMut<Play>,
    mut dice: ResMut<Rng>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::KeepAway {
        return;
    }
    if !play.begun {
        play.begun = true;
        for mut kart in &mut karts {
            (kart.stake, kart.it) = (0.0, false);
        }
        draw(&mut karts, &mut dice);
    }
    if !on(&race) || !karts.iter().any(|kart| playing(kart)) {
        return;
    }
    let dt = time.delta_secs();
    play.held = (play.held - dt).max(0.0);
    let taker = bumped(&karts, &bumps).first().copied().filter(|_| play.held <= 0.0);
    if let Some(taker) = taker {
        play.held = settings.tag_back();
        for mut kart in &mut karts {
            // The driver it is taken from starts at it.
            kart.cues.hit |= kart.it;
            kart.it = kart.slot == taker;
        }
    }
    for mut kart in karts.iter_mut().filter(|kart| kart.it) {
        kart.stake += dt;
    }
    scored(&mut karts, &settings, race.time);
}

/// How far from the middle of the hill a car is on it, and how far above or below.
pub const HILL_REACH: f32 = 7.0;
const HILL_LEVEL: f32 = 2.5;

/// King of the hill: a car alone on the hill is its king, and whoever has been
/// king longest wins.
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
    if !play.begun {
        play.begun = true;
        for mut kart in &mut karts {
            (kart.stake, kart.it) = (0.0, false);
        }
    }
    if !on(&race) || !karts.iter().any(|kart| playing(kart)) {
        return;
    }
    let top = track.hill();
    let on_it = |kart: &Kart| {
        let from = kart.pos - top;
        playing(kart) && from.with_y(0.0).length() < HILL_REACH && from.y.abs() < HILL_LEVEL
    };
    let alone = karts.iter().filter(|kart| on_it(kart)).count() == 1;
    let dt = time.delta_secs();
    for mut kart in &mut karts {
        let king = alone && on_it(&kart);
        if kart.it != king {
            kart.it = king;
        }
        if king {
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

/// The hill's beacon, and the ring round the hill at its foot.
#[derive(Component)]
pub struct Beacon;

/// How high the beacon stands, how wide it is, and its colour.
const BEACON: (f32, f32) = (30.0, 0.6);
const BEACON_COLOUR: Color = Color::srgba(1.0, 0.85, 0.1, 0.45);

/// In king of the hill the hill has a beacon standing on it, to be seen from all
/// over the arena, and a ring round it as far out as a car is on it.
pub fn beacon(
    mut commands: Commands,
    settings: Res<Settings>,
    track: Res<crate::track::Track>,
    beacons: Query<(), With<Beacon>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if settings.mode != Mode::Hill || !beacons.is_empty() {
        return;
    }
    let material = materials.add(StandardMaterial {
        base_color: BEACON_COLOUR,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let top = track.hill();
    let scoped = || DespawnOnExit(crate::menu::Screen::Race);
    commands.spawn((
        Beacon,
        Mesh3d(meshes.add(Cylinder::new(BEACON.1, BEACON.0))),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(top + Vec3::Y * BEACON.0 / 2.0),
        bevy::light::NotShadowCaster,
        scoped(),
    ));
    commands.spawn((
        Beacon,
        Mesh3d(meshes.add(Torus::new(HILL_REACH - 0.3, HILL_REACH))),
        MeshMaterial3d(material),
        Transform::from_translation(top + Vec3::Y * 0.15).with_scale(Vec3::new(1.0, 0.3, 1.0)),
        bevy::light::NotShadowCaster,
        scoped(),
    ));
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
    fn the_caught_catch_too_and_the_last_car_uncaught_wins() {
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
        // After that a car it bumps is "it" as well, and catches in its turn.
        assert!(car(&mut world, rest[0]).it && car(&mut world, first).it);
        world.resource_mut::<Bumps>().0 = vec![(rest[1], rest[0])];
        world.run_system_once(infection).unwrap();
        // One car is left that wasn't caught, and the game is over: the cars are
        // home in the order of how long each lasted, the one drawn last of all.
        world.run_system_once(crate::kart::update_places).unwrap();
        let all = cars(&mut world);
        assert!(all.iter().all(|kart| kart.finished.is_some() && kart.out.is_none()));
        let places = [first, rest[0], rest[1], rest[2]].map(|slot| all[slot].place);
        assert_eq!(places, [4, 3, 2, 1]);
        // A car that is "it" is no quicker than one that isn't.
        let mut world = self::world(Mode::Infection, 3);
        world.run_system_once(infection).unwrap();
        world.run_system_once(pace).unwrap();
        assert!(cars(&mut world).iter().all(|kart| kart.pace == 1.0));
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
        let top = world.resource::<Track>().hill();
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
