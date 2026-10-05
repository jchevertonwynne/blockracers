//! Showing a race that is stepped sixty times a second on a screen drawn at some
//! other rate. The port's own.
//!
//! A frame may fall anywhere between two steps, and a screen faster than the race
//! draws some steps twice. Shown as they stand, the cars would move in jerks against
//! a camera that moves every frame, which the eye sees as a blur. So for as long as a
//! frame is being drawn, each car is put where it was between its last two steps,
//! as far along as the frame is; before the race is next stepped it is put back.
//! Everything that looks at the cars while drawing (the camera, the shadows, the
//! map) sees them move evenly, and the race itself never sees the difference.

use bevy::prelude::*;

use crate::kart::Kart;

/// Farther than this in one step is a car being put somewhere, not driven there, and
/// is shown as it is.
const PUT: f32 = 8.0;

/// Where a car was at the end of each of the last two steps, and where it has been
/// put for drawing, if it has.
#[derive(Component)]
pub struct Shown {
    before: (Vec3, Quat),
    now: (Vec3, Quat),
    drawn: Option<(Vec3, Quat)>,
}

/// A step has ended: where the cars are now is what the next frames are drawn towards.
pub fn note(mut commands: Commands, mut karts: Query<(Entity, &Kart, Option<&mut Shown>)>) {
    for (entity, kart, shown) in &mut karts {
        let now = (kart.pos, kart.rot);
        match shown {
            Some(mut shown) => (shown.before, shown.now) = (shown.now, now),
            None => drop(commands.entity(entity).insert(Shown {
                before: now,
                now,
                drawn: None,
            })),
        }
    }
}

/// Puts the cars between their last two steps for the frame about to be drawn.
pub fn blend(time: Res<Time<Fixed>>, mut karts: Query<(&mut Kart, &mut Shown)>) {
    let along = time.overstep_fraction().clamp(0.0, 1.0);
    for (mut kart, mut shown) in &mut karts {
        let (before, now) = (shown.before, shown.now);
        let drawn = if before.0.distance_squared(now.0) > PUT * PUT {
            now
        } else {
            (before.0.lerp(now.0, along), before.1.slerp(now.1, along))
        };
        (kart.pos, kart.rot) = drawn;
        shown.drawn = Some(drawn);
    }
}

/// Puts the cars back where the race has them, before it is stepped again. Whatever
/// moved a car while it was being drawn (a hazard, say) has moved it for the race too.
pub fn unblend(mut karts: Query<(&mut Kart, &mut Shown)>) {
    for (mut kart, mut shown) in &mut karts {
        let Some(drawn) = shown.drawn.take() else {
            continue;
        };
        shown.now.0 += kart.pos - drawn.0;
        if kart.rot != drawn.1 {
            shown.now.1 = kart.rot;
        }
        (kart.pos, kart.rot) = shown.now;
    }
}
