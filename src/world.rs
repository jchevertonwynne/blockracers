//! Builds a playable circuit out of the original game's data: the track model and its
//! textures for rendering, an AI route as the racing line, and the power-up bricks.

use crate::assets::{
    mab,
    Jam,
    bvb::Volume,
    gdb::{Batch, Bone, Model, Vertex, parse_skeleton},
    image, materials, route,
    tokens::{Token, tokenize},
};
use crate::items::Power;
use crate::particles;
use crate::roster::{self, Driver};
use crate::scenery;
use crate::physics::UNIT;
use crate::track::{Checkpoint, Track};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use std::collections::HashMap;
use std::sync::Arc;

const DEFAULT_JAM: &str = "Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM";
/// Half-width of the band around the recorded racing line that the AI may use.
const LANE: f32 = 4.0;

/// The game is Z-up; this is a pure rotation onto Bevy's Y-up axes.
fn to_world(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1]) * UNIT
}

pub struct Surface {
    mesh: Mesh,
    pub texture: Option<image::Pixels>,
    cutout: bool,
    blend: bool,
    additive: bool,
    /// Which of the model's materials this is.
    pub material: usize,
}

/// A pair of wheels: the game rigs each axle as one bone of the wheel model.
pub struct Axle {
    pub surfaces: Vec<Surface>,
    pub position: Vec3,
    pub rotation: Quat,
    /// Wheel radius, in the model's own units.
    pub radius: f32,
}

/// One racer's car, wheels and minifigure, each still in the game's model space
/// (X forward, Y left, Z up) and needing its own scale.
pub struct KartModel {
    pub body: Vec<Surface>,
    pub body_scale: f32,
    pub axles: Vec<Axle>,
    pub wheel_scale: f32,
    pub driver: Vec<Surface>,
    pub driver_scale: f32,
    pub chassis: Chassis,
    /// Half-width, and how far the car reaches ahead of and behind its origin, in
    /// game units.
    pub outline: [f32; 3],
}

#[derive(Resource)]
pub struct LoadedWorld {
    surfaces: Vec<Surface>,
    pub bricks: Vec<(Option<Power>, Vec3)>,
    /// What the player's shots lock onto, and the event that puts each out of use.
    pub targets: Vec<(Vec3, i32)>,
    /// One per grid slot, in the order of the driver roster.
    pub karts: Vec<KartModel>,
    /// Who is in each of those slots; the player is the last.
    pub field: Vec<Driver>,
    /// The drives the computer's cars play back, one per slot.
    pub routes: Vec<Arc<route::Record>>,
    /// The record run of a time race, and cars for it and the player's best to be shown as.
    pub ghost: Option<crate::time_race::Run>,
    pub ghost_models: Vec<KartModel>,
    /// The colours of the sky.
    pub sky: Option<crate::sky::Sky>,
    /// Scenery and animated models around the track.
    pub props: Vec<scenery::PropDef>,
    /// The models power-ups are made of.
    pub models: Vec<scenery::PropDef>,
    /// Particle emitters by name, with the picture each one's particles use.
    pub emitters: Vec<(String, particles::EmitterDef, particles::Look)>,
    /// Pictures of materials that hazards swap onto models, by material name.
    pub swatches: Vec<(String, image::Pixels)>,
}

/// Materials and texture definitions, plus where to look for the textures themselves.
pub struct Library<'a> {
    jam: &'a Jam,
    dirs: Vec<String>,
    materials: HashMap<String, materials::Material>,
    textures: HashMap<String, materials::Texture>,
}

