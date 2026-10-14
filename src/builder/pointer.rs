//! The pointer as the builder has it: where it is, which of its buttons are down
//! and how far its wheel has turned, read once a frame from the mouse, or from a
//! script, which is how a demo, having no mouse, is shown the builder being used.
//! The port's own.
//!
//! `BRICK_POINTER=600:400@1,down@1.2,g120:-40@2.5,up@2.6` is such a script: each
//! step is something and the second it happens at. `x:y` is where the pointer has
//! come to by then, in the window's own units from its top left corner, going
//! there evenly from where it was at the step before; `gx:z` is the same of a
//! place on the map's ground, wherever on the window that then is (`gx:z:y` of
//! a place so high over it), and `[Keep]`
//! of whatever on the panels says that, `[+#2]` being the third from the top of
//! those that say `+`; `down` and `up` are the left button, `rdown` and `rup` the
//! right, `mdown` and `mup` the middle, and `wheel:-3` turns the wheel so many
//! notches, up being nearer. While there is a script the mouse itself is not
//! looked at.

use bevy::camera::visibility::RenderLayers;
use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// How many of a trackpad's units of scrolling are a notch of a wheel, and how
/// much of a pinch is.
const NOTCH: f32 = 40.0;
const PINCH: f32 = 0.1;

/// One of the pointer's buttons: whether it is down, and whether it went down or
/// came up this frame.
#[derive(Default, Clone, Copy)]
pub struct Button {
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
}

impl Button {
    fn set(&mut self, down: bool) {
        (self.pressed, self.released) = (down && !self.down, !down && self.down);
        self.down = down;
    }
}

#[derive(Resource, Default)]
pub struct Pointer {
    /// Where it is on the window, in the window's own units, if it is over it.
    pub at: Option<Vec2>,
    /// How far it has moved since the frame before.
    pub moved: Vec2,
    pub left: Button,
    pub right: Button,
    pub middle: Button,
    /// How many notches the wheel has turned this frame, up being nearer.
    pub wheel: f32,
    /// A panel has the pointer this frame, and the map under it is left alone.
    pub taken: bool,
}

/// Somewhere a script has the pointer go: a place on the window, one on the
/// ground, or whatever on the panels has these words, and which of those that
/// have, from the top.
#[derive(Clone, PartialEq, Debug)]
enum Place {
    Window(Vec2),
    Ground(Vec3),
    Words(String, usize),
}

enum Step {
    To(Place),
    Left(bool),
    Right(bool),
    Middle(bool),
    Wheel(f32),
}

/// What a demo has the pointer do, and how far through it the demo is.
#[derive(Resource)]
pub struct Script {
    steps: Vec<(f32, Step)>,
    done: usize,
    /// When the last step done was, and where the pointer then was.
    since: (f32, Option<Place>),
    buttons: [bool; 3],
}

impl Script {
    /// The script `$BRICK_POINTER` has, if it has one.
    pub fn from_env() -> Option<Script> {
        Script::read(&std::env::var("BRICK_POINTER").ok()?)
    }

    fn read(script: &str) -> Option<Script> {
        let steps: Vec<(f32, Step)> = script
            .split(',')
            .filter_map(|step| {
                let (what, at) = step.trim().rsplit_once('@')?;
                let words = what.strip_prefix('[').and_then(|what| what.strip_suffix(']'));
                let step = match what {
                    _ if words.is_some() => {
                        let (words, which) = words?.rsplit_once('#').unwrap_or((words?, "0"));
                        Step::To(Place::Words(words.into(), which.parse().ok()?))
                    }
                    "down" => Step::Left(true),
                    "up" => Step::Left(false),
                    "rdown" => Step::Right(true),
                    "rup" => Step::Right(false),
                    "mdown" => Step::Middle(true),
                    "mup" => Step::Middle(false),
                    what => match what.split_once(':')? {
                        ("wheel", notches) => Step::Wheel(notches.parse().ok()?),
                        (x, y) => match x.strip_prefix('g') {
                            Some(x) => {
                                let (z, y) = y.split_once(':').unwrap_or((y, "0"));
                                Step::To(Place::Ground(Vec3::new(x.parse().ok()?, y.parse().ok()?, z.parse().ok()?)))
                            }
                            None => Step::To(Place::Window(Vec2::new(x.parse().ok()?, y.parse().ok()?))),
                        },
                    },
                };
                Some((at.parse().ok()?, step))
            })
            .collect();
        (!steps.is_empty()).then_some(Script { steps, done: 0, since: (0.0, None), buttons: [false; 3] })
    }

    /// Where the pointer is `now`, which buttons are down, and how far the wheel
    /// has turned since this was last asked. `seen` is where on the window a
    /// place that is not on the window is.
    fn at(&mut self, now: f32, seen: impl Fn(&Place) -> Option<Vec2>) -> (Option<Vec2>, [bool; 3], f32) {
        let on_window = |place: &Place| match place {
            Place::Window(at) => Some(*at),
            place => seen(place),
        };
        let mut wheel = 0.0;
        // A button pressed or let go is where the pointer was when it was, however
        // long the frame: so the pointer is no further on than that this frame.
        let mut now = now;
        while let Some((when, step)) = self.steps.get(self.done).filter(|step| step.0 <= now) {
            match step {
                Step::To(at) => self.since.1 = Some(at.clone()),
                Step::Left(down) => self.buttons[0] = *down,
                Step::Right(down) => self.buttons[1] = *down,
                Step::Middle(down) => self.buttons[2] = *down,
                Step::Wheel(notches) => wheel += notches,
            }
            self.since.0 = *when;
            self.done += 1;
            // A button that has gone down is seen down before it comes up again.
            if !matches!(step, Step::To(_)) {
                now = *when;
                break;
            }
        }
        // On the way to wherever it is next to be.
        let at = match (self.steps.get(self.done), &self.since) {
            (Some((when, Step::To(to))), (since, Some(from))) => {
                let part = ((now - since) / (when - since).max(1e-3)).clamp(0.0, 1.0);
                on_window(from).zip(on_window(to)).map(|(from, to)| from.lerp(to, part))
            }
            (_, (_, at)) => at.as_ref().and_then(on_window),
        };
        (at, self.buttons, wheel)
    }
}

