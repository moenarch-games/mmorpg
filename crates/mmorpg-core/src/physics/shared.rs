//! Game-owned movement intent and rigid-piece policies shared by both backends.

use super::{BodyId, CONTACT_OFFSET_UNITS, DebrisBody, Kind, RigidBody, Vec3i};
use std::collections::BTreeMap;

/// Approaches within 60 degrees of head-on step fully aside in [`slide_around`].
const NEAR_HEAD_ON_COS: f32 = 0.5;
/// The outward motion (units per tick: 30 units/s) [`slide_around`] adds against a
/// touching body.
const SEPARATION_UNITS: f32 = 30.0 / crate::TICK_HZ as f32;
/// A piece slower than this (units per tick: 18 units/s) and turning slower than
/// [`DEBRIS_REST_SPIN`] (radians per tick: 0.9 rad/s) settles.
pub(super) const DEBRIS_REST_SPEED_UNITS: f32 = 18.0 / crate::TICK_HZ as f32;
pub(super) const DEBRIS_REST_SPIN: f32 = 0.9 / crate::TICK_HZ as f32;
/// Fixed geometry this far (units) around the pieces takes part in a debris step.
pub(super) const DEBRIS_REACH_UNITS: f32 = 1_000.0;
pub(super) const TICKS_PER_SECOND: f32 = crate::TICK_HZ as f32;

/// `desired` with the horizontal part that presses into a touching moving capsule turned
/// along it, so characters slide around each other instead of locking head-on: Rapier's
/// controller only stops a kinematic body at another (#273; physics-engine #308 will do
/// this natively). Within 60 degrees of head-on, both step aside to opposite sides at full
/// speed. The engine arm turns its moves the same way, so the two agree.
pub(super) fn slide_around<'a>(
    bodies: impl IntoIterator<Item = &'a RigidBody>,
    body: &RigidBody,
    desired: [f32; 3],
) -> [f32; 3] {
    let [mut vx, vy, mut vz] = desired;
    let speed = vx.hypot(vz);
    if speed == 0.0 {
        return desired;
    }
    let radius = |half: Vec3i| super::capsule_dimensions(half).1;
    #[allow(clippy::cast_precision_loss)]
    let (low, high) = (
        body.position[1] - body.half_extents.y as f32,
        body.position[1] + body.half_extents.y as f32,
    );
    for other in bodies {
        if other.id == body.id || !other.is_moving() {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let other_half_y = other.half_extents.y as f32;
        if other.position[1] + other_half_y < low || other.position[1] - other_half_y > high {
            continue;
        }
        let (dx, dz) = (
            body.position[0] - other.position[0],
            body.position[2] - other.position[2],
        );
        let distance = dx.hypot(dz);
        let reach =
            radius(body.half_extents) + radius(other.half_extents) + CONTACT_OFFSET_UNITS + 1.0;
        if distance == 0.0 || distance > reach {
            continue;
        }
        // `n` points from the other body to this one; only motion against it is turned.
        let (nx, nz) = (dx / distance, dz / distance);
        let into = vx * nx + vz * nz;
        if into >= 0.0 {
            continue;
        }
        // The tangent (-nz, nx) is mirrored for the other body, whose `n` is opposite.
        let (tx, tz) = (-nz, nx);
        let along = vx * tx + vz * tz;
        let along = if into < -NEAR_HEAD_ON_COS * speed {
            // Near head-on, projecting would leave a crawl: step aside at full speed, to
            // the side the motion already leans or, exactly head-on, the tangent's side
            // (opposite for the two bodies, so they never pick the same one).
            let side = if along.abs() > 1.0e-3 * speed {
                along.signum()
            } else {
                1.0
            };
            side * speed
        } else {
            along
        };
        // A little outward motion too: against a touching capsule Rapier's controller
        // otherwise stops even motion along it.
        vx = along * tx + SEPARATION_UNITS * nx;
        vz = along * tz + SEPARATION_UNITS * nz;
    }
    [vx, vy, vz]
}

/// How a dynamic step treats its pieces.
#[derive(Default)]
pub(super) struct Dynamic<'a> {
    /// Pieces keep upright: they slide but never turn (movable props, #58).
    pub upright: bool,
    /// Units' capsules stop the pieces (movable props), instead of letting them pass (debris).
    pub units_block: bool,
    /// Bodies left out of the step: the movable props' own fixed boxes.
    pub exclude: &'a [BodyId],
}

/// The game bodies a dynamic step takes part with: the floors and everything within
/// [`DEBRIS_REACH_UNITS`] of a piece, units only when they block, minus `options.exclude`.
pub(super) fn nearby<'a>(
    bodies: &'a BTreeMap<BodyId, RigidBody>,
    debris: &[DebrisBody],
    options: &'a Dynamic<'_>,
) -> impl Iterator<Item = &'a RigidBody> {
    let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
    for piece in debris {
        for axis in 0..3 {
            low[axis] = low[axis].min(piece.position[axis] - DEBRIS_REACH_UNITS);
            high[axis] = high[axis].max(piece.position[axis] + DEBRIS_REACH_UNITS);
        }
    }
    bodies
        .values()
        .filter(|body| options.units_block || !body.is_moving())
        .filter(|body| !options.exclude.contains(&body.id))
        .filter(move |body| {
            #[allow(clippy::cast_precision_loss)]
            let near = matches!(body.kind, Kind::Floor)
                || (0..3).all(|axis| {
                    let half = [
                        body.half_extents.x,
                        body.half_extents.y,
                        body.half_extents.z,
                    ][axis] as f32;
                    body.position[axis] + half >= low[axis]
                        && body.position[axis] - half <= high[axis]
                });
            near
        })
}