impl<'a> Library<'a> {
    fn new<'f>(jam: &'a Jam, files: impl Iterator<Item = &'f str>, dirs: &[&str]) -> Self {
        let mut library = Library {
            jam,
            dirs: dirs.iter().map(|d| d.to_string()).collect(),
            materials: HashMap::new(),
            textures: HashMap::new(),
        };
        for file in files {
            let Some(data) = jam.get(file) else { continue };
            if file.to_uppercase().ends_with(".MDB") {
                library.materials.extend(materials::parse_mdb(data));
            } else {
                library.textures.extend(materials::parse_tdb(data));
            }
        }
        library
    }

    /// The picture a material is drawn with.
    pub fn texture(&self, material: &str) -> Option<image::Pixels> {
        let name = self.materials.get(material)?.texture.clone()?;
        let definition = self.textures.get(&name).cloned().unwrap_or_default();
        let mut pixels = self.picture(&name, &definition)?;
        if definition.flip {
            let row = pixels.width as usize * 4;
            pixels.rgba = pixels.rgba.chunks(row).rev().flatten().copied().collect();
        }
        Some(pixels)
    }

    /// A material's picture in the material's own colour and at its own strength, for
    /// things drawn without lighting or vertex colours.
    pub fn tinted(&self, material: &str) -> Option<image::Pixels> {
        let mut pixels = self.texture(material)?;
        let info = self.materials.get(material)?;
        let tint = [info.diffuse[0], info.diffuse[1], info.diffuse[2], info.alpha.unwrap_or(255)];
        for pixel in pixels.rgba.as_chunks_mut::<4>().0 {
            for (value, scale) in pixel.iter_mut().zip(tint) {
                *value = (*value as u16 * scale as u16 / 255) as u8;
            }
        }
        Some(pixels)
    }

    /// Whether a material is added to what is behind it.
    pub fn additive(&self, material: &str) -> bool {
        self.materials.get(material).is_some_and(|m| m.additive)
    }

    /// A texture's pixels, from whichever of the folders holds it.
    fn picture(&self, name: &str, definition: &materials::Texture) -> Option<image::Pixels> {
        let file = |ext: &str| self.dirs.iter().find_map(|dir| self.jam.get(&format!("{dir}/{name}.{ext}")));
        if definition.tga {
            return image::decode_tga(file("TGA")?);
        }
        image::decode_bmp(file("BMP")?, definition.color_key)
    }

    /// One mesh per material out of the batches of `model` that pass `keep`.
    pub fn surfaces(
        &self,
        model: &Model,
        keep: impl Fn(&Batch) -> bool,
        place: impl Fn([f32; 3]) -> Vec3,
    ) -> Vec<Surface> {
        let mut by_material: HashMap<usize, Vec<u32>> = HashMap::new();
        for batch in model.batches.iter().filter(|b| keep(b)) {
            by_material.entry(batch.material).or_default().extend(&batch.indices);
        }
        let mut surfaces = Vec::new();
        for (material, indices) in &by_material {
            let info = model.materials.get(*material).and_then(|name| self.materials.get(name));
            let texture_name = info.and_then(|m| m.texture.clone()).unwrap_or_default();
            let definition = self.textures.get(&texture_name).cloned().unwrap_or_default();
            let texture = self.picture(&texture_name, &definition).map(|mut pixels| {
                if definition.flip {
                    let row = pixels.width as usize * 4;
                    pixels.rgba = pixels.rgba.chunks(row).rev().flatten().copied().collect();
                }
                pixels
            });

            let vertex = |&i: &u32| model.vertices[i as usize];
            // Lighting is baked into vertex colours, which multiply the texture.
            let colour = |v: Vertex| {
                let c = |x: u8| (x as f32 / 255.0).powf(2.2);
                [c(v.color[0]), c(v.color[1]), c(v.color[2]), v.color[3] as f32 / 255.0]
            };
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    indices.iter().map(|i| place(vertex(i).pos)).collect::<Vec<_>>(),
                )
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_UV_0,
                    indices.iter().map(|i| vertex(i).uv).collect::<Vec<_>>(),
                )
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_COLOR,
                    indices.iter().map(|i| colour(vertex(i))).collect::<Vec<_>>(),
                )
                .with_computed_flat_normals();
            surfaces.push(Surface {
                mesh,
                texture,
                cutout: definition.color_key.is_some() || info.is_some_and(|m| m.alpha_test),
                blend: definition.tga || info.is_some_and(|m| m.blend),
                additive: info.is_some_and(|m| m.additive),
                material: *material,
            });
        }
        surfaces
    }
}

const COMMON: &str = "/GAMEDATA/COMMON";

/// A bone's rotation and position in model space.
fn bone_pose(bones: &[Bone], index: usize) -> (Quat, Vec3) {
    let bone = &bones[index];
    // Stored for vectors multiplied from the other side, as every rotation of the game's is.
    let (rotation, position) = (crate::assets::adb::turn(bone.rotation), Vec3::from(bone.position));
    match bone.parent {
        Some(parent) => {
            let (parent_rotation, parent_position) = bone_pose(bones, parent);
            (parent_rotation * rotation, parent_position + parent_rotation * position)
        }
        None => (rotation, position),
    }
}

