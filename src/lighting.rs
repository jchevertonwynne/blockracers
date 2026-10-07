//! Lights, for the models the original lights as it draws them. Follows
//! `GolModelRenderState::UpdateMaterialCaches` and `LightVertices1` onward (and the
//! device's `ClearLights`, `SetAmbient`, `AddLight`, which the D3D path does the same
//! with): a vertex that has a normal is given
//!
//! `ambient light * material ambient + sum of max(0, -normal . direction) * light * material diffuse`
//!
//! as its colour, and a model's vertices that have none keep the colours baked into
//! them, which no light touches. With no ambient light set and a light cast, the
//! ambient is full (`ClearLights`); with neither, lighting is off and a model is drawn
//! as it was made.
//!
//! Only the models of the menus' films have normals, so only those are lit here: a
//! race is lit by nothing, as the original's is. A model's lights are the scene's, in
//! the game's axes; the normals are turned with the model each frame, as
//! `GolD3DRenderDevice` turns the lights into model space.

use bevy::prelude::*;

use crate::scenery::to_world;

/// A directional light: the way it travels, in the game's axes, and its colour.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Beam {
    pub direction: Vec3,
    pub colour: Vec3,
}

/// The lights of a scene: the ambient colour, if one is set, and the lights cast
/// (at most seven, as the original's are). Colours are of nought to one.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Lights {
    pub ambient: Option<Vec3>,
    pub beams: Vec<Beam>,
}

impl Lights {
    /// Whether anything lights a model.
    pub fn on(&self) -> bool {
        self.ambient.is_some() || !self.beams.is_empty()
    }
}

/// A mesh whose vertices are lit by the scene's lights.
#[derive(Component, Clone)]
pub struct Lit {
    /// A normal to each vertex, in the model's own axes.
    pub normals: Vec<Vec3>,
    /// The colours the vertices have with nothing to light them, and so the alpha.
    pub base: Vec<[f32; 4]>,
    /// The material's ambient and diffuse colours, of nought to one.
    pub ambient: Vec3,
    pub diffuse: Vec3,
    /// The lights and the turn the colours were last made for.
    seen: Option<(Lights, Quat)>,
}

impl Lit {
    pub fn new(normals: Vec<Vec3>, base: Vec<[f32; 4]>, ambient: Vec3, diffuse: Vec3) -> Lit {
        Lit {
            normals,
            base,
            ambient,
            diffuse,
            seen: None,
        }
    }

    /// The colour of each vertex, for a model turned by `turn` into the world.
    pub fn colours(&self, lights: &Lights, turn: Quat) -> Vec<[f32; 4]> {
        if !lights.on() {
            return self.base.clone();
        }
        let ambient = lights.ambient.unwrap_or(Vec3::ONE) * self.ambient;
        // The lights' directions, in the model's own axes.
        let toward = turn.inverse();
        let beams: Vec<(Vec3, Vec3)> = lights
            .beams
            .iter()
            .map(|beam| {
                let world = to_world(beam.direction).normalize_or_zero();
                (toward * world, beam.colour * self.diffuse)
            })
            .collect();
        self.normals
            .iter()
            .zip(&self.base)
            .map(|(normal, base)| {
                let mut colour = ambient;
                for (direction, product) in &beams {
                    colour += *product * (-normal.dot(*direction)).max(0.0);
                }
                let colour = colour.min(Vec3::ONE);
                // The colours of a mesh are linear, as `Library::meshes` makes them.
                let linear = |value: f32| value.powf(2.2);
                [linear(colour.x), linear(colour.y), linear(colour.z), base[3]]
            })
            .collect()
    }
}

/// Lights the vertices of the meshes that are lit again when the lights change or
/// the model turns.
fn relight(
    lights: Option<Res<Lights>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut lit: Query<(&mut Lit, &Mesh3d, &GlobalTransform)>,
) {
    let lights = lights.map(|lights| lights.clone()).unwrap_or_default();
    for (mut lit, mesh, transform) in &mut lit {
        let turn = transform.rotation();
        let stale = match &lit.seen {
            Some((was, at)) => *was != lights || at.angle_between(turn) > 1e-3,
            None => true,
        };
        if !stale {
            continue;
        }
        let colours = lit.colours(&lights, turn);
        lit.seen = Some((lights.clone(), turn));
        if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Lights>()
        .add_systems(PostUpdate, relight.after(TransformSystems::Propagate));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit() -> Lit {
        // One face looking along +Z, one along -Z (in the game's axes).
        let up = Vec3::Z;
        Lit::new(
            vec![up, -up],
            vec![[1.0; 4]; 2],
            Vec3::ONE,
            Vec3::splat(0.5),
        )
    }

    fn grey(colour: [f32; 4]) -> f32 {
        colour[0].powf(1.0 / 2.2)
    }

    #[test]
    fn a_vertex_is_lit_by_the_light_that_shines_on_it() {
        // A light going down, on a model that is not turned (the world's -Y being
        // the game's -Z).
        let lights = Lights {
            ambient: Some(Vec3::splat(0.25)),
            beams: vec![Beam {
                direction: Vec3::NEG_Z,
                colour: Vec3::ONE,
            }],
        };
        let [top, bottom] = lit().colours(&lights, crate::scenery::basis())[..] else {
            panic!("two vertices");
        };
        // Facing the light: ambient and half the light. Facing away: ambient alone.
        assert!((grey(top) - 0.75).abs() < 1e-3, "{top:?}");
        assert!((grey(bottom) - 0.25).abs() < 1e-3, "{bottom:?}");
    }

    #[test]
    fn no_lights_leave_the_colours_as_made_and_a_light_alone_has_full_ambient() {
        let none = Lights::default();
        assert_eq!(lit().colours(&none, Quat::IDENTITY), vec![[1.0; 4]; 2]);
        let alone = Lights {
            ambient: None,
            beams: vec![Beam {
                direction: Vec3::NEG_Z,
                colour: Vec3::splat(0.2),
            }],
        };
        let [_, bottom] = lit().colours(&alone, crate::scenery::basis())[..] else {
            panic!("two vertices");
        };
        assert!((grey(bottom) - 1.0).abs() < 1e-3);
    }
}
