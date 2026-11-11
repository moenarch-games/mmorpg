//! How a character's body moves: run and jump speeds, the dodge roll, climbing and surface
//! hazards. Built on `sim-foundation`; it knows nothing of combat, zones or authored content.

pub mod climb;
pub mod roll;
pub mod surface;

pub use roll::Roll;

use sim_foundation::collision::{
    self, MAX_CONTENT_COORDINATE_UNITS, MAX_STATIC_COLLIDERS, XzBounds,
};
use sim_foundation::{TICK_HZ, ZoneError, entity, physics, subunits_per_tick, ticks_from_ms, trig};

/// Character collision box: 0.6 m × 1.8 m × 0.6 m, shared with clients.
pub const PLAYER_HALF_EXTENTS_UNITS: [i32; 3] = [30, 90, 30];
/// Horizontal speed for forward, strafe and forward-diagonal intent (6.3 m/s).
pub const RUN_SPEED_UNITS_PER_SECOND: i32 = 630;
pub const RUN_SPEED_SUBUNITS_PER_TICK: i32 = subunits_per_tick(RUN_SPEED_UNITS_PER_SECOND);
/// Horizontal speed whenever intent has a backward component (3.9 m/s).
pub const BACKPEDAL_SPEED_UNITS_PER_SECOND: i32 = 390;
pub const BACKPEDAL_SPEED_SUBUNITS_PER_TICK: i32 =
    subunits_per_tick(BACKPEDAL_SPEED_UNITS_PER_SECOND);
/// Upward velocity a grounded jump sets before physics applies gravity (4.8 m/s).
pub const JUMP_VELOCITY_UNITS_PER_SECOND: i32 = 480;
pub const JUMP_VELOCITY_SUBUNITS_PER_TICK: i32 = subunits_per_tick(JUMP_VELOCITY_UNITS_PER_SECOND);

/// Scales a horizontal velocity by `percent`, rounding toward zero.
#[must_use]
pub fn scale_velocity(velocity: [i32; 2], percent: i32) -> [i32; 2] {
    velocity.map(|component| {
        i32::try_from(i64::from(component) * i64::from(percent) / 100).unwrap_or(0)
    })
}
