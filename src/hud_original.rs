//! The original game's race display (`RaceHud`), drawn with its own fonts, pictures
//! and words: place and lap times along the top, the held power-up bottom left, a map
//! of the circuit or a speedometer bottom right, and the countdown and finish banners.

use crate::assets::{
    Jam,
    font::{Font, load_fonts, load_strings},
    image::{Pixels, decode_bmp},
    materials,
    tokens::{Token, tokenize},
};
use crate::items::Power;
use crate::kart::{Kart, Player};
use crate::menu::{Circuits, Settings};
use crate::physics::UNIT;
use crate::{Phase, Race};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};
use std::collections::HashMap;

const COMMON: &str = "/GAMEDATA/COMMON";
/// The original lays the display out for a screen this tall and scales it to fit.
const HEIGHT: f32 = 480.0;
/// Text sits this far from the top.
const TOP: f32 = 7.0;
const MAP_SIZE: f32 = 128.0;
/// A warp's wash of blue comes and goes over this long, and is this blue.
const WARP_WASH: f32 = 0.2;
const WARP_BLUE: f32 = 100.0 / 255.0;
/// The map and the speedometer keep this far from the corner.
const MAP_INSET: f32 = HEIGHT / 32.0;
const MARKER: f32 = 16.0;
/// The side of the square the player's arrow is drawn in.
const ARROW_SPAN: f32 = 20.0;
/// A change of place makes the number swell to twice its size and back.
const PULSE: (f32, f32) = (0.175, 0.35);
/// After a lap its time is held up, blinking, for this long.
const LAP_HOLD: f32 = 3.0;
const BLINK: f32 = 0.25;
/// The speedometer's needle: how far it turns for full speed, and where it rests.
const NEEDLE_SWEEP: f32 = 3.4;
const NEEDLE_REST: f32 = 2.25;
const NEEDLE_LENGTH: f32 = 45.0;
/// Top speed, for the needle, in the original's units a millisecond.
const NEEDLE_FULL_SPEED: f32 = 0.19;

// Pictures of `LEGOIMGS.IDB`, in order.
const PICTURES: [&str; 24] = [
    "speed", "dot", "cannon", "grapple", "wand", "rocket", "oilcan", "barrel", "magnet", "curse", "igdcrdot", "holder",
    "holderwp", "oneenh", "twoenh", "threeenh", "shld1", "shld2", "shld3", "shld4", "octan1", "octan2", "octan3", "octan4",
];
const HOLDER: usize = 11;
const HOLDER_TINT: usize = 12;

// Strings of `GAME.SRF`.
mod text {
    /// ST, ND, RD and TH from here.
    pub const PLACES: usize = 25;
    pub const WRONG_WAY: usize = 36;
    pub const BEST: usize = 37;
    pub const LAP: usize = 39;
    pub const FINISH: usize = 40;
    pub const GO: usize = 41;
    pub const CONTINUE: usize = 14;
    /// Each a pair of lines.
    pub const RECORD_STANDS: usize = 21;
    pub const RECORD_BEATEN: usize = 23;
    pub const RESTART: usize = 15;
    pub const EXIT: usize = 17;
}

struct Face {
    font: Font,
    image: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
    /// Which piece of the layout each character is.
    pieces: HashMap<char, usize>,
}

/// Everything the display is drawn with.
#[derive(Resource)]
pub struct Art {
    faces: HashMap<String, Face>,
    strings: Vec<String>,
    pictures: Vec<Option<(Handle<Image>, Vec2)>>,
    /// The picture of the circuit and the part of the world it covers:
    /// least and greatest X, then greatest and least Y, in the original's units.
    map: Option<(Handle<Image>, [f32; 4])>,
    white: Handle<Image>,
    /// The player's mark on the map, pointing right.
    arrow: Handle<Image>,
}

/// What the display remembers from frame to frame.
#[derive(Resource, Default)]
pub struct State {
    lap: i32,
    lap_started: f32,
    last_lap: Option<f32>,
    best_lap: Option<f32>,
    place: usize,
    pulse: Option<f32>,
    speed: f32,
    /// 0 the map, 1 the speedometer, 2 neither.
    gadget: u8,
    /// How much of a warp's blue is over the screen.
    wash: f32,
}

#[derive(Component)]
pub struct Root;

