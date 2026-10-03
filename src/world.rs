//! Builds a playable circuit out of the original game's data: the track model and its
//! textures for rendering, an AI route as the racing line, and the power-up bricks.

use crate::assets::{
    Jam,
    bvb::Volume,
    gdb::{Batch, Bone, Model, Vertex, parse_skeleton},
    image, materials, route,
    tokens::{Token, tokenize},
};
use crate::items::Power;
use crate::physics::UNIT;
use crate::track::Track;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use std::collections::HashMap;

const DEFAULT_JAM: &str = "Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM";
/// Imperial Grand Prix.
const DEFAULT_RACE: &str = "RACEC0R0";
/// Half-width of the band around the recorded racing line that the AI may use.
const LANE: f32 = 4.0;

/// The game is Z-up; this is a pure rotation onto Bevy's Y-up axes.
fn to_world(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1]) * UNIT
}

pub struct Surface {
    mesh: Mesh,
    texture: Option<image::Pixels>,
    cutout: bool,
    blend: bool,
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
    /// Where the driver sits, in game units relative to the car.
    pub mount: Vec3,
}

#[derive(Resource)]
pub struct LoadedWorld {
    surfaces: Vec<Surface>,
    pub bricks: Vec<(Option<Power>, Vec3)>,
    /// One per grid slot, in the order of the driver roster.
    pub karts: Vec<KartModel>,
}

/// Materials and texture definitions, plus where to look for the textures themselves.
struct Library<'a> {
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

    /// One mesh per material out of the batches of `model` that pass `keep`.
    fn surfaces(
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
            let texture = self
                .dirs
                .iter()
                .find_map(|dir| self.jam.get(&format!("{dir}/{texture_name}.BMP")))
                .and_then(|d| image::decode_bmp(d, definition.color_key))
                .map(|mut pixels| {
                    if definition.flip {
                        let row = pixels.width as usize * 4;
                        pixels.rgba = pixels.rgba.chunks(row).rev().flatten().copied().collect();
                    }
                    pixels
                });

            let vertex = |&i: &u32| model.vertices[i as usize];
            // Lighting is baked into vertex colours, where 0x80 is full brightness.
            let colour = |v: Vertex| {
                let c = |x: u8| (x as f32 / 127.5).powf(2.2);
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
                blend: info.is_some_and(|m| m.blend),
            });
        }
        surfaces
    }
}

const COMMON: &str = "/GAMEDATA/COMMON";

/// A bone's rotation and position in model space.
fn bone_pose(bones: &[Bone], index: usize) -> (Quat, Vec3) {
    let bone = &bones[index];
    let (rotation, position) = (Quat::from_array(bone.rotation), Vec3::from(bone.position));
    match bone.parent {
        Some(parent) => {
            let (parent_rotation, parent_position) = bone_pose(bones, parent);
            (parent_rotation * rotation, parent_position + parent_rotation * position)
        }
        None => (rotation, position),
    }
}

/// Loads the car of the champion whose files start with `prefix` ("rr" for Rocket Racer).
fn load_kart(jam: &Jam, prefix: &str) -> Option<KartModel> {
    let part = |suffix: &str| {
        let file = |ext: &str| format!("{COMMON}/{prefix}{suffix}.{ext}");
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

    let (body, library) = part("CM")?;
    let body_surfaces = library.surfaces(&body, |_| true, Vec3::from);

    let (wheels, library) = part("JMW")?;
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
    let (driver, library) = part("PELVIS")?;
    let skeleton = parse_skeleton(jam.get(&format!("{COMMON}/PELVIS.SDB"))?)?;
    let mut driver_surfaces = Vec::new();
    for bone in bones_used(&driver) {
        let (rotation, position) = bone_pose(&skeleton, bone);
        let place = |p: [f32; 3]| position + rotation * Vec3::from(p);
        driver_surfaces.extend(library.surfaces(&driver, |b| b.bone == Some(bone), place));
    }

    Some(KartModel {
        body: body_surfaces,
        body_scale: body.scale,
        axles,
        wheel_scale: wheels.scale,
        driver: driver_surfaces,
        driver_scale: driver.scale,
        mount: driver_mount(jam, prefix).unwrap_or(Vec3::Z * 0.8),
    })
}

/// The driver's seat position from the chassis table.
fn driver_mount(jam: &Jam, prefix: &str) -> Option<Vec3> {
    let tokens = tokenize(jam.get(&format!("{COMMON}/CHASSIS.CMB"))?);
    let start = tokens.iter().position(
        |t| matches!(t, Token::Str(name) if name.starts_with(prefix) && name[prefix.len()..].starts_with("cha")),
    )?;
    let at = start + tokens[start..].iter().position(|t| *t == Token::Key(0x2b))?;
    match tokens.get(at + 1..at + 4)? {
        [Token::Float(x), Token::Float(y), Token::Float(z)] => Some(Vec3::new(*x, *y, *z)),
        _ => None,
    }
}

/// Champions whose cars the six racers drive, in roster order.
const KART_PREFIXES: [&str; 6] = ["rr", "cr", "kk", "bb", "jt", "vv"];

/// Loads the race named by `$LEGO_RACE` from the archive at `$LEGO_JAM`, falling back
/// to the defaults above. `None` if the game data isn't there.
pub fn load() -> Option<(Track, LoadedWorld)> {
    let jam = Jam::open(std::env::var("LEGO_JAM").unwrap_or(DEFAULT_JAM.into()))?;
    let race = std::env::var("LEGO_RACE").unwrap_or(DEFAULT_RACE.into());
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
    let lap = route::Route::parse(jam.get(route_file)?)?.lap;
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
    for tri in &volume.triangles {
        let name = volume.materials.get(tri[3] as usize);
        let surface = name.and_then(|n| surface_table.get(n)).copied().unwrap_or_default();
        if !surface.non_solid {
            let corner = |i: u16| volume.vertices.get(i as usize).copied().map(to_world);
            track.collision.add([corner(tri[0])?, corner(tri[1])?, corner(tri[2])?], surface);
        }
    }

    let bricks = with_ext(".PWB")
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

    info!("loaded {race}: {model_file}, {route_file}, lap {:.0}", track.length);
    // Karts are optional: without them the brick-built stand-ins are used.
    let karts: Vec<KartModel> = KART_PREFIXES.iter().map_while(|p| load_kart(&jam, p)).collect();
    if karts.len() != KART_PREFIXES.len() {
        warn!("could not load the original kart models");
    }
    Some((track, LoadedWorld { surfaces, bricks, karts }))
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
        alpha_mode: if surface.blend {
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
