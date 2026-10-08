//! The picture shown while a race loads, after `LoadingScreen` (`race/loadingscreen.cpp`):
//! the race folder's `LOADSCRN.LSB` names a picture of the folder's (`rkr.bmp`, `tt.bmp`),
//! a line of `GAME.SRF` written across the top of it, and the places for a row of dots;
//! `LOADSCRN.IDB` and `TICK.BMP` are the tick drawn at as many of the places as the load
//! is of the way through.
//!
//! The picture is drawn for a few frames, and then the race is loaded behind it a
//! step to a frame, the ticks filling in as the steps are done. The original draws
//! the screen between the parts of `RaceSession`'s load (`DrawLoadProgress`), each
//! with how far on it is; the port loads in fewer parts and another order, so each
//! of its steps has the original's figure for the part most like it (`STEPS`).
//! (`BRICK_LOADING=<seconds>`, for demos, holds the picture for that long on the real
//! clock with the ticks coming one by one, to be looked at, and loads the race after;
//! `BRICK_LOADING=0` shows a demo the load as it is; without it a demo goes straight
//! on to its race.)

use crate::assets::{
    Jam,
    font::{load_fonts, load_strings},
    image::{Pixels, decode_bmp},
    tokens::{Token, tokenize},
};
use crate::menu::{Circuits, Screen, Settings};
use bevy::{
    asset::RenderAssetUsages,
    ecs::schedule::ScheduleLabel,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// The screen the original lays the picture out for.
const SIZE: Vec2 = Vec2::new(640.0, 480.0);
/// A tick is drawn this far each way from its place (`LoadingScreen::Draw`).
const TICK: f32 = 8.0;
/// Where the words are, from the top (`LoadingScreen::Draw`).
const TEXT_TOP: f32 = 25.0;
/// How many frames the picture is up before the race is loaded behind it.
const FRAMES: u32 = 3;

/// One of the steps a race is loaded in: a schedule of what the step does.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Step(pub usize);

/// How far through the load the end of each step is, by `RaceSession`'s calls of
/// `DrawLoadProgress`: the circuit's files read (0.42, its databases), the circuit
/// put in the world (0.6), the sky (0.75), the cars (0.8, the last of what is made
/// for them), the bricks and power-ups (0.97) and the display (1).
pub const STEPS: [f32; 6] = [0.42, 0.6, 0.75, 0.8, 0.97, 1.0];

/// How many of the steps have been taken for the race about to begin.
#[derive(Resource, Default)]
pub struct Done(usize);

pub fn plugin(app: &mut App) {
    app.init_resource::<Done>()
        .add_systems(OnEnter(Screen::Loading), show)
        .add_systems(OnEnter(Screen::Race), finish)
        .add_systems(OnEnter(Screen::Menu), abandon)
        .add_systems(Update, go_on.run_if(in_state(Screen::Loading)));
}

/// The steps the loading screen didn't take, all at once: a race come to without
/// the screen, or after a demo's hold.
fn finish(world: &mut World) {
    let done = std::mem::take(&mut world.resource_mut::<Done>().0);
    for step in done..STEPS.len() {
        let _ = world.try_run_schedule(Step(step));
    }
}

/// A load given up for the menu, as a session's can be: what it had put in the
/// world goes, as it would have when its race was left.
fn abandon(
    mut commands: Commands,
    mut done: ResMut<Done>,
    loaded: Query<(Entity, &DespawnOnExit<Screen>)>,
) {
    if std::mem::take(&mut done.0) > 0 {
        for (entity, _) in loaded.iter().filter(|l| l.1.0 == Screen::Race) {
            commands.entity(entity).despawn();
        }
    }
}

/// What is on the screen, and how long it is to stay.
#[derive(Resource)]
pub struct Up {
    frames: u32,
    /// A demo's hold, in real seconds, and when it began.
    hold: Option<f32>,
    began: Option<f32>,
    ticks: Vec<Entity>,
    /// The last tick has been drawn.
    full: bool,
}

/// What a race folder's `LOADSCRN.LSB` says.
struct Sheet {
    picture: String,
    string: usize,
    /// Where the ticks go, as fractions of the screen.
    dots: Vec<Vec2>,
}

fn sheet(data: &[u8]) -> Option<Sheet> {
    let tokens = tokenize(data);
    let mut out = Sheet {
        picture: String::new(),
        string: 0,
        dots: Vec::new(),
    };
    for at in 0..tokens.len() {
        match (&tokens[at], tokens.get(at + 1)) {
            (Token::Key(0x28), Some(Token::Str(name))) => out.picture = name.clone(),
            (Token::Key(0x2a), Some(Token::Int(index))) => out.string = *index as usize,
            (Token::Key(0x29), Some(Token::LBracket)) => {
                let numbers: Vec<f32> = tokens[at + 2..]
                    .iter()
                    .skip_while(|t| **t != Token::LCurly)
                    .skip(1)
                    .take_while(|t| **t != Token::RCurly)
                    .filter_map(|t| match t {
                        Token::Float(v) => Some(*v),
                        Token::Int(v) => Some(*v as f32),
                        _ => None,
                    })
                    .collect();
                out.dots = numbers
                    .chunks_exact(2)
                    .map(|pair| Vec2::new(pair[0], pair[1]))
                    .collect();
            }
            _ => {}
        }
    }
    (!out.picture.is_empty()).then_some(out)
}

fn image(pixels: Pixels, images: &mut Assets<Image>) -> (Handle<Image>, Vec2) {
    let size = Vec2::new(pixels.width as f32, pixels.height as f32);
    let mut image = Image::new(
        Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    (images.add(image), size)
}

/// A place and size given in the original's 640 by 480, as the fraction of the
/// window that is.
fn node(at: Vec2, size: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(at.x / SIZE.x * 100.0),
        top: Val::Percent(at.y / SIZE.y * 100.0),
        width: Val::Percent(size.x / SIZE.x * 100.0),
        height: Val::Percent(size.y / SIZE.y * 100.0),
        ..default()
    }
}

fn show(
    mut commands: Commands,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
    demo: Option<Res<crate::DemoShot>>,
    mut images: ResMut<Assets<Image>>,
    mut next: ResMut<NextState<Screen>>,
) {
    let hold = std::env::var("BRICK_LOADING")
        .ok()
        .and_then(|seconds| seconds.parse::<f32>().ok());
    // A demo goes straight on to its race; one that asks for no hold is shown the
    // load itself.
    let shown = hold.is_some();
    let hold = hold.filter(|seconds| *seconds > 0.0);
    if demo.is_some() && !shown {
        commands.remove_resource::<Up>();
        next.set(Screen::Race);
        return;
    }
    let drawn = (|| {
        let race = circuits.0.get(settings.circuit)?.race.as_deref()?;
        let path = std::env::var("BRICK_JAM")
            .unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into());
        let jam = Jam::open(path)?;
        let dir = format!("/GAMEDATA/{race}");
        let sheet = sheet(jam.get(&format!("{dir}/LOADSCRN.LSB"))?)?;
        let bmp = format!("{dir}/{}.BMP", sheet.picture.to_uppercase());
        let (picture, _) = image(decode_bmp(jam.get(&bmp)?, None)?, &mut images);
        // The tick's colour key is in `LOADSCRN.IDB`: magenta.
        let key = jam.get(&format!("{dir}/LOADSCRN.IDB")).and_then(|data| {
            let tokens = tokenize(data);
            let at = tokens.iter().position(|t| *t == Token::Key(0x2b))?;
            match tokens.get(at + 1..at + 4)? {
                [Token::Int(r), Token::Int(g), Token::Int(b)] => {
                    Some([*r as u8, *g as u8, *b as u8])
                }
                _ => None,
            }
        });
        let tick = decode_bmp(
            jam.get(&format!("{dir}/TICK.BMP"))?,
            key.or(Some([255, 0, 255])),
        )?;
        let (tick, _) = image(tick, &mut images);
        let language = format!(
            "/GAMEDATA/COMMON/{}",
            crate::assets::font::language_folder(settings.language)
        );
        let strings = load_strings(jam.get(&format!("{language}/GAME.SRF"))?);
        let fonts = load_fonts(&jam, &language, "LEGOFNTS.FDB");
        let words = fonts
            .get("font_ths")?
            .render(strings.get(sheet.string)?, false);
        let (words, words_size) = image(words, &mut images);
        Some((sheet, picture, tick, words, words_size))
    })();
    let Some((sheet, picture, tick, words, words_size)) = drawn else {
        commands.remove_resource::<Up>();
        next.set(Screen::Race);
        return;
    };
    let mut ticks = Vec::new();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(1000),
            DespawnOnExit(Screen::Loading),
        ))
        .with_children(|root| {
            root.spawn((node(Vec2::ZERO, SIZE), ImageNode::new(picture)));
            root.spawn((
                node(
                    Vec2::new((SIZE.x - words_size.x).floor() / 2.0, TEXT_TOP),
                    words_size,
                ),
                ImageNode::new(words),
            ));
            for dot in &sheet.dots {
                let at = *dot * SIZE - Vec2::splat(TICK);
                ticks.push(
                    root.spawn((
                        node(at, Vec2::splat(TICK * 2.0)),
                        ImageNode::new(tick.clone()),
                        Visibility::Hidden,
                    ))
                    .id(),
                );
            }
        });
    commands.insert_resource(Up {
        frames: 0,
        hold,
        began: None,
        ticks,
        full: false,
    });
}