fn image(pixels: Pixels, images: &mut Assets<Image>) -> (Handle<Image>, Vec2) {
    let size = Vec2::new(pixels.width as f32, pixels.height as f32);
    let extent = Extent3d { width: pixels.width, height: pixels.height, depth_or_array_layers: 1 };
    let mut image =
        Image::new(extent, TextureDimension::D2, pixels.rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    image.sampler = ImageSampler::nearest();
    (images.add(image), size)
}

/// Loads the display's art for the circuit about to be raced.
pub fn load(
    mut commands: Commands,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
    mut images: ResMut<Assets<Image>>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    commands.insert_resource(State::default());
    commands.remove_resource::<Art>();
    let path = std::env::var("BRICK_JAM").unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into());
    let Some(jam) = Jam::open(path) else { return };
    let language = format!("{COMMON}/ENGLISH");
    let strings = jam.get(&format!("{language}/GAME.SRF")).map(load_strings).unwrap_or_default();
    let mut faces = HashMap::new();
    for (name, font) in load_fonts(&jam, &language, "LEGOFNTS.FDB") {
        // One piece of the picture per character.
        let pixels = font.pixels();
        let mut layout = TextureAtlasLayout::new_empty(UVec2::new(pixels.width, pixels.height));
        let mut pieces = HashMap::new();
        for code in 1..=0xff_u32 {
            let Some(character) = char::from_u32(code) else { continue };
            if let Some((x, width)) = font.glyph(character) {
                pieces.insert(character, layout.add_texture(URect::new(x, 0, x + width, pixels.height)));
            }
        }
        let copy = Pixels { width: pixels.width, height: pixels.height, rgba: pixels.rgba.clone() };
        let face = Face { image: image(copy, &mut images).0, layout: layouts.add(layout), pieces, font };
        faces.insert(name, face);
    }
    if strings.len() < 44 || !faces.contains_key("font_ths") || !faces.contains_key("ignum") {
        return;
    }
    let black = Some([0, 0, 0]);
    let picture = |name: &str, images: &mut Assets<Image>| {
        Some(image(decode_bmp(jam.get(&format!("{COMMON}/{name}.BMP"))?, black)?, images))
    };
    let pictures = PICTURES.iter().map(|name| picture(name, &mut images)).collect();

    // The circuit's map, and the bounds its race definition gives for it.
    let map = circuits.0[settings.circuit].race.as_deref().and_then(|race| {
        let dir = format!("/GAMEDATA/{race}");
        let key = jam.get(&format!("{dir}/IGD_MAP.TDB")).and_then(|d| materials::parse_tdb(d).into_values().next());
        let pixels = decode_bmp(jam.get(&format!("{dir}/IGD_MAP.BMP"))?, key.and_then(|t| t.color_key).or(black))?;
        let tokens = tokenize(jam.get(&format!("{dir}/{race}.RAB"))?);
        let at = tokens.iter().position(|t| *t == Token::Key(0x46))?;
        let number = |n: usize| match tokens.get(at + 1 + n) {
            Some(Token::Float(v)) => *v,
            Some(Token::Int(v)) => *v as f32,
            _ => 0.0,
        };
        Some((image(pixels, &mut images).0, [number(0), number(1), number(2), number(3)]))
    });
    let white = image(Pixels { width: 1, height: 1, rgba: vec![255; 4] }, &mut images).0;
    let arrow = image(arrow(), &mut images).0;
    commands.insert_resource(Art { faces, strings, pictures, map, white, arrow });
}

/// The arrow `RaceHud::DrawMapArrow` draws for the player: a green triangle inside a
/// black one, here pointing right and drawn large so that it turns smoothly.
fn arrow() -> Pixels {
    const SIZE: usize = 96;
    let per_unit = SIZE as f32 / ARROW_SPAN;
    // Tip ahead of the middle, base behind it, and half the base's width.
    let inside = |at: Vec2, [tip, base, half]: [f32; 3]| {
        let along = (at.x + base) / (tip + base);
        (0.0..=1.0).contains(&along) && at.y.abs() <= half * (1.0 - along)
    };
    let mut rgba = vec![0; SIZE * SIZE * 4];
    for (i, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let at = (Vec2::new((i % SIZE) as f32, (i / SIZE) as f32) + 0.5) / per_unit - ARROW_SPAN / 2.0;
        if inside(at, [8.0, 5.0, 4.0]) {
            pixel.copy_from_slice(&[0, 255, 0, 255]);
        } else if inside(at, [9.5, 6.5, 5.5]) {
            pixel.copy_from_slice(&[0, 0, 0, 255]);
        }
    }
    Pixels { width: SIZE as u32, height: SIZE as u32, rgba }
}