/// Loads the car of the champion whose files start with `prefix` ("rr" for Rocket Racer).
fn load_kart(jam: &Jam, driver: &Driver) -> Option<KartModel> {
    let prefix = &driver.car;
    let part = |name: &str| {
        let file = |ext: &str| format!("{COMMON}/{name}.{ext}");
        let model = Model::parse(jam.get(&file("GDB"))?)?;
        let library = Library::new(jam, [file("MDB"), file("TDB")].iter().map(String::as_str), &[COMMON]);
        Some((model, library))
    };
    let bones_used = |model: &Model| {
        let mut bones: Vec<usize> = model.batches.iter().filter_map(|b| b.bone).collect();
        bones.sort();
        bones.dedup();
        bones
    };

    let (body, library) = part(&format!("{prefix}CM"))?;
    let body_surfaces = library.surfaces(&body, |_| true, Vec3::from);

    let (wheels, library) = part(&format!("{prefix}JMW"))?;
    let skeleton = parse_skeleton(jam.get(&format!("{COMMON}/{prefix}JMW.SDB"))?)?;
    let mut axles = Vec::new();
    for bone in bones_used(&wheels) {
        let (rotation, position) = bone_pose(&skeleton, bone);
        let radius = wheels
            .batches
            .iter()
            .filter(|b| b.bone == Some(bone))
            .flat_map(|b| &b.indices)
            .map(|&i| wheels.vertices[i as usize].pos[2].abs())
            .fold(0.0, f32::max);
        let surfaces = library.surfaces(&wheels, |b| b.bone == Some(bone), Vec3::from);
        axles.push(Axle { surfaces, position, rotation, radius });
    }

    // Every minifigure shares one skeleton; bake the figure into its rest pose.
    let (figure, library) = part(&driver.figure)?;
    let skeleton = parse_skeleton(jam.get(&format!("{COMMON}/PELVIS.SDB"))?)?;
    let mut driver_surfaces = Vec::new();
    for bone in bones_used(&figure) {
        let (rotation, position) = bone_pose(&skeleton, bone);
        let place = |p: [f32; 3]| position + rotation * Vec3::from(p);
        driver_surfaces.extend(library.surfaces(&figure, |b| b.bone == Some(bone), place));
    }

    // The car's outline, from the body and the wheels at the ends of each axle.
    let mut outline = [0.0f32; 3];
    let mut cover = |x: f32, y: f32| {
        outline = [outline[0].max(y.abs()), outline[1].max(x), outline[2].max(-x)];
    };
    for v in &body.vertices {
        cover(v.pos[0] * body.scale, v.pos[1] * body.scale);
    }
    for axle in &axles {
        let reach = wheels.vertices.iter().map(|v| v.pos[0].abs()).fold(0.0, f32::max);
        for x in [-axle.radius, axle.radius] {
            cover((axle.position.x + x) * wheels.scale, reach * wheels.scale);
        }
    }

    Some(KartModel {
        outline,
        body: body_surfaces,
        body_scale: body.scale,
        axles,
        wheel_scale: wheels.scale,
        driver: driver_surfaces,
        driver_scale: figure.scale,
        chassis: chassis(jam, &driver.chassis)?,
    })
}

/// What the chassis table (`CHASSIS.CMB`) says about one car, in game units and axes.
pub struct Chassis {
    /// Where the driver sits.
    pub mount: Vec3,
    /// Where each wheel meets the ground: front left, front right, rear left, rear right.
    pub wheels: [Vec3; 4],
    /// Width and length of the car's footprint.
    pub footprint: Vec2,
    /// Handling, top speed and acceleration, 0..100.
    pub stats: [f32; 3],
    /// How high the engine revs.
    pub engine_pitch: f32,
}

