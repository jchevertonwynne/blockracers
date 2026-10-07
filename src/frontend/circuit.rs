//! The turning view of a circuit on the race pages: a frame of `SINGRACE/PST.CDB`
//! for each race, shown through the frame's camera in the page's scene region.
//! Follows `SingleRaceSelectBase::CreateWidgets` (the scene widget, looping its
//! frame), `RaceModeSetupScreen::UpdateRacePreview` and
//! `CircuitRaceScreen::UpdateRacePreview` (which frame: the race's theme name in
//! `LEGORACE.RCB`; the circuit race page goes through the circuit's four races
//! every two seconds) and `MenuSceneScreen::SceneWidget` (the film's frame).
//!
//! The original draws it between the page's frame and an overlay that darkens it;
//! the port's menus are drawn over everything else, so it is drawn by a camera of
//! its own onto a picture, as the main menu's figure is (`mascot`), and the menu
//! shows the picture dimmed.
//!
//! Not as the original has it: the figure of the race's mascot (`SetPreviewDriver`)
//! is not stood in the scene, and the models are unlit, made as bright as the
//! frame's lights come to as `film` does.

use std::collections::HashMap;

use super::{Art, Menu, Page};
use crate::assets::tokens::{Token, tokenize};
use crate::championship::Championship;
use crate::film::{Showing, entries, number, text, vec3};
use crate::menu::{Circuits, Settings};
use crate::scenery::{self, Animated};
use crate::world::Library;
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    mesh::skinning::SkinnedMeshInverseBindposes,
    prelude::*,
    render::render_resource::TextureFormat,
};

const FILE: &str = "/MENUDATA/SINGRACE/PST.CDB";
const DIR: &str = "/MENUDATA/SINGRACE";
/// `CircuitRaceScreen::Update`: how long each race of a circuit is shown.
const EACH: f32 = 2.0;
/// How much of a directional light's colour everything is brightened by.
const BEAM_SHARE: f32 = 0.5;
const DETAIL: f32 = 2.0;
const LAYER: usize = 9;
/// How much the picture is darkened to be read over (the original's overlay).
pub const DIM: f32 = 0.55;

#[derive(Resource, Default)]
pub struct Preview {
    /// The picture it is drawn onto.
    pub picture: Option<Handle<Image>>,
    /// Which frame is shown, and what it is made of and seen through.
    shown: Option<String>,
    stage: Vec<Entity>,
    lit: Vec3,
}

/// A model of the frame, and the part of its animation it plays.
#[derive(Component)]
pub struct Cue(Option<usize>);

#[derive(Component)]
pub struct Started;

#[derive(Component)]
pub struct Staged;

/// The frame's theme name, for a race's folder: `RaceNameEntry::GetThemeName`.
fn theme(art: &Art, folder: &str) -> Option<String> {
    let tokens = tokenize(art.jam().get("/MENUDATA/LEGORACE.RCB")?);
    entries(&tokens, 0x27).into_iter().find_map(|(_, fields)| {
        (text(fields, 0x29)?.eq_ignore_ascii_case(folder))
            .then(|| text(fields, 0x2d))
            .flatten()
    })
}

/// Which race's frame the page shows.
fn wanted(
    menu: &Menu,
    time: f32,
    circuits: &Circuits,
    settings: &Settings,
    championship: &Championship,
) -> Option<String> {
    match menu.page {
        Page::SingleRace | Page::TimeRace => circuits.0.get(settings.circuit)?.race.clone(),
        Page::CircuitRace => {
            let series = championship.series.get(championship.chosen)?;
            // The last circuit has the one race, and no preview.
            (series.rounds.len() > 1)
                .then(|| series.rounds[(time / EACH) as usize % series.rounds.len()].clone())
        }
        _ => None,
    }
}