/// Minutes, seconds and hundredths, as `RaceHud::FormatTime` writes them.
fn clock(seconds: f32) -> String {
    let hundredths = (seconds.max(0.0) * 100.0) as u32;
    format!("{}:{:02}:{:02}", hundredths / 6000 % 60, hundredths / 100 % 60, hundredths % 100)
}

/// The pieces of one frame of the display, on a screen `HEIGHT` tall.
struct Frame<'a> {
    art: &'a Art,
    nodes: Vec<(Node, ImageNode, UiTransform)>,
}

fn place(at: Vec2, size: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(at.x),
        top: Val::Px(at.y),
        width: Val::Px(size.x),
        height: Val::Px(size.y),
        ..default()
    }
}

impl Frame<'_> {
    fn width(&self, face: &str, words: &str, scale: f32) -> f32 {
        self.art.faces.get(face).map_or(0.0, |f| f.font.measure(words) * scale)
    }

    /// Writes `words` with their top left at `at`.
    fn write(&mut self, face: &str, words: &str, at: Vec2, scale: f32, colour: Color) {
        let Some(face) = self.art.faces.get(face) else { return };
        let height = face.font.height() as f32 * scale;
        let mut x = at.x;
        for character in words.to_uppercase().chars() {
            if let (Some(&piece), Some((_, width))) = (face.pieces.get(&character), face.font.glyph(character)) {
                let atlas = TextureAtlas { layout: face.layout.clone(), index: piece };
                let mut glyph = ImageNode::from_atlas_image(face.image.clone(), atlas);
                (glyph.color, glyph.image_mode) = (colour, NodeImageMode::Stretch);
                self.nodes.push((place(Vec2::new(x, at.y), Vec2::new(width as f32 * scale, height)), glyph, UiTransform::IDENTITY));
            }
            x += face.font.advance(character) as f32 * scale;
        }
    }

    /// The big face where it has the letters, and the small one at twice the size
    /// where it hasn't; centred on `centre`.
    fn banner(&mut self, words: &str, centre: Vec2, scale: f32, colour: Color) {
        let big = self.art.faces.get("ignum").is_some_and(|f| words.chars().all(|c| c == ' ' || f.pieces.contains_key(&c)));
        let (face, scale) = if big { ("ignum", scale) } else { ("font_ths", scale * 2.0) };
        let height = self.art.faces.get(face).map_or(0.0, |f| f.font.height() as f32) * scale;
        let at = centre - Vec2::new(self.width(face, words, scale), height) / 2.0;
        self.write(face, words, at, scale, colour);
    }

    fn picture(&mut self, index: usize, at: Vec2, scale: f32, colour: Color) -> Vec2 {
        let Some((handle, size)) = self.art.pictures.get(index).cloned().flatten() else { return Vec2::ZERO };
        let node = ImageNode { image: handle, color: colour, image_mode: NodeImageMode::Stretch, ..default() };
        self.nodes.push((place(at, size * scale), node, UiTransform::IDENTITY));
        size * scale
    }

    fn block(&mut self, at: Vec2, size: Vec2, colour: Color) {
        let node = ImageNode { image: self.art.white.clone(), color: colour, image_mode: NodeImageMode::Stretch, ..default() };
        self.nodes.push((place(at, size), node, UiTransform::IDENTITY));
    }
}

