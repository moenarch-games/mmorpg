//! The lowest layer of the game simulation: fixed-point maths, seeded randomness, tick timing,
//! unit identity, collision geometry and the physics world. It knows nothing of combat, zones,
//! quests or authored content, so every game built on the simulation can share it.

pub mod collision;
pub mod entity;
pub mod physics;
pub mod ramp;
pub mod rng;
pub mod rock;
pub mod static_mesh;
pub mod trig;

pub use collision::{StaticCollider, Terrain};
pub use ramp::{Ramp, RampRise};
pub use rock::RockShape;
pub use static_mesh::StaticMesh;

use std::error::Error;
use std::fmt;

pub type PlayerId = u32;

/// The authoritative simulation rate. Durations are authored in milliseconds and
/// speeds in units per second; the helpers below are the only place they become
/// ticks, and the handshake and both clients read this constant.
pub const TICK_HZ: u16 = 60;

/// Whole ticks in `ms` milliseconds at [`TICK_HZ`], rounded to the nearest tick
/// (halves up): 1 tick lasts 16⅔ ms at 60 Hz, so a duration that is not a
/// multiple of 50 ms is authored as its nearest whole millisecond. A nonzero
/// duration shorter than half a tick, or one longer than `u16::MAX` ticks, fails
/// to compile in const contexts.
#[must_use]
pub const fn ticks_from_ms(ms: u32) -> u16 {
    let ticks = long_ticks_from_ms(ms);
    assert!(ticks <= u16::MAX as u32, "duration is too long");
    ticks as u16
}

/// [`ticks_from_ms`] for the long `u32` timers (zone day, respawns, corpses).
#[must_use]
pub const fn long_ticks_from_ms(ms: u32) -> u32 {
    let ticks = (ms as u64 * TICK_HZ as u64 + 500) / 1_000;
    assert!(ms == 0 || ticks > 0, "duration is shorter than half a tick");
    assert!(ticks <= u32::MAX as u64, "duration is too long");
    ticks as u32
}

/// Units per tick of a speed authored in units per second. A speed that is not
/// a whole number of units per tick fails to compile in const contexts.
#[must_use]
pub const fn units_per_tick(units_per_second: i32) -> i32 {
    assert!(
        units_per_second % TICK_HZ as i32 == 0,
        "speed is not a whole number of units per tick"
    );
    units_per_second / TICK_HZ as i32
}

/// Subunits ([`physics::SUBUNITS_PER_UNIT`]) per tick² of an acceleration authored in units
/// per second². One that is not a whole number of subunits per tick² fails to compile in const
/// contexts.
#[must_use]
pub const fn subunits_per_tick_squared(units_per_second_squared: i32) -> i32 {
    let subunits = units_per_second_squared * physics::SUBUNITS_PER_UNIT;
    let ticks_squared = TICK_HZ as i32 * TICK_HZ as i32;
    assert!(
        subunits % ticks_squared == 0,
        "acceleration is not a whole number of subunits per tick squared"
    );
    subunits / ticks_squared
}

/// Subunits ([`physics::SUBUNITS_PER_UNIT`]) per tick of a speed authored in units per second:
/// the velocities the zone hands physics. One that is not a whole number of subunits per tick
/// fails to compile in const contexts.
#[must_use]
pub const fn subunits_per_tick(units_per_second: i32) -> i32 {
    let subunits = units_per_second * physics::SUBUNITS_PER_UNIT;
    assert!(
        subunits % TICK_HZ as i32 == 0,
        "speed is not a whole number of subunits per tick"
    );
    subunits / TICK_HZ as i32
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ZoneId(u32);

impl ZoneId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneError {
    message: String,
}

impl ZoneError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ZoneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ZoneError {}
