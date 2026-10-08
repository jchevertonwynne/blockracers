//! The racer the garage shows: the minifigure standing on its pad, going through
//! its moves one after another, and beside it the racer's car on its own, in the
//! set the original has them in. After `RacerModelScreenBase` (`CreateModelSlots`:
//! where each is put; `RefreshSlotModel`: what each is made of; `AlignDriverSlots`
//! and `AlignCarSlots`: which way each faces; `PlayRandomAnimation` and `Update`:
//! which move the figure makes next, a different one each time the last has
//! played out), `RacerModelSlot` (the slots, neither of which turns, and the figure
//! of which moves) and `MenuFramedSceneView` over `MenuSceneView` (the showcase of
//! `GARAGE.MIB`, with the world `RS_SET/RACER.WDB` and its camera). The set's own
//! models (the ground, the pools of light, the pad and the shadows) are loaded as any
//! world file's are.
//!
//! The original draws it straight onto the screen. The port's menus are drawn over
//! everything else, so here it has a camera of its own that draws it onto a picture,
//! which the menu shows where the original's showcase is, as the main menu's figure is
//! (`mascot`), inside the showcase's frame.

use std::sync::Arc;

use super::{Art, Menu, mascot, workshop};
use crate::assets::{
    adb::Animation,
    gdb::parse_skeleton,
    lrs::Racer,
    tokens::{Token, tokenize},
};
use crate::garage::Garage;
use crate::menu::Settings;
use crate::scenery::{self, Animated, PropDef, Rig};
use crate::world::Library;
use crate::{build, physics::UNIT};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    mesh::skinning::SkinnedMeshInverseBindposes,
    prelude::*,
    render::render_resource::TextureFormat,
};

/// One of the build menu's sets: the garage's showcase, where a racer stands
/// beside its car, or the platform a driver is dressed on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Set {
    Showcase,
    Platform,
}

/// `RacerModelScreenBase::CreateModelSlots`: where the car is put in the showcase.
const CAR_AT: Vec3 = Vec3::new(-11.52, -6.767, 0.0);
/// `AlignCarSlots`: the car faces as the shadow it stands on (`crsdow` of
/// `BLENDED.WDB`) does at the start of its animation, which is as it is placed.
const CAR_FACING: Vec3 = Vec3::new(0.829_038, -0.559_193, 0.0);
/// The moves the platform's figure makes when its hat or its face is changed, when
/// its torso is, when its legs are (one or the other), and as the page is done
/// with (`EditDriverScreen::OnWidgetValueChanged`, `g_exitAnimTextIds`).
const HEAD_MOVE: usize = 0x83;
const TORSO_MOVE: usize = 0x84;
const LEG_MOVES: [usize; 2] = [0xd4, 0x85];

impl Set {
    /// The folder the set's files are in.
    fn dir(self) -> &'static str {
        match self {
            Set::Showcase => "/MENUDATA/RS_SET",
            Set::Platform => "/MENUDATA/CB_SET",
        }
    }

    /// Its world files: the one with its camera first.
    fn scenes(self) -> [String; 2] {
        let scene = match self {
            Set::Showcase => "RACER",
            Set::Platform => "CBSET",
        };
        [format!("{}/{scene}.WDB", self.dir()), format!("{}/BLENDED.WDB", self.dir())]
    }

    /// The animation its figure moves by.
    fn moves(self) -> &'static str {
        match self {
            Set::Showcase => "/MENUDATA/RSANIM.ADB",
            Set::Platform => "/MENUDATA/CBANIM.ADB",
        }
    }

    /// Where the figure stands (`RacerModelScreenBase::CreateModelSlots`,
    /// `EditDriverScreen::CreateDriverScene`), and how far round from facing along
    /// X it is turned (`AlignDriverSlots`; the platform's faces as it was made).
    fn standing(self) -> (Vec3, f32) {
        match self {
            Set::Showcase => (Vec3::new(-0.938, -0.898, 1.487), (-0.267_238f32).atan2(0.963_631)),
            Set::Platform => (Vec3::new(-5.359, -3.15, 0.026), 0.0),
        }
    }

    /// The moves the figure makes while nothing is asked of it, by the strings of
    /// `MENUNAME.SRF` that name them (`g_racerIdleAnimTextIds`, `g_idleAnimTextIds`).
    fn idle(self) -> &'static [usize] {
        match self {
            Set::Showcase => &[0x74, 0x75, 0x77, 0x78, 0x79, 0x7a, 0x7b],
            Set::Platform => &[0x74, 0x75, 0x76, 0xd3, 0x81, 0x82, 0x78, 0x79],
        }
    }

    /// Where on the menu's screen the set is shown: inside the border of its frame.
    pub fn area(self, art: &Art) -> Rect {
        self.frame(art).inflate(-super::BORDER)
    }

    /// The frame it is shown in (`showcase` of `GARAGE.MIB`, `platform` of
    /// `EDITDRVR.MIB`).
    pub fn frame(self, art: &Art) -> Rect {
        match self {
            Set::Showcase => art.place("garage", "showcase"),
            Set::Platform => art.place("editdrvr", "platform"),
        }
    }
}