fn chassis(jam: &Jam, name: &str) -> Option<Chassis> {
    let tokens = tokenize(jam.get(&format!("{COMMON}/CHASSIS.CMB"))?);
    let start = tokens.iter().position(|t| matches!(t, Token::Str(entry) if entry.eq_ignore_ascii_case(name)))?;
    // The entry runs up to the next chassis.
    let end = tokens[start + 1..]
        .iter()
        .position(|t| *t == Token::Key(0x27))
        .map_or(tokens.len(), |i| start + 1 + i);
    let entry = &tokens[start..end];
    let numbers = |key: u16, skip: usize, count: usize| -> Option<Vec<f32>> {
        let at = entry.iter().position(|t| *t == Token::Key(key))?;
        let values = entry[at + 1..].iter().filter(|t| !matches!(t, Token::LCurly));
        values.skip(skip).take(count).map(|t| match t {
            Token::Float(v) => Some(*v),
            Token::Int(v) => Some(*v as f32),
            _ => None,
        }).collect()
    };
    let vec3 = |v: &[f32]| Vec3::new(v[0], v[1], v[2]);
    // Contact points come after two skid-mark widths: front right, front left, rear
    // right, rear left (Y is left).
    let contacts = numbers(0x30, 2, 12)?;
    let footprint = numbers(0x2e, 0, 2)?;
    Some(Chassis {
        mount: vec3(&numbers(0x2b, 0, 3)?),
        wheels: [vec3(&contacts[3..6]), vec3(&contacts[0..3]), vec3(&contacts[9..12]), vec3(&contacts[6..9])],
        footprint: Vec2::new(footprint[0], footprint[1]),
        stats: [numbers(0x3a, 0, 1)?[0], numbers(0x3b, 0, 1)?[0], numbers(0x3c, 0, 1)?[0]],
        engine_pitch: numbers(0x2f, 0, 1).map_or(1.0, |v| v[0]),
    })
}

/// Puts another of the game's drivers, with their car, in a grid slot: `code` is the
/// driver's in `roster::NAMES`. The port's own, for players online who have chosen
/// who to race as. False if there is no such driver or their car won't load.
pub fn recast(loaded: &mut LoadedWorld, slot: usize, code: &str) -> bool {
    let cast = open_jam().and_then(|jam| {
        let driver = roster::driver(&jam, code)?;
        Some((load_kart(&jam, &driver)?, driver))
    });
    match (cast, loaded.karts.get_mut(slot), loaded.field.get_mut(slot)) {
        (Some((kart, driver)), Some(car), Some(seat)) => {
            (*car, *seat) = (kart, driver);
            true
        }
        _ => false,
    }
}

fn open_jam() -> Option<Jam> {
    Jam::open(std::env::var("LEGO_JAM").unwrap_or(DEFAULT_JAM.into()))
}

/// Display names for the race folders. The archive's own race definitions mostly carry
/// a placeholder name, so these are matched up from each folder's scenery.
const CIRCUIT_NAMES: [(&str, &str); 13] = [
    ("RACEC0R0", "Royal Knights Raceway"),
    ("RACEC0R1", "Imperial Grand Prix"),
    ("RACEC0R2", "Desert Adventure Dragway"),
    ("RACEC0R3", "Magma Moon Marathon"),
    ("RACEC1R0", "Dark Forest Dash"),
    ("RACEC1R1", "Tribal Island Trail"),
    ("RACEC1R2", "Amazon Adventure Alley"),
    ("RACEC1R3", "Ice Planet Pathway"),
    ("RACEC2R0", "Knightmare-athon"),
    ("RACEC2R1", "Pirate Skull Pass"),
    ("RACEC2R2", "Adventure Temple Trail"),
    ("RACEC2R3", "Alien Rally Asteroid"),
    ("RACEC3R0", "Rocket Racer Run"),
];

/// The circuits in the original game's archive, as (folder, display name).
pub fn circuits() -> Vec<(String, String)> {
    let Some(jam) = open_jam() else { return Vec::new() };
    CIRCUIT_NAMES
        .iter()
        .filter(|(race, _)| jam.get(&format!("/GAMEDATA/{race}/{race}.RAB")).is_some())
        .map(|(race, name)| (race.to_string(), name.to_string()))
        .collect()
}

/// The folders of the game's races in the order its circuits run them, each with which
/// circuit it belongs to: the three sets of four in turn, then Rocket Racer's.
pub fn circuit_order() -> Vec<(String, usize)> {
    let Some(jam) = open_jam() else { return Vec::new() };
    let mut order: Vec<(String, usize)> = Vec::new();
    let mut group = 0;
    for (_, rounds) in roster::circuits(&jam) {
        // The mirrored circuits run the same races over again.
        let fresh: Vec<_> = rounds.iter().filter(|r| !r.mirrored && !order.iter().any(|o| o.0 == r.folder)).collect();
        if fresh.is_empty() {
            continue;
        }
        order.extend(fresh.into_iter().map(|round| (round.folder.clone(), group)));
        group += 1;
    }
    order
}