pub fn draw(
    mut commands: Commands,
    time: Res<Time>,
    real: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    race: Res<Race>,
    settings: Res<Settings>,
    art: Res<Art>,
    mut state: ResMut<State>,
    mut scale: ResMut<UiScale>,
    window: Single<&Window, With<PrimaryWindow>>,
    karts: Query<(&Kart, Has<Player>)>,
    roots: Query<Entity, With<Root>>,
    pause: Res<crate::Pause>,
    championship: Res<crate::championship::Championship>,
    time_race: Res<crate::time_race::TimeRace>,
    (variant, replay): (Res<crate::variant::Variant>, Res<crate::replay::Replay>),
) {
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some((player, _)) = karts.iter().find(|k| k.1) else { return };
    let fit = window.height() / HEIGHT;
    if scale.0 != fit {
        scale.0 = fit;
    }
    let width = window.width() / fit;
    let dt = time.delta_secs();
    let state = &mut *state;
    if keys.just_pressed(KeyCode::Tab) {
        state.gadget = (state.gadget + 1) % 3;
    }
    let mut frame = Frame { art: &art, nodes: Vec::new() };
    let string = |index: usize| art.strings.get(index).cloned().unwrap_or_default();
    let line = art.faces["font_ths"].font.height() as f32;

    // The banner: three, two, one, GO, and FINISH at the end.
    let banner_at = Vec2::new(width / 2.0, TOP + line * 21.0 / 8.0 + line / 2.0);
    let yellow = Color::srgb(1.0, 1.0, 0.0);
    match race.phase {
        Phase::Countdown => {
            // Each number starts big and shrinks through its second.
            let left = race.countdown.max(0.0);
            let swell = 1.0 + left.fract();
            frame.banner(&format!("{}", left.ceil() as u32), banner_at, swell, yellow);
        }
        Phase::Racing if race.time < 2.0 => frame.banner(&string(text::GO), banner_at, 1.8, yellow),
        Phase::Finished if replay.showing.is_some() => frame.banner("REPLAY", banner_at, 1.0, Color::WHITE),
        Phase::Finished => frame.banner(&string(text::FINISH), banner_at, 1.0, Color::WHITE),
        _ => {}
    }
    if race.phase == Phase::Intro {
        *state = State { gadget: state.gadget, ..default() };
    }

    // Laps: the time of the one under way, and the best so far.
    let laps = settings.laps();
    let racing_time = player.finished.unwrap_or(race.time);
    if player.lap != state.lap {
        if state.lap >= 1 && player.lap > state.lap {
            let taken = racing_time - state.lap_started;
            state.last_lap = Some(taken);
            state.best_lap = Some(state.best_lap.map_or(taken, |best| best.min(taken)));
        }
        (state.lap, state.lap_started) = (player.lap, racing_time);
    }
    let on_lap = racing_time - state.lap_started;
    // A finished lap's time is held up for a moment, blinking.
    let held = state.last_lap.filter(|_| on_lap < LAP_HOLD || player.finished.is_some());
    let shown = held.unwrap_or(on_lap);
    let blink = held.is_some() && (on_lap / BLINK) as u32 % 2 == 1 && player.finished.is_none();
    let time_width = frame.width("font_ths", "0:00:00", 1.0);
    let time_x = width - time_width * 11.0 / 8.0;
    let white = Color::WHITE;
    if race.phase != Phase::Intro {
        if !blink {
            frame.write("font_ths", &clock(shown), Vec2::new(time_x, TOP), 1.0, white);
        }
        let count = format!("{}/{}", player.display_lap(laps), laps);
        let count_x = time_x - frame.width("font_ths", &count, 1.0) - 14.0;
        frame.write("font_ths", &count, Vec2::new(count_x, TOP), 1.0, white);
        let label = string(text::LAP);
        frame.write("font_ths", &label, Vec2::new(count_x - frame.width("font_ths", &label, 1.0) - 8.0, TOP), 1.0, white);
        if let Some(best) = state.best_lap {
            let y = TOP + line * 7.0 / 8.0;
            let label = string(text::BEST);
            frame.write("font_ths", &clock(best), Vec2::new(time_x, y), 1.0, white);
            frame.write("font_ths", &label, Vec2::new(time_x - frame.width("font_ths", &label, 1.0) - 14.0, y), 1.0, white);
        }

        // Place: a big number that swells when it changes, and its ending.
        if player.place != state.place {
            (state.place, state.pulse) = (player.place, Some(0.0));
        }
        let swell = match &mut state.pulse {
            Some(age) => {
                *age += dt;
                let swell = if *age < PULSE.0 { 1.0 + *age / PULSE.0 } else { (2.0 - (*age - PULSE.0) / PULSE.0).max(1.0) };
                if *age > PULSE.1 * 3.0 {
                    state.pulse = None;
                }
                swell
            }
            None => 1.0,
        };
        let place_x = width / 10.0;
        let number = player.place.to_string();
        let number_width = frame.width("ignum", &number, 1.0);
        frame.write("ignum", &number, Vec2::new(place_x - number_width, TOP), swell, white);
        frame.write("font_ths", &string(text::PLACES + (player.place - 1).min(3)), Vec2::new(place_x + 5.0, TOP), 1.0, white);

        if race.phase == Phase::Racing && player.wrong_way() {
            let words = string(text::WRONG_WAY);
            let at = Vec2::new((width - frame.width("font_ths", &words, 1.0)) / 2.0, HEIGHT / 5.0);
            frame.write("font_ths", &words, at, 1.0, white);
        }
    }

    // The power-up in hand: a holder in its colour, its picture, and a badge for
    // the white bricks that go with it.
    let level = player.whites.min(3) as usize;
    let held = player.held.map(|power| match power {
        Power::Red => (Color::srgb(1.0, 0.0, 0.0), 2 + level),
        Power::Yellow => (Color::srgb(1.0, 1.0, 0.0), 6 + level),
        Power::Blue => (Color::srgb_u8(0x50, 0x50, 0xff), 16 + level),
        Power::Green => (Color::srgb(0.0, 1.0, 0.0), 20 + level),
    });
    if held.is_some() || level > 0 {
        let size = art.pictures[HOLDER].as_ref().map_or(Vec2::ZERO, |p| p.1);
        let at = Vec2::new(width / 32.0, HEIGHT - size.y - 3.0);
        frame.picture(HOLDER, at, 1.0, white);
        frame.picture(HOLDER_TINT, at, 1.0, held.map_or(white, |h| h.0));
        if let Some((_, picture)) = held {
            let item = art.pictures[picture].as_ref().map_or(Vec2::ZERO, |p| p.1);
            frame.picture(picture, at + size * Vec2::new(31.0, 38.0) / 64.0 - item / 2.0, 1.0, white);
        }
        if level > 0 {
            frame.picture(HOLDER_TINT + level, at + Vec2::new(size.x * 24.0 / 64.0, 0.0), 1.0, white);
        }
    }

    // Bottom right: the map with everyone on it, or the speedometer.
    let forward = player.rot * Vec3::NEG_Z;
    let speed = player.vel.dot(forward) / UNIT / 1000.0;
    state.speed = state.speed * 0.8 + speed * 0.2;
    let corner = Vec2::new(width - MAP_INSET, HEIGHT - MAP_INSET);
    match (state.gadget, &art.map) {
        (0, Some((picture, [min_x, max_y, max_x, min_y]))) => {
            let range = Vec2::new(max_x - min_x, max_y - min_y);
            let per_unit = MAP_SIZE / range.max_element();
            let size = range * per_unit;
            // Mirrored, the map is turned over top to bottom, as `RaceHud` draws it.
            let node = ImageNode { image: picture.clone(), image_mode: NodeImageMode::Stretch, flip_y: variant.mirror, ..default() };
            frame.nodes.push((place(corner - size, size), node, UiTransform::IDENTITY));
            // East is right and north is up.
            let spot = |k: &Kart| {
                let north = if variant.mirror { k.pos.z / UNIT - max_y } else { min_y + k.pos.z / UNIT };
                corner + Vec2::new(k.pos.x / UNIT - max_x, north) * per_unit
            };
            // The marker picture holds four; the first is the one for other racers.
            if let Some((handle, _)) = art.pictures[10].clone() {
                for (kart, _) in karts.iter().filter(|k| !k.1 && k.0.out.is_none()) {
                    let rect = Some(Rect::new(0.0, 0.0, MARKER, MARKER));
                    let node = ImageNode { image: handle.clone(), rect, image_mode: NodeImageMode::Stretch, ..default() };
                    frame.nodes.push((place(spot(kart) - MARKER / 2.0, Vec2::splat(MARKER)), node, UiTransform::IDENTITY));
                }
            }
            // The player: an arrow pointing the way the kart is.
            let heading = Vec2::new(forward.x, forward.z);
            let turn = UiTransform { rotation: Rot2::radians(heading.y.atan2(heading.x)), ..UiTransform::IDENTITY };
            let node = ImageNode { image: art.arrow.clone(), image_mode: NodeImageMode::Stretch, ..default() };
            frame.nodes.push((place(spot(player) - ARROW_SPAN / 2.0, Vec2::splat(ARROW_SPAN)), node, turn));
        }
        (1, _) => {
            let size = art.pictures[0].as_ref().map_or(Vec2::ZERO, |p| p.1);
            let at = Vec2::new(width, HEIGHT) - size - 2.0;
            frame.picture(0, at, 1.0, white);
            // The needle turns about a hub off-centre in the dial.
            let hub = Vec2::new(width, HEIGHT) - size * 52.0 / 128.0 - 2.0;
            let angle = NEEDLE_REST + (state.speed / NEEDLE_FULL_SPEED).clamp(0.0, 1.0) * NEEDLE_SWEEP;
            let along = Vec2::new(angle.cos(), angle.sin());
            for step in 0..15 {
                let reach = step as f32 / 14.0;
                let thick = 5.0 - reach * 3.0;
                frame.block(hub + along * reach * NEEDLE_LENGTH - thick / 2.0, Vec2::splat(thick), Color::srgb_u8(0xc8, 0xc8, 0xc8));
            }
            let dot = art.pictures[1].as_ref().map_or(Vec2::ZERO, |p| p.1);
            frame.picture(1, hub - dot / 2.0, 1.0, white);
        }
        _ => {}
    }

    // The finishing order, and what to press next. In a circuit the points go beside
    // it, and after the last race the order is the circuit's.
    // The port's own keys, under whatever the race's end offers.
    let extras = "R: REPLAY   P: PHOTO";
    if replay.showing.is_some() {
        let keys = "LEFT RIGHT: CAR   T: CAMERA   ESC: BACK";
        let at = Vec2::new((width - frame.width("font_ths", keys, 0.6)) / 2.0, HEIGHT - 1.2 * line);
        frame.write("font_ths", keys, at, 0.6, white);
    } else if race.phase == Phase::Finished && settings.time_race {
        // Against the clock: the laps, what they come to, the time to beat, and how it went.
        let mut rows: Vec<(String, String, Color)> = time_race
            .run
            .laps
            .iter()
            .enumerate()
            .map(|(lap, &seconds)| (format!("{} {}", string(text::LAP), lap + 1), clock(seconds), white))
            .collect();
        rows.push(("TOTAL".into(), clock(time_race.run.total()), yellow));
        if let Some(record) = &time_race.record {
            rows.push((string(text::BEST), clock(record.total()), white));
        }
        let top = HEIGHT * 0.3;
        for (row, (label, value, colour)) in rows.iter().enumerate() {
            let y = top + row as f32 * line * 7.0 / 8.0;
            frame.write("font_ths", label, Vec2::new(width / 2.0 - 150.0, y), 1.0, *colour);
            frame.write("font_ths", value, Vec2::new(width / 2.0 + 150.0 - frame.width("font_ths", value, 1.0), y), 1.0, *colour);
        }
        let verdict = if time_race.result == Some(true) { text::RECORD_BEATEN } else { text::RECORD_STANDS };
        for (row, words) in [string(verdict), string(verdict + 1)].iter().enumerate() {
            let y = top + (rows.len() as f32 + 0.6 + row as f32 * 0.8) * line * 7.0 / 8.0;
            frame.write("font_ths", words, Vec2::new((width - frame.width("font_ths", words, 0.75)) / 2.0, y), 0.75, yellow);
        }
        let prompt = format!("ENTER: {}   ESC: {}", string(text::RESTART), string(text::EXIT));
        let at = Vec2::new((width - frame.width("font_ths", &prompt, 0.75)) / 2.0, HEIGHT * 0.3 + 7.5 * line);
        frame.write("font_ths", &prompt, at, 0.75, white);
        let at = Vec2::new((width - frame.width("font_ths", extras, 0.75)) / 2.0, HEIGHT * 0.3 + 8.3 * line);
        frame.write("font_ths", extras, at, 0.75, white);
    } else if race.phase == Phase::Finished {
        let run = championship.run.as_ref();
        let over = championship.over();
        let mut rows: Vec<&Kart> = karts.iter().map(|k| k.0).collect();
        rows.sort_by_key(|k| if over { championship.standing(k.slot) } else { k.place });
        let top = HEIGHT * 0.3;
        let right = |frame: &mut Frame, words: &str, x: f32, y: f32, colour: Color| {
            let at = Vec2::new(x - frame.width("font_ths", words, 1.0), y);
            frame.write("font_ths", words, at, 1.0, colour);
        };
        for (row, kart) in rows.iter().enumerate() {
            let y = top + row as f32 * line * 7.0 / 8.0;
            let colour = if std::ptr::eq(*kart, player) { yellow } else { white };
            let position = if over { championship.standing(kart.slot) } else { kart.place };
            let place = format!("{}{}", position, string(text::PLACES + (position - 1).min(3)));
            frame.write("font_ths", &place, Vec2::new(width / 2.0 - 230.0, y), 1.0, colour);
            frame.write("font_ths", &kart.name, Vec2::new(width / 2.0 - 160.0, y), 1.0, colour);
            match run {
                Some(run) => {
                    if !over {
                        right(&mut frame, &format!("+{}", run.round_points[kart.slot]), width / 2.0 + 160.0, y, colour);
                    }
                    right(&mut frame, &run.points[kart.slot].to_string(), width / 2.0 + 230.0, y, colour);
                }
                None => right(&mut frame, &kart.finished.map_or("-".into(), clock), width / 2.0 + 230.0, y, colour),
            }
        }
        let prompt = match run {
            Some(_) => format!("ENTER: {}", string(text::CONTINUE)),
            None => format!("ENTER: {}   ESC: {}", string(text::RESTART), string(text::EXIT)),
        };
        let at = Vec2::new((width - frame.width("font_ths", &prompt, 0.75)) / 2.0, HEIGHT * 0.3 + 6.5 * line);
        frame.write("font_ths", &prompt, at, 0.75, white);
        let at = Vec2::new((width - frame.width("font_ths", extras, 0.75)) / 2.0, HEIGHT * 0.3 + 7.3 * line);
        frame.write("font_ths", extras, at, 0.75, white);
    }

    // A warp washes the screen blue as the car goes into the tunnel and comes out of it.
    let wash = if player.warp_start > 0.0 {
        1.0 - player.warp_start / WARP_WASH
    } else if player.warp > 0.0 {
        let through = crate::items::WARP_TIME - player.warp;
        (1.0 - through / WARP_WASH).max(1.0 - player.warp / WARP_WASH)
    } else {
        state.wash - dt / WARP_WASH
    };
    state.wash = wash.clamp(0.0, 1.0);
    if state.wash > 0.0 {
        let colour = Color::srgba(0.0, 0.0, WARP_BLUE, state.wash);
        let node = ImageNode { image: art.white.clone(), color: colour, image_mode: NodeImageMode::Stretch, ..default() };
        frame.nodes.insert(0, (place(Vec2::ZERO, Vec2::new(width, HEIGHT)), node, UiTransform::IDENTITY));
    }

    // Paused: everything else gives way to a darkened screen and the menu.
    if let Some(dialog) = &pause.0 {
        frame.nodes.clear();
        let shade = ImageNode { image: art.white.clone(), color: Color::srgba(0.0, 0.0, 0.0, 64.0 / 255.0), image_mode: NodeImageMode::Stretch, ..default() };
        frame.nodes.push((place(Vec2::ZERO, Vec2::new(width, HEIGHT)), shade, UiTransform::IDENTITY));
        let step = line * 2.0;
        let top = HEIGHT / 2.0 - dialog.options.len() as f32 * step / 2.0;
        frame.banner(&string(dialog.prompt), Vec2::new(width / 2.0, top - step / 2.0), 1.0, white);
        for (row, &option) in dialog.options.iter().enumerate() {
            // The answer picked out pulses.
            let colour = if row == dialog.selected {
                let pulse = 0.75 + 0.25 * (real.elapsed_secs_f64() * std::f64::consts::TAU).cos() as f32;
                Color::srgba(1.0, 1.0, 0x24 as f32 / 255.0, pulse)
            } else {
                Color::srgb(0.5, 0.5, 0x12 as f32 / 255.0)
            };
            frame.banner(&string(option), Vec2::new(width / 2.0, top + (row as f32 + 0.5) * step), 1.0, colour);
        }
    }

    let nodes = frame.nodes;
    let screen = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((Root, screen)).with_children(|root| {
        for node in nodes {
            root.spawn(node);
        }
    });
}