/// The names of the figure's moves (`MENUNAME.SRF`), which the screens know them by.
fn names(art: &Art) -> Vec<String> {
    let table = art.jam().get("/MENUDATA/MENUNAME.SRF");
    table.map(crate::assets::font::load_strings).unwrap_or_default()
}

/// How many times finer than the menu's screen its picture is drawn.
const DETAIL: f32 = 3.0;
const LAYER: usize = 13;

/// The garage's racer on show, while there is one.
#[derive(Resource, Default)]
pub struct Stage {
    pub picture: Option<Handle<Image>>,
    /// Whose it is and which set they are on, which is what it is remade for.
    shown: Option<(Racer, Set)>,
    entities: Vec<Entity>,
    /// Which parts of the moves are the idle ones.
    idle: Vec<usize>,
    /// A move to make before the next idle one.
    first: Option<usize>,
}

impl Stage {
    /// The set on show.
    pub fn set(&self) -> Option<Set> {
        self.shown.as_ref().map(|shown| shown.1)
    }
}

/// What the showcase's camera draws: the set and the racer in it.
#[derive(Component)]
pub struct Staged;

/// The minifigure.
#[derive(Component)]
pub struct Figure;

/// The names of a `.ADB` file's parts, in order.
fn parts(data: &[u8]) -> Vec<String> {
    let tokens = tokenize(data);
    tokens
        .windows(3)
        .filter_map(|w| match (&w[0], &w[1], &w[2]) {
            (Token::Key(0x2c), Token::Str(name), Token::LCurly) => Some(name.to_lowercase()),
            _ => None,
        })
        .collect()
}

/// The figure of a racer on its skeleton with the set's moves, where it stands there.
fn figure(art: &Art, racer: &Racer, set: Set) -> Option<PropDef> {
    let jam = art.jam();
    let catalogue = build::Catalogue::open(jam)?;
    let model = build::figure(jam, &catalogue, racer.cosmetics, true)?;
    let (files, folders) = build::Catalogue::files();
    let library = Library::new(jam, files.iter().map(String::as_str), &folders);
    let rig = Rig {
        bones: Arc::new(parse_skeleton(build::skeleton(jam, &catalogue, racer.cosmetics, true)?)?),
        animation: Arc::new(Animation::parse(jam.get(set.moves())?)?),
    };
    let mut made = PropDef::made("racer", &model, Some(rig), &library);
    // Facing along X is facing as it was made; it is turned about Z from there.
    let (at, turn) = set.standing();
    made.moved(Quat::from_rotation_z(turn), at, 1.0);
    Some(made)
}

/// The set's camera (`Camera01`, of the cameras the world file lists), which comes after its models.
fn camera(scene: &[Token]) -> Option<(Vec3, Vec3, Vec3, f32)> {
    let at = scene.iter().position(|token| *token == Token::Key(0x43))?;
    mascot::camera(&scene[at..])
}

fn clear(commands: &mut Commands, stage: &mut Stage) {
    for entity in stage.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    (stage.picture, stage.shown) = (None, None);
}