/// Loads a race (a folder name such as `RACEC0R0`) from the archive at `$LEGO_JAM`.
/// `None` if the game data isn't there or doesn't hold what we need.
#[cfg(test)]
pub fn load(race: &str) -> Option<(Track, LoadedWorld)> {
    load_in(race, None, false)
}

/// As `load`, with the field of a given circuit rather than the race's own, and the
/// bricks of a time race or an ordinary one.
pub fn load_in(race: &str, circuit: Option<&str>, time_race: bool) -> Option<(Track, LoadedWorld)> {
    let jam = open_jam()?;
    let dir = format!("/GAMEDATA/{race}");
    let mut files: Vec<&str> = jam.list(&dir).collect();
    files.sort();
    let with_ext = |ext: &'static str| files.iter().copied().filter(move |f| f.ends_with(ext));

    // The biggest model in the folder is the track itself.
    let model_file = with_ext(".GDB").max_by_key(|f| jam.get(f).map_or(0, <[u8]>::len))?;
    let model = Model::parse(jam.get(model_file)?)?;
    let library = Library::new(&jam, with_ext(".MDB").chain(with_ext(".TDB")), &[&dir, COMMON]);
    let surfaces = library.surfaces(&model, |_| true, to_world);

    let route_file = with_ext(".RRB").next()?;
    let lap = route::Record::parse(jam.get(route_file)?, false)?.lap();
    let line: Vec<Vec3> = lap.into_iter().map(to_world).collect();
    let mut track = Track::from_loop(&line, LANE);

    // The solid world, minus trigger surfaces (checkpoints and the like).
    let mut surface_table = HashMap::new();
    for file in with_ext(".TMB") {
        surface_table.extend(materials::parse_tmb(jam.get(file)?));
    }
    // The biggest volume is the track; the small ones are checkpoints and the start line.
    let volume_file = with_ext(".BVB").max_by_key(|f| jam.get(f).map_or(0, <[u8]>::len))?;
    let volume = Volume::parse(jam.get(volume_file)?)?;
    // Each surface material gets a tag, so that hazards can open and close it.
    for (index, name) in volume.materials.iter().enumerate() {
        let surface = surface_table.get(name).copied().unwrap_or_default();
        // Surfaces that only shots pass through are the invisible barriers cars stop at.
        let passable = surface.non_solid;
        if surface.shots_pass {
            track.collision.set_shots_pass(index + 1);
        }
        track.collision.set_passable(index + 1, passable);
        track.surfaces.insert(name.clone(), (index + 1, passable));
    }
    for tri in &volume.triangles {
        let name = volume.materials.get(tri[3] as usize);
        let mut surface = name.and_then(|n| surface_table.get(n)).copied().unwrap_or_default();
        surface.force = to_world(surface.force).to_array();
        let corner = |i: u16| volume.vertices.get(i as usize).copied().map(to_world);
        track.collision.add_tagged([corner(tri[0])?, corner(tri[1])?, corner(tri[2])?], surface, tri[3] as usize + 1);
    }
    // The race definition names the volume that is the finish line.
    let finish_volume = with_ext(".RAB")
        .filter_map(|f| jam.get(f))
        .find_map(|data| {
            let tokens = tokenize(data);
            let at = tokens.iter().position(|t| *t == Token::Key(0x2b))?;
            match tokens.get(at + 3)? {
                Token::Str(name) => Some(name.to_lowercase()),
                _ => None,
            }
        })
        .unwrap_or("startfin".into());

    // Race rules: checkpoint gates, the finish line and the starting grid.
    let has_finish_volume = jam.get(&format!("{dir}/{finish_volume}.BVB")).is_some();
    for (name, position, forward, up) in
        with_ext(".WDB").filter_map(|f| jam.get(f)).flat_map(route::parse_placements)
    {
        let file = format!("{dir}/{name}.BVB");
        if file.eq_ignore_ascii_case(volume_file) {
            continue;
        }
        let Some(volume) = jam.get(&file).and_then(Volume::parse) else { continue };
        let (origin, forward, up) = (Vec3::from(position), Vec3::from(forward), Vec3::from(up));
        let left = up.cross(forward);
        let place = |i: u16| {
            let v = Vec3::from(volume.vertices[i as usize]);
            to_world((origin + forward * v.x + left * v.y + up * v.z).to_array())
        };
        // The finish line is the volume the race definition names; some circuits name
        // one that isn't there, and then it is the one called "start" something.
        let is_finish = name == finish_volume || (!has_finish_volume && name.starts_with("st"));
        // Anything else is a door: open until an animated model closes it.
        let door = track.surfaces.len() + 1000;
        if !name.starts_with("chckpt") && !is_finish {
            track.collision.set_passable(door, true);
            track.surfaces.insert(name.clone(), (door, true));
        }
        for tri in &volume.triangles {
            let corners = [place(tri[0]), place(tri[1]), place(tri[2])];
            let gate = volume.materials.get(tri[3] as usize).and_then(|m| m.parse::<usize>().ok());
            // The checkpoint volume also holds surfaces for unrelated events.
            match (name.starts_with("chckpt"), gate) {
                (true, Some(gate)) => track.course.gates.add_tagged(corners, default(), gate),
                (true, None) => {}
                (false, _) if is_finish => track.course.finish.add(corners, default()),
                (false, _) => track.collision.add_tagged(corners, default(), door),
            }
        }
    }
    if let Some(checkpoints) = with_ext(".CPB").find_map(|f| route::parse_checkpoints(jam.get(f)?)) {
        track.course.checkpoints = checkpoints
            .into_iter()
            .map(|c| Checkpoint {
                normal: to_world(c.normal).normalize_or_zero(),
                position: to_world(c.position),
                next: c.next,
                fraction: 0.0,
            })
            .collect();
        track.course.compute_fractions();
    }
    let lap_zones: Vec<(i32, u8)> =
        with_ext(".EVB").filter_map(|f| jam.get(f)).flat_map(route::parse_lap_zones).collect();
    for trigger in with_ext(".TRB").filter_map(|f| jam.get(f)).flat_map(route::parse_triggers) {
        if let Some(&(_, zone)) = lap_zones.iter().find(|z| z.0 == trigger.event && z.1 != 1) {
            track.course.zones.push((to_world(trigger.centre), trigger.radius * UNIT, zone));
        }
    }
    if let Some(mut grid) = with_ext(".SPB").find_map(|f| route::parse_start_positions(jam.get(f)?)) {
        grid.sort_by_key(|g| g.0);
        track.course.grid = grid
            .into_iter()
            .map(|(_, position, forward)| (to_world(position), to_world(forward).normalize_or_zero()))
            .collect();
    }

    // `RaceSession` loads one power-up file: the one the race names, or for a time race
    // the one with a 2 on the end of that name.
    let bricks = with_ext(".PWB")
        .filter(|f| f.ends_with("2.PWB") == time_race)
        .filter_map(|f| jam.get(f))
        .flat_map(route::parse_powerups)
        .map(|(brick, pos)| {
            let power = match brick {
                route::Brick::Red => Some(Power::Red),
                route::Brick::Yellow => Some(Power::Yellow),
                route::Brick::Blue => Some(Power::Blue),
                route::Brick::Green => Some(Power::Green),
                route::Brick::White => None,
            };
            (power, to_world(pos))
        })
        .collect();

    let targets: Vec<(Vec3, i32)> = with_ext(".TGB")
        .filter_map(|f| route::parse_targets(jam.get(f)?))
        .flatten()
        .map(|(position, index)| (to_world(position), index))
        .collect();

    info!("loaded {race}: {model_file}, {route_file}, lap {:.0}, {} targets", track.length, targets.len());
    let circuit = circuit.map(str::to_string).or_else(|| roster::circuit_of(&jam, race)).unwrap_or("c0".into());
    let ghost = jam.get(&format!("{dir}/GHOST.GHB")).and_then(crate::time_race::Run::parse);
    // Each of the computer's cars plays one of the drives recorded for its place on
    // the grid, picked by chance.
    let pick = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.subsec_nanos() as usize);
    let routes = (1..=5)
        .filter_map(|slot| {
            let prefix = format!("{dir}/R{slot}_");
            let recorded: Vec<&str> = with_ext(".RRB").filter(|f| f.starts_with(&prefix)).collect();
            let file = recorded.get((pick / slot) % recorded.len().max(1))?;
            Some(Arc::new(route::Record::parse(jam.get(file)?, false)?))
        })
        .collect();
    let track_model = model_file.rsplit('/').next().unwrap_or_default().trim_end_matches(".GDB").to_lowercase();
    let props = scenery::load(&jam, &dir, &library, &track_model);
    // The circuit's own emitters first, then the ones every circuit shares.
    let mut emitters = Vec::new();
    for file in with_ext(".EMB") {
        let animation = format!("{dir}/EMIT{}.MAB", &race[race.len().saturating_sub(4)..]);
        add_emitters(&jam, file, &library, &animation, |_| true, &mut emitters);
    }
    let mut world = shared(&jam, &circuit, emitters);
    world.swatches.extend(crate::hazards::SWATCHES.iter().filter_map(|material| picture(&library, material)));
    let sky = with_ext(".SKB").find_map(|f| crate::sky::Sky::parse(jam.get(f)?));
    Some((track, LoadedWorld { sky, surfaces, bricks, targets, routes, ghost, props, ..world }))
}