/// Reads the pointer for this frame: from the script if there is one, and from the
/// mouse if not.
pub fn read(
    mut pointer: ResMut<Pointer>,
    script: Option<ResMut<Script>>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut pinches: MessageReader<PinchGesture>,
    camera: Single<(&Camera, &GlobalTransform), (With<Camera3d>, Without<RenderLayers>)>,
    cells: Query<(&Text, &ComputedNode, &UiGlobalTransform), With<super::panel::Spot>>,
) {
    let was = pointer.at;
    let (at, down, wheel) = match script {
        Some(mut script) => {
            let (camera, eye) = *camera;
            script.at(time.elapsed_secs(), |place| match place {
                Place::Window(at) => Some(*at),
                Place::Ground(at) => camera.world_to_viewport(eye, *at).ok(),
                // The middle of whichever cell says so, counting from the top
                // and then from the left.
                Place::Words(words, which) => {
                    let saying = cells.iter().filter(|(text, node, _)| text.0 == *words && node.size().x > 0.0);
                    let mut middles: Vec<Vec2> = saying.map(|(_, _, place)| place.translation / window.scale_factor()).collect();
                    middles.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).unwrap_or(std::cmp::Ordering::Equal));
                    middles.get(*which).copied()
                }
            })
        }
        None => {
            let down = [MouseButton::Left, MouseButton::Right, MouseButton::Middle].map(|button| buttons.pressed(button));
            // A wheel turns by the notch, and a trackpad by a great many little units.
            let turned = match scroll.unit {
                MouseScrollUnit::Line => scroll.delta.y,
                MouseScrollUnit::Pixel => scroll.delta.y / NOTCH,
            };
            let pinched: f32 = pinches.read().map(|pinch| pinch.0 / PINCH).sum();
            (window.cursor_position(), down, turned + pinched)
        }
    };
    pointer.moved = match (was, at) {
        (Some(was), Some(at)) => at - was,
        _ => Vec2::ZERO,
    };
    pointer.at = at;
    pointer.left.set(down[0]);
    pointer.right.set(down[1]);
    pointer.middle.set(down[2]);
    (pointer.wheel, pointer.taken) = (wheel.clamp(-6.0, 6.0), false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_moves_the_pointer_evenly_and_presses_its_buttons() {
        let mut script = Script::read("100:100@1,down@1.5,300:100@2.5,up@2.5,wheel:-2@3,nonsense,rdown@4,g5:7@5").unwrap();
        // A place on the ground is, here, ten times as far across the window.
        let seen = |place: &Place| match place {
            Place::Ground(at) => Some(Vec2::new(at.x, at.z) * 10.0),
            _ => None,
        };
        assert_eq!(script.at(0.5, seen).0, None);
        assert_eq!(script.at(1.0, seen), (Some(Vec2::new(100.0, 100.0)), [false; 3], 0.0));
        // It stays where it is until the button is down, and then goes on.
        assert_eq!(script.at(1.4, seen).0, Some(Vec2::new(100.0, 100.0)));
        assert_eq!(script.at(1.5, seen).1, [true, false, false]);
        let (at, down, _) = script.at(2.0, seen);
        assert!(down[0] && (at.unwrap().x - 200.0).abs() < 0.1);
        // The button comes up where the pointer has got to.
        assert_eq!(script.at(2.6, seen), (Some(Vec2::new(300.0, 100.0)), [false; 3], 0.0));
        assert_eq!(script.at(3.1, seen).2, -2.0);
        assert_eq!(script.at(4.2, seen).1, [false, true, false]);
        // On its way to a place on the ground, and there.
        assert_eq!(script.at(4.5, seen).0, Some(Vec2::new(175.0, 85.0)));
        assert_eq!(script.at(5.5, seen).0, Some(Vec2::new(50.0, 70.0)));
        // However long a frame is, a button goes down and comes up where it was
        // meant to, and the pointer goes on afterwards.
        let mut script = Script::read("10:10@1,down@1.1,up@1.2,90:10@2").unwrap();
        assert_eq!(script.at(1.9, seen), (Some(Vec2::new(10.0, 10.0)), [true, false, false], 0.0));
        assert_eq!(script.at(1.95, seen), (Some(Vec2::new(10.0, 10.0)), [false; 3], 0.0));
        assert_eq!(script.at(2.0, seen).0, Some(Vec2::new(90.0, 10.0)));
        // Something on a panel is named by what it says.
        let script = Script::read("[Try it]@1,[+#2]@2").unwrap();
        let places: Vec<&Place> = script.steps.iter().filter_map(|step| match &step.1 {
            Step::To(place) => Some(place),
            _ => None,
        }).collect();
        assert_eq!(places, [&Place::Words("Try it".into(), 0), &Place::Words("+".into(), 2)]);
    }
}