/// Draws the ticks for how far on the load is (`LoadingScreen::SetProgress`), takes
/// the load's next step, and goes on to the race when the last is done and drawn.
pub fn go_on(
    mut commands: Commands,
    up: Option<ResMut<Up>>,
    mut done: ResMut<Done>,
    real: Res<Time<Real>>,
    mut visibility: Query<&mut Visibility>,
    mut next: ResMut<NextState<Screen>>,
) {
    let Some(mut up) = up else { return };
    up.frames += 1;
    let now = real.elapsed_secs();
    let began = *up.began.get_or_insert(now);
    let progress = match up.hold {
        Some(hold) => ((now - began) / hold.max(0.01)).min(1.0),
        None => done.0.checked_sub(1).map_or(0.0, |last| STEPS[last]),
    };
    // `LoadingScreen::Draw`: as many ticks as the places' count times the progress.
    let count = (up.ticks.len() as f32 * progress) as usize;
    for (n, tick) in up.ticks.iter().enumerate() {
        if let Ok(mut visibility) = visibility.get_mut(*tick) {
            *visibility = if n < count {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    let over = match up.hold {
        Some(hold) => now - began >= hold,
        None if done.0 == STEPS.len() => std::mem::replace(&mut up.full, true),
        None => {
            if up.frames >= FRAMES {
                debug!("step {} of the load", done.0);
                commands.run_schedule(Step(done.0));
                done.0 += 1;
            }
            false
        }
    };
    if over {
        next.set(Screen::Race);
    }
}
