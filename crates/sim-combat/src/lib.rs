//! Combat rules every game shares: the unit baseline, the hit table, swing reach and height
//! band with the coverage test that applies them, the movement-state strikes by weapon family (#885), and the player classes with their resources. Built on `sim-foundation`; it knows
//! nothing of zones, quests or authored content.

pub mod class;
pub mod melee;
pub mod swing;
pub mod unit;

#[cfg(test)]
use sim_foundation::ZoneId;
use sim_foundation::{long_ticks_from_ms, rng, ticks_from_ms};