/// Keeps the racer the garage's page shows on its stage, and nowhere else.
pub fn keep(
    mut commands: Commands,
    art: Res<Art>,
    garage: Res<Garage>,
    settings: Res<Settings>,
    bench: Res<workshop::Bench>,
    mut menu: ResMut<Menu>,
    mut stage: ResMut<Stage>,
    (mut meshes, mut materials, mut images, mut binds): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    let Some((racer, platform)) = workshop::showcased(menu.page, &bench, &garage, &settings)
    else {
        if !stage.entities.is_empty() {
            clear(&mut commands, &mut stage);
        }
        return;
    };
    let set = if platform { Set::Platform } else { Set::Showcase };
    let wanted = (racer, set);
    if stage.shown.as_ref() == Some(&wanted) {
        return;
    }
    // `EditDriverScreen::OnWidgetValueChanged`: a driver whose part is changed
    // where it stands makes a move about it.
    let names = names(&art);
    let moves = parts(art.jam().get(set.moves()).unwrap_or_default());
    let part = |string: usize| {
        let name = names.get(string)?.to_lowercase();
        moves.iter().position(|m| *m == name)
    };
    let before = stage.shown.as_ref().filter(|shown| shown.1 == Set::Platform && platform);
    let first = before.and_then(|(was, _)| {
        let (was, now) = (was.cosmetics, wanted.0.cosmetics);
        if (was.hat, was.face) != (now.hat, now.face) {
            part(HEAD_MOVE)
        } else if was.torso != now.torso {
            part(TORSO_MOVE)
        } else if was.legs != now.legs {
            part(LEG_MOVES[(now.legs % 2) as usize])
        } else {
            None
        }
    });
    let (racer, _) = wanted.clone();
    clear(&mut commands, &mut stage);
    let jam = art.jam();
    // A race run mirrored leaves the world mirrored; the menu's is as it was made.
    scenery::set_mirror(false);
    let scenes = set.scenes();
    let scene = tokenize(jam.get(&scenes[0]).unwrap_or_default());
    let (Some(figure), Some((eye, forward, up, fov))) = (figure(&art, &racer, set), camera(&scene))
    else {
        return;
    };
    let mut entities = Vec::new();
    // The set: the ground, the pools of light, the pad and the shadows.
    let mut own: Vec<&str> = jam
        .list(set.dir())
        .filter(|file| file.ends_with(".MDB") || file.ends_with(".TDB"))
        .collect();
    own.sort();
    let library = Library::new(jam, own.iter().copied(), &[set.dir()]);
    let files = [scenes[0].as_str(), scenes[1].as_str()];
    for def in scenery::load_files(jam, set.dir(), &files, &library, |_, _| true) {
        let prop = scenery::spawn(def, &mut commands, &mut meshes, &mut materials, &mut images, &mut binds);
        commands.entity(prop).insert(Staged);
        entities.push(prop);
    }
    // The figure, which begins on a move of its own.
    let figure = scenery::spawn(figure, &mut commands, &mut meshes, &mut materials, &mut images, &mut binds);
    commands.entity(figure).insert((Staged, Figure));
    entities.push(figure);
    // The car, on the ground and facing as `AlignCarSlots` has it. The platform has none.
    let car = (set == Set::Showcase).then(|| crate::world::load_built(jam, &racer, true));
    if let Some(mut model) = car.flatten() {
        // The driver stands beside it.
        model.driver.clear();
        let way = scenery::to_world(CAR_FACING).normalize_or_zero();
        let car = commands
            .spawn((
                Staged,
                Transform::from_translation(scenery::to_world(CAR_AT) + Vec3::Y * crate::physics::RIDE_HEIGHT)
                    .looking_to(way, Vec3::Y),
                Visibility::default(),
            ))
            .id();
        crate::time_race::dress(&mut commands, car, model, &mut meshes, &mut materials, &mut images, None);
        entities.push(car);
    }
    // `MenuSceneView::SetupCamera`: the set's own camera, drawn onto a picture the
    // size of the showcase.
    let area = set.area(&art);
    let size = (area.size() * DETAIL).max(Vec2::ONE).as_uvec2();
    let picture = images.add(Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None));
    let way = |v: Vec3| scenery::to_world(v).normalize_or_zero();
    let lens = PerspectiveProjection {
        fov: fov.to_radians(),
        near: 5.0 * UNIT,
        far: 800.0 * UNIT,
        ..default()
    };
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -6,
                clear_color: ClearColorConfig::Custom(super::BLUE_FILL),
                ..default()
            },
            Projection::Perspective(lens),
            RenderTarget::from(picture.clone()),
            RenderLayers::layer(LAYER),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Transform::from_translation(scenery::to_world(eye)).looking_to(way(forward), way(up)),
        ))
        .id();
    entities.push(camera);
    *stage = Stage {
        picture: Some(picture),
        shown: Some(wanted),
        entities,
        idle: set.idle().iter().filter_map(|&string| part(string)).collect(),
        first,
    };
    menu.drawn = false;
}

