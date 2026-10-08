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
//! The race's mascot stands in the frame in place of its own figure
//! (`SingleRaceSelectBase::SetPreviewDriver`): the driver `LEGORACE.RCB` names for
//! the race, or Veronica Voltage on the time race page, made of that driver's parts
//! (`PARTDB/DRIVERS.DDB`) on the frame's own bones, standing where the frame puts
//! its figure and playing the move the frame gives it, as `film` does for a film's
//! racer. The frame's ambient and directional lights light the models that have
//! normals (`lighting`), as a film's do; the rest keep the colours they were made
//! with. Every light of a frame lasts as long as the frame loops, and is taken as
//! lit throughout. The mascot's face keeps its default look, as the frame has no
//! tracks for it.

use std::collections::HashMap;

use super::{Art, Menu, Page};
use crate::assets::Jam;
use crate::assets::tokens::{Token, tokenize};
use crate::championship::Championship;
use crate::film::{Showing, entries, facing, number, text, vec3};
use crate::lighting::{Beam, Lights};
use crate::menu::{Circuits, Settings};
use crate::scenery::{self, Animated, PropDef};
use crate::{build, roster};
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
/// `RaceModeSetupScreen::UpdateRacePreview`: the time race page's mascot, whatever
/// the race.
const TIME_RACE_MASCOT: &str = "vv";
/// The model a frame has standing in for its mascot.
const MASCOT: &str = "guy1";
const DETAIL: f32 = 2.0;
const LAYER: usize = 9;
/// How much the picture is darkened to be read over (the original's overlay).
pub const DIM: f32 = 0.55;

#[derive(Resource, Default)]
pub struct Preview {
    /// The picture it is drawn onto.
    pub picture: Option<Handle<Image>>,
    /// Which frame is shown, and what it is made of and seen through.
    shown: Option<(String, String)>,
    stage: Vec<Entity>,
    /// Whether the scene's lights are the frame's.
    lit: bool,
}

/// A model of the frame, and the part of its animation it plays.
#[derive(Component)]
pub struct Cue(Option<usize>);

#[derive(Component)]
pub struct Started;

#[derive(Component)]
pub struct Staged;

/// The frame's theme name and the mascot's, for a race's folder:
/// `RaceNameEntry::GetThemeName` and `GetMascotName`.
fn theme(art: &Art, folder: &str) -> Option<(String, String)> {
    let tokens = tokenize(art.jam().get("/MENUDATA/LEGORACE.RCB")?);
    entries(&tokens, 0x27).into_iter().find_map(|(_, fields)| {
        (text(fields, 0x29)?.eq_ignore_ascii_case(folder)).then(|| {
            Some((text(fields, 0x2d)?, text(fields, 0x2e).unwrap_or_default()))
        })?
    })
}

/// The mascot's figure, made of the driver's parts on the bones of the frame's own
/// (`SetPreviewDriver`: the model of `BuildDriverModel` takes the frame's figure's
/// place), or `None` where the race names no driver or the driver has no parts.
fn mascot(jam: &Jam, name: &str, frame: &PropDef) -> Option<PropDef> {
    let cosmetics = roster::cosmetics_of(jam, name)?;
    let catalogue = build::Catalogue::open(jam)?;
    let figure = build::figure(jam, &catalogue, cosmetics, true)?;
    let (files, folders) = build::Catalogue::files();
    let parts = Library::new(jam, files.iter().map(String::as_str), &folders);
    let mut made = PropDef::made(MASCOT, &figure, frame.rig().cloned(), &parts);
    made.stand_in(frame);
    Some(made)
}