fn picture(library: &Library, material: &str) -> Option<(String, image::Pixels)> {
    Some((material.to_string(), library.texture(material)?))
}

/// Adds the emitters of an emitter file that pass `keep` and aren't there already,
/// with pictures from `library` and the material animation at `animation`.
fn add_emitters(
    jam: &Jam,
    file: &str,
    library: &Library,
    animation: &str,
    keep: impl Fn(&str) -> bool,
    emitters: &mut Vec<(String, particles::EmitterDef, particles::Look)>,
) {
    let animation = jam.get(animation).and_then(mab::MaterialAnimation::parse).unwrap_or_default();
    for (name, def) in jam.get(file).map(particles::parse).unwrap_or_default() {
        if !keep(&name) || emitters.iter().any(|e| e.0 == name) {
            continue;
        }
        let look = match (&def.material, def.track) {
            (Some(material), _) => particles::Look {
                frames: library.tinted(material).map(|p| (0, p)).into_iter().collect(),
                track: None,
                additive: library.additive(material),
            },
            (None, Some(track)) => particles::Look {
                frames: animation
                    .materials(track)
                    .iter()
                    .filter_map(|(material, frame)| Some((*frame, library.tinted(material)?)))
                    .collect(),
                track: animation.tracks.get(track).copied(),
                additive: animation.materials(track).first().is_some_and(|m| library.additive(&m.0)),
            },
            _ => particles::Look::default(),
        };
        if look.frames.is_empty() {
            warn!("no picture for the particles of {name}");
        }
        emitters.push((name, def, look));
    }
}