/// `RacerModelScreenBase::Update`: when a move has played out the figure makes
/// another, never the same twice running.
pub fn idle(
    mut stage: ResMut<Stage>,
    time: Res<Time<Real>>,
    mut figures: Query<&mut Animated, With<Figure>>,
    mut chance: Local<u32>,
) {
    let Ok(mut animated) = figures.single_mut() else {
        return;
    };
    if let Some(part) = stage.first.take() {
        animated.play(part, false);
        return;
    }
    if stage.idle.is_empty() || (animated.playing && stage.idle.contains(&animated.part)) {
        return;
    }
    // Which is a random pick from the seven.
    *chance = chance
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223)
        .wrapping_add(time.elapsed().subsec_nanos());
    let mut part = stage.idle[(*chance >> 8) as usize % stage.idle.len()];
    if part == animated.part && stage.idle.len() > 1 {
        let at = stage.idle.iter().position(|&p| p == part).unwrap_or(0);
        part = stage.idle[(at + 1) % stage.idle.len()];
    }
    animated.play(part, false);
}

/// Has the stage's camera, and no other, draw the meshes it is made of.
pub fn dress(
    mut commands: Commands,
    made: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    staged: Query<(), With<Staged>>,
) {
    for mesh in &made {
        if parents.iter_ancestors(mesh).any(|above| staged.contains(above)) {
            commands.entity(mesh).insert(RenderLayers::layer(LAYER));
        }
    }
}

/// Clears the stage away when the menus are left.
pub fn put_away(mut commands: Commands, mut stage: ResMut<Stage>) {
    clear(&mut commands, &mut stage);
}

#[cfg(test)]
#[test]
fn each_set_has_its_figure_s_moves_and_a_camera() {
    let Some(art) = super::load_art() else {
        return;
    };
    let names = names(&art);
    let racer = art_racer(&art);
    for set in [Set::Showcase, Set::Platform] {
        let moves = parts(art.jam().get(set.moves()).unwrap());
        let known = |string: &usize| moves.contains(&names[*string].to_lowercase());
        assert!(set.idle().iter().all(known), "{set:?}");
        let scene = tokenize(art.jam().get(&set.scenes()[0]).unwrap());
        let (eye, forward, _, fov) = camera(&scene).unwrap();
        // It looks at what stands there, which is far down its line of sight.
        let stood = if set == Set::Showcase { CAR_AT } else { set.standing().0 };
        let to = scenery::to_world(stood) - scenery::to_world(eye);
        assert!(to.normalize().dot(scenery::to_world(forward).normalize()) > 0.95, "{set:?}");
        assert_eq!(fov.round(), if set == Set::Showcase { 32.0 } else { 36.0 });
        assert!(figure(&art, &racer, set).is_some());
    }
    // The driver being dressed has a move for each part that is changed.
    let moves = parts(art.jam().get(Set::Platform.moves()).unwrap());
    for string in [HEAD_MOVE, TORSO_MOVE, LEG_MOVES[0], LEG_MOVES[1]] {
        assert!(moves.contains(&names[string].to_lowercase()), "{string}");
    }
}

#[cfg(test)]
fn art_racer(art: &Art) -> Racer {
    crate::garage::stock(art.jam()).into_iter().next().unwrap_or_default()
}