/// Puts the frame of the race on the stage: what the frame has, from `PST.CDB`.
fn stage(
    art: &Art,
    theme: &str,
    commands: &mut Commands,
    (meshes, materials, images, binds): (
        &mut Assets<Mesh>,
        &mut Assets<StandardMaterial>,
        &mut Assets<Image>,
        &mut Assets<SkinnedMeshInverseBindposes>,
    ),
) -> Option<(Vec<Entity>, (Vec3, Vec3, Vec3, f32), Vec3)> {
    let jam = art.jam();
    let tokens = tokenize(jam.get(FILE)?);
    let worlds = scenery::names(&tokens, 0x28);
    let frames = entries(&tokens, 0x27);
    let (_, frame) = frames.iter().find(|(name, _)| name == theme)?;
    let mut wanted: HashMap<String, Option<usize>> = HashMap::new();
    for (_, fields) in entries(frame, 0x2e) {
        let Some(model) = text(fields, 0x30).or_else(|| text(fields, 0x2f)) else {
            continue;
        };
        let part = number(fields, 0x2d, 0)
            .filter(|part| *part >= 0.0 && text(fields, 0x30).is_some())
            .map(|part| part as usize);
        wanted.insert(model, part);
    }
    // The figure of the race's mascot is the original's to stand in; here there is none.
    wanted.remove("guy1");
    let own: Vec<&str> = jam
        .list(DIR)
        .filter(|f| f.ends_with(".MDB") || f.ends_with(".TDB"))
        .collect();
    let mut library = Library::new(jam, own.iter().copied(), &[DIR]);
    library.plain();
    let files: Vec<String> = worlds
        .iter()
        .map(|world| format!("{DIR}/{}.WDB", world.to_uppercase()))
        .collect();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    let props = scenery::load_files(jam, DIR, &files, &library, |_, model| {
        wanted.contains_key(model)
    });
    let mut made = Vec::new();
    for def in props {
        let part = wanted.get(def.name()).copied().flatten();
        let prop = scenery::spawn(def, commands, meshes, materials, images, binds);
        commands.entity(prop).insert((Staged, Cue(part)));
        made.push(prop);
    }
    // The frame's lights: the ambient one and the directional ones.
    let colour = |fields: &[Token]| vec3(fields, 0x38, 0).unwrap_or_default() / 255.0;
    let glow = entries(frame, 0x35).first().map(|(_, f)| colour(f));
    let beams: Vec3 = entries(frame, 0x3a).iter().map(|(_, f)| colour(f)).sum();
    let lit = match glow {
        Some(glow) => (glow + beams * BEAM_SHARE).min(Vec3::ONE),
        None => Vec3::ONE,
    };
    // The camera the frame is seen through: `Camera01`, placed by the world file.
    let camera = worlds.iter().find_map(|world| {
        let file = format!("{DIR}/{}.WDB", world.to_uppercase());
        let tokens = tokenize(jam.get(&file)?);
        let (_, fields) = entries(&tokens, 0x43).into_iter().next()?;
        Some((
            vec3(fields, 0x31, 0)?,
            vec3(fields, 0x32, 0).unwrap_or(Vec3::X),
            vec3(fields, 0x32, 3).unwrap_or(Vec3::Z),
            number(fields, 0x47, 0).unwrap_or(36.0),
        ))
    })?;
    Some((made, camera, lit))
}

/// Keeps the view of the race the page is about, and nowhere else.
pub fn keep(
    mut commands: Commands,
    art: Res<Art>,
    time: Res<Time<Real>>,
    showing: Res<Showing>,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
    championship: Res<Championship>,
    mut menu: ResMut<Menu>,
    mut preview: ResMut<Preview>,
    (mut meshes, mut materials, mut images, mut binds): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    let folder = if showing.busy() {
        None
    } else {
        wanted(&menu, time.elapsed_secs(), &circuits, &settings, &championship)
    };
    let theme = folder.and_then(|folder| theme(&art, &folder));
    if theme == preview.shown {
        return;
    }
    clear(&mut commands, &mut preview);
    menu.drawn = false;
    let Some(theme) = theme else {
        return;
    };
    scenery::set_mirror(false);
    let Some((made, (eye, forward, up, fov), lit)) = stage(
        &art,
        &theme,
        &mut commands,
        (&mut meshes, &mut materials, &mut images, &mut binds),
    ) else {
        return;
    };
    let area = art.place("race", "singrace");
    let size = (area.size() * DETAIL).max(Vec2::ONE).as_uvec2();
    let picture = images.add(Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let way = |v: Vec3| scenery::to_world(v).normalize_or_zero();
    let lens = PerspectiveProjection {
        fov: fov.to_radians(),
        // The scene is hundreds of the game's units across.
        far: 2000.0,
        ..default()
    };
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -3,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            Projection::Perspective(lens),
            RenderTarget::from(picture.clone()),
            RenderLayers::layer(LAYER),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Transform::from_translation(scenery::to_world(eye)).looking_to(way(forward), way(up)),
        ))
        .id();
    preview.picture = Some(picture);
    preview.stage = made.into_iter().chain([camera]).collect();
    preview.shown = Some(theme);
    preview.lit = lit;
}

/// Starts each model's animation, and has its camera alone draw it, as bright as
/// the frame's lights come to.
pub fn dress(
    mut commands: Commands,
    preview: Res<Preview>,
    made: Query<(Entity, Option<&MeshMaterial3d<StandardMaterial>>), Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    staged: Query<(), With<Staged>>,
    mut starting: Query<(Entity, &mut Animated, &Cue), (With<Staged>, Without<Started>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mesh, material) in &made {
        if !parents.iter_ancestors(mesh).any(|above| staged.contains(above)) {
            continue;
        }
        commands.entity(mesh).insert(RenderLayers::layer(LAYER));
        let handle = material.and_then(|material| materials.get_mut(&material.0));
        if let Some(mut material) = handle {
            let alpha = material.base_color.alpha();
            let lit = preview.lit;
            material.base_color = Color::srgba(lit.x, lit.y, lit.z, alpha);
        }
    }
    for (entity, mut animated, cue) in &mut starting {
        if let Some(part) = cue.0 {
            animated.play(part, true);
        }
        commands.entity(entity).insert(Started);
    }
}

fn clear(commands: &mut Commands, preview: &mut Preview) {
    for entity in preview.stage.drain(..) {
        commands.entity(entity).despawn();
    }
    (preview.picture, preview.shown) = (None, None);
}

/// Clears the view away when the menus are left.
pub fn put_away(mut commands: Commands, mut preview: ResMut<Preview>) {
    clear(&mut commands, &mut preview);
}