/// What a race has whatever its circuit: the field of `circuit` and its cars, the
/// power-ups' models and pictures, and the emitters every circuit shares, after the
/// ones given.
fn shared(jam: &Jam, circuit: &str, mut emitters: Vec<(String, particles::EmitterDef, particles::Look)>) -> LoadedWorld {
    // Karts are optional: without them the brick-built stand-ins are used.
    let field = roster::field(jam, circuit);
    let karts: Vec<KartModel> = field.iter().map_while(|driver| load_kart(jam, driver)).collect();
    if karts.len() != field.len() || karts.is_empty() {
        warn!("could not load the original kart models");
    }
    let record_holder = roster::driver(jam, roster::PLAYER);
    let ghost_models: Vec<KartModel> = (0..2).filter_map(|_| load_kart(jam, record_holder.as_ref()?)).collect();
    let powerups = ["POWERUP", "DTURBO0", "DTURBO1", "DTURBO2", "CURSE", "BARREL", "WARPHOLE", "CGREEN", "GRAPPLE", "DBRICKS", "DTUBE"];
    let powerups: Vec<String> = powerups.iter().flat_map(|n| [format!("{COMMON}/{n}.MDB"), format!("{COMMON}/{n}.TDB")]).collect();
    let powerups = Library::new(jam, powerups.iter().map(String::as_str), &[COMMON]);
    let files = [format!("{COMMON}/POWERUP.WDB"), format!("{COMMON}/TURBO3.WDB")];
    let models = scenery::load_files(jam, COMMON, &files.each_ref().map(String::as_str), &powerups, |_, _| true);
    let common = [format!("{COMMON}/EMITTER.MDB"), format!("{COMMON}/EMITTER.TDB")];
    let common = Library::new(jam, common.iter().map(String::as_str), &[COMMON]);
    add_emitters(jam, &format!("{COMMON}/EMITTER.EMB"), &common, &format!("{COMMON}/EMITTER.MAB"), |_| true, &mut emitters);
    let swatches = crate::item_models::PICTURES.iter().filter_map(|material| picture(&powerups, material)).collect();
    LoadedWorld {
        surfaces: Vec::new(),
        bricks: Vec::new(),
        targets: Vec::new(),
        karts,
        field,
        routes: Vec::new(),
        ghost: None,
        ghost_models,
        sky: None,
        props: Vec::new(),
        models,
        emitters,
        swatches,
    }
}