/// The lights a frame casts: its ambient light (the last it has) and its
/// directional ones, of nought to one.
fn lights(frame: &[Token]) -> Lights {
    let colour = |fields: &[Token]| vec3(fields, 0x38, 0).unwrap_or_default() / 255.0;
    Lights {
        ambient: entries(frame, 0x35).last().map(|(_, f)| colour(f)),
        beams: entries(frame, 0x3a)
            .iter()
            .map(|(_, f)| Beam {
                direction: vec3(f, 0x39, 0).unwrap_or(Vec3::NEG_Z),
                colour: colour(f),
            })
            .collect(),
    }
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
    (theme, driver): (&str, &str),
    commands: &mut Commands,
    (meshes, materials, images, binds): (
        &mut Assets<Mesh>,
        &mut Assets<StandardMaterial>,
        &mut Assets<Image>,
        &mut Assets<SkinnedMeshInverseBindposes>,
    ),
) -> Option<(Vec<Entity>, (Vec3, Vec3, Vec3, f32), Lights)> {
    let jam = art.jam();
    let tokens = tokenize(jam.get(FILE)?);
    let worlds = scenery::names(&tokens, 0x28);
    let frames = entries(&tokens, 0x27);
    let (_, frame) = frames.iter().find(|(name, _)| name == theme)?;
    // What the frame has, and for each the part of its animation it plays and
    // where the frame stands it, which is not where its world file does.
    let mut wanted: HashMap<String, Option<usize>> = HashMap::new();
    let mut places: HashMap<String, (Vec3, Quat)> = HashMap::new();
    for (_, fields) in entries(frame, 0x2e) {
        let Some(model) = text(fields, 0x30).or_else(|| text(fields, 0x2f)) else {
            continue;
        };
        let part = number(fields, 0x2d, 0)
            .filter(|part| *part >= 0.0 && text(fields, 0x30).is_some())
            .map(|part| part as usize);
        if let Some(position) = vec3(fields, 0x33, 0) {
            let turn = facing(
                vec3(fields, 0x34, 0).unwrap_or(Vec3::X),
                vec3(fields, 0x34, 3).unwrap_or(Vec3::Z),
            );
            places.insert(model.clone(), (position, turn));
        }
        wanted.insert(model, part);
    }
    let own: Vec<&str> = jam
        .list(DIR)
        .filter(|f| f.ends_with(".MDB") || f.ends_with(".TDB"))
        .collect();
    let mut library = Library::new(jam, own.iter().copied(), &[DIR]);
    library.plain();
    library.dynamic();
    let files: Vec<String> = worlds
        .iter()
        .map(|world| format!("{DIR}/{}.WDB", world.to_uppercase()))
        .collect();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    let mut props = scenery::load_files(jam, DIR, &files, &library, |_, model| {
        wanted.contains_key(model)
    });
    // `SetPreviewDriver`: with a driver named, the frame's figure gives way to theirs.
    if let Some(at) = props.iter().position(|prop| prop.name() == MASCOT) {
        if let Some(driver) = (!driver.is_empty()).then(|| mascot(jam, driver, &props[at])).flatten() {
            props[at] = driver;
        }
    }
    let mut made = Vec::new();
    for mut def in props {
        if let Some(&(position, turn)) = places.get(def.name()) {
            def.place(position, turn);
        }
        let part = wanted.get(def.name()).copied().flatten();
        let prop = scenery::spawn(def, commands, meshes, materials, images, binds);
        commands.entity(prop).insert((Staged, Cue(part)));
        made.push(prop);
    }
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
    Some((made, camera, lights(frame)))
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
    mut scene_lights: ResMut<Lights>,
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
    let mut theme = folder.and_then(|folder| theme(&art, &folder));
    if let (Page::TimeRace, Some(theme)) = (menu.page, &mut theme) {
        theme.1 = TIME_RACE_MASCOT.to_string();
    }
    if theme == preview.shown {
        return;
    }
    clear(&mut commands, &mut preview, &mut scene_lights);
    menu.drawn = false;
    let Some(theme) = theme else {
        return;
    };
    scenery::set_mirror(false);
    let Some((made, (eye, forward, up, fov), frame_lights)) = stage(
        &art,
        (&theme.0, &theme.1),
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
    *scene_lights = frame_lights;
    preview.lit = true;
}

/// Starts each model's animation, and has its camera alone draw it.
pub fn dress(
    mut commands: Commands,
    made: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    staged: Query<(), With<Staged>>,
    mut starting: Query<(Entity, &mut Animated, &Cue), (With<Staged>, Without<Started>)>,
) {
    for mesh in &made {
        if parents.iter_ancestors(mesh).any(|above| staged.contains(above)) {
            commands.entity(mesh).insert(RenderLayers::layer(LAYER));
        }
    }
    for (entity, mut animated, cue) in &mut starting {
        if let Some(part) = cue.0 {
            animated.play(part, true);
        }
        commands.entity(entity).insert(Started);
    }
}

fn clear(commands: &mut Commands, preview: &mut Preview, lights: &mut Lights) {
    if std::mem::take(&mut preview.lit) {
        *lights = Lights::default();
    }
    for entity in preview.stage.drain(..) {
        commands.entity(entity).despawn();
    }
    (preview.picture, preview.shown) = (None, None);
}

/// Clears the view away when the menus are left.
pub fn put_away(
    mut commands: Commands,
    mut preview: ResMut<Preview>,
    mut lights: ResMut<Lights>,
) {
    clear(&mut commands, &mut preview, &mut lights);
}


#[cfg(test)]
#[test]
fn each_race_has_its_mascot_and_the_time_race_has_veronica_voltage() {
    let Some(art) = super::load_art() else {
        return;
    };
    let jam = art.jam();
    // The races the tables list, with who stands in each one's frame.
    let (theme_1, mascot_1) = theme(&art, "racec0r1").unwrap();
    assert_eq!((theme_1.as_str(), mascot_1.as_str()), ("pirate1", "gb"));
    let all = roster::races(jam);
    assert!(!all.is_empty());
    let mut seen = std::collections::HashSet::new();
    for race in &all {
        let (_, who) = theme(&art, &race.folder).unwrap();
        // Every race names a driver, who has parts to be made of.
        assert!(roster::cosmetics_of(jam, &who).is_some(), "{} {who}", race.folder);
        seen.insert(who);
    }
    assert!(seen.len() >= 6, "{seen:?}");
    assert_ne!(
        roster::cosmetics_of(jam, "gb").unwrap().hat,
        roster::cosmetics_of(jam, TIME_RACE_MASCOT).unwrap().hat
    );
}
