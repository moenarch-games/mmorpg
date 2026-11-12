//! Game-owned movement intent and rigid-piece policies shared by both backends.

use super::{BodyId, CONTACT_OFFSET_UNITS, DebrisBody, Kind, PieceMedium, RigidBody, Vec3i};
use std::collections::BTreeMap;

/// Approaches within 60 degrees of head-on step fully aside in [`slide_around`].
const NEAR_HEAD_ON_COS: f32 = 0.5;
/// The outward motion (units per tick: 30 units/s) [`slide_around`] adds against a
/// touching body.
const SEPARATION_UNITS: f32 = 30.0 / crate::TICK_HZ as f32;
/// A boulder's damping (#299): rolling loses speed to it harder than a sliding box does.
pub(super) const BOULDER_LINEAR_DAMPING: f32 = 0.4;
pub(super) const BOULDER_ANGULAR_DAMPING: f32 = 1.2;
/// Hard rock (kg/m³); mass only matters between dynamic pieces.
pub(super) const BOULDER_DENSITY: f32 = 2_400.0;
/// A piece slower than this (units per tick: 18 units/s) and turning slower than
/// [`DEBRIS_REST_SPIN`] (radians per tick: 0.9 rad/s) settles.
pub(super) const DEBRIS_REST_SPEED_UNITS: f32 = 18.0 / crate::TICK_HZ as f32;
pub(super) const DEBRIS_REST_SPIN: f32 = 0.9 / crate::TICK_HZ as f32;
/// In water (#667) a piece settles only once it is slower than this (units per tick: 2 units/s)
/// and its velocity changes less than [`WATER_REST_CHANGE_UNITS`] over a step (1 unit/s, an
/// acceleration of 0.6 m/s²): a floating piece drifting toward its depth, or bobbing through a
/// turning point, does not freeze short of where buoyancy, weight and contact balance.
pub(super) const WATER_REST_SPEED_UNITS: f32 = 2.0 / crate::TICK_HZ as f32;
pub(super) const WATER_REST_CHANGE_UNITS: f32 = 1.0 / crate::TICK_HZ as f32;

/// Whether a just-stepped piece, moving at `before` ahead of the step, settles: it may, it is
/// slower than [`DEBRIS_REST_SPEED_UNITS`] and turns slower than [`DEBRIS_REST_SPIN`], and in
/// water it has all but stopped ([`WATER_REST_SPEED_UNITS`], [`WATER_REST_CHANGE_UNITS`]).
pub(super) fn comes_to_rest(
    piece: &DebrisBody,
    before: [f32; 3],
    settles: bool,
    in_water: bool,
) -> bool {
    let length = |v: [f32; 3]| v.iter().map(|v| v * v).sum::<f32>().sqrt();
    let change = [0, 1, 2].map(|axis| piece.velocity[axis] - before[axis]);
    settles
        && length(piece.velocity) < DEBRIS_REST_SPEED_UNITS
        && length(piece.spin) < DEBRIS_REST_SPIN
        && (!in_water
            || (length(piece.velocity) < WATER_REST_SPEED_UNITS
                && length(change) < WATER_REST_CHANGE_UNITS))
}

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
    /// Each piece's water (#667), parallel to the pieces; empty puts none in water.
    pub media: &'a [Option<PieceMedium>],
}

/// Whether `piece` lies over ground that dips below the floor half-space (a brook bed or lake
/// basin, #667): the same footprint test units use, with the piece's reach in any turn. There it
/// passes through the floor and only the terrain holds it, so it can sink into the water.
pub(super) fn over_open_floor(terrain: Option<&crate::Terrain>, piece: &DebrisBody) -> bool {
    let Some(terrain) = terrain else {
        return false;
    };
    #[allow(clippy::cast_possible_truncation)]
    let (x, z) = (
        piece.position[0].round() as i32,
        piece.position[2].round() as i32,
    );
    let half = piece.half_extents;
    terrain.low_within(x, z, half.x + half.y + half.z + 1) < 0
}

impl Dynamic<'_> {
    /// Piece `index`'s water, if any.
    pub fn medium(&self, index: usize) -> Option<PieceMedium> {
        self.media.get(index).copied().flatten()
    }
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