/// What a circuit of the port's own takes from the game's data: the cars and the
/// power-ups, and from each race named (a folder, then the names of models and of
/// emitters) those models, where their own circuit puts them, and those emitters.
pub fn borrowed(circuit: &str, races: &[(&str, &[&str], &[&str])]) -> Option<LoadedWorld> {
    let jam = open_jam()?;
    let (mut props, mut emitters) = (Vec::new(), Vec::new());
    for &(race, models, particles) in races {
        let dir = format!("/GAMEDATA/{race}");
        let mut files: Vec<&str> = jam.list(&dir).collect();
        files.sort();
        let with_ext = |ext: &'static str| files.iter().copied().filter(move |f| f.ends_with(ext));
        let library = Library::new(&jam, with_ext(".MDB").chain(with_ext(".TDB")), &[&dir, COMMON]);
        props.extend(scenery::load_named(&jam, &dir, &library, models));
        for file in with_ext(".EMB") {
            let animation = format!("{dir}/EMIT{}.MAB", &race[race.len().saturating_sub(4)..]);
            add_emitters(&jam, file, &library, &animation, |name| particles.contains(&name), &mut emitters);
        }
    }
    Some(LoadedWorld { props, ..shared(&jam, circuit, emitters) })
}

/// Render components for one surface.
pub fn surface_bundle(
    surface: Surface,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> (Mesh3d, MeshMaterial3d<StandardMaterial>) {
    let texture = surface.texture.map(|pixels| {
        let mut image = Image::new(
            Extent3d { width: pixels.width, height: pixels.height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            pixels.rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        images.add(image)
    });
    let material = StandardMaterial {
        base_color_texture: texture,
        unlit: true,
        // Winding isn't consistent enough across the original models to cull.
        cull_mode: None,
        double_sided: true,
        alpha_mode: if surface.additive {
            AlphaMode::Add
        } else if surface.blend {
            AlphaMode::Blend
        } else if surface.cutout {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Opaque
        },
        ..default()
    };
    (Mesh3d(meshes.add(surface.mesh)), MeshMaterial3d(materials.add(material)))
}

pub fn spawn_world(
    mut commands: Commands,
    mut world: ResMut<LoadedWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for surface in std::mem::take(&mut world.surfaces) {
        commands.spawn(surface_bundle(surface, &mut meshes, &mut materials, &mut images));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the original game data; silently passes without it.
    #[test]
    fn a_race_has_its_own_bricks_and_a_time_race_has_others() {
        for (race, name) in circuits() {
            let bricks = |time_race| load_in(&race, None, time_race).map(|(_, world)| world.bricks).unwrap_or_default();
            let (race_bricks, time_bricks) = (bricks(false), bricks(true));
            assert!(race_bricks.len() > 20, "{name}: {} bricks", race_bricks.len());
            // Fewer in a time race, and none at all where the original has no such file.
            assert!(time_bricks.len() < race_bricks.len(), "{name}: {} in a time race", time_bricks.len());
            // No brick sits inside another: the nearest pair is a kart's width apart.
            for (i, a) in race_bricks.iter().enumerate() {
                for b in &race_bricks[i + 1..] {
                    assert!(a.1.distance(b.1) > 3.0, "{name}: bricks at {} and {}", a.1, b.1);
                }
            }
        }
    }
}

/// Needs the original game data; silently passes without it. Everyone a player may
/// choose to race as online has a car to race in.
#[cfg(test)]
#[test]
fn every_driver_on_the_roster_can_be_raced_as() {
    let Some(jam) = open_jam() else { return };
    let missing: Vec<&str> = roster::NAMES.iter().map(|driver| driver.0).filter(|code| roster::driver(&jam, code).and_then(|driver| load_kart(&jam, &driver)).is_none()).collect();
    assert!(missing.is_empty(), "no car for {missing:?}");
    // And one put in another's place on a circuit's grid takes it.
    let Some((_, mut loaded)) = load_in("RACEC0R0", None, false) else { return };
    let before = loaded.field[4].code.clone();
    assert!(recast(&mut loaded, 4, "PH") && loaded.field[4].code != before && loaded.field[4].name == "Pharaoh Hotep");
    assert!(!recast(&mut loaded, 4, "nobody"));
}
