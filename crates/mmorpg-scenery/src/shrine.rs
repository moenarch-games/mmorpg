//! The Sunken Shrine's kit (#431): asset-tooling's `shrine-kit` modules (`shrine.kit.<part>`,
//! variant `default`) over the zone's shrine colliders.
//!
//! Where each module stands is world dressing (#683):
//! `content/greyhaven/dressing/sunken-shrine.json` places the arch and broken walls on the plaza, the pillars, the fallen blocks, the altar and
//! the cave's mouth, tunnels, air-pocket room and closed end over the boxes
//! [`mmorpg_core::greyhaven_vale::shrine`] lays out; weed, silt drifts and shells only render.
//! `mmorpg_presentation::shrine` draws them. Presentation only: the colliders decide.

/// The shrine kit's rolling release the clients draw.
pub const RELEASE: &str = "shrine-kit";
