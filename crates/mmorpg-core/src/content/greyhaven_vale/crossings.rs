//! Footbridges over the region's streams (#828): asset-tooling's `settlement.props.footbridge`
//! (a 9 m plank bridge, its level 6.6 m deck 0.55 m above its ends, 1.2 m sloped ends and
//! post-and-rail sides) where the Delving Track crosses the Delving Run, east of the lookout. The
//! world dressing (`content/greyhaven/dressing/crossings.json`) draws
//! the art; here are the bodies a player walks: the deck, a ramp at each end and the rails.
//!
//! Bodies are axis-aligned, so each bridge runs along X or Z. Its foot height is chosen between
//! its two banks' ground: the higher end's ramp sinks a little into the bank and the lower end's
//! starts at most a small step above it.

use super::{Road, ids};
use crate::{AreaId, Ramp, RampRise, StaticCollider, XzBounds};

/// One footbridge: its centre (XZ), the height its ends stand at and whether it runs along X.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bridge {
    pub centre: [i32; 2],
    pub foot: i32,
    pub along_x: bool,
}

/// The crossings, in body order.
pub const BRIDGES: [Bridge; 1] = [
    // The Delving Track over the Delving Run, east of the lookout.
    Bridge {
        centre: [70_156, -61_400],
        foot: 4_780,
        along_x: true,
    },
];

/// Half the bridge's length, the level deck's half length and the deck's half width (units).
const HALF_LENGTH: i32 = 450;
const HALF_LEVEL: i32 = 330;
const HALF_WIDTH: i32 = 110;
/// The deck's top and the ends' foot above the bridge's foot, and the deck's thickness.
const DECK_TOP: i32 = 55;
const END_FOOT: i32 = 2;
const DECK_THICKNESS: i32 = 8;
/// The rails: their offset from the axis (outside the deck, which they touch), half thickness and
/// top.
const RAIL_OFFSET: i32 = 116;
const RAIL_HALF: i32 = 6;
const RAIL_TOP: i32 = 157;

/// `[along, across]` in the bridge's frame to XZ.
const fn xz(bridge: Bridge, along: i32, across: i32) -> [i32; 2] {
    if bridge.along_x {
        [bridge.centre[0] + along, bridge.centre[1] + across]
    } else {
        [bridge.centre[0] + across, bridge.centre[1] + along]
    }
}

/// Half extents `[along, height, across]` in the bridge's frame to XYZ.
const fn extents(bridge: Bridge, along: i32, height: i32, across: i32) -> [i32; 3] {
    if bridge.along_x {
        [along, height, across]
    } else {
        [across, height, along]
    }
}

fn body(id: u32, at: [i32; 2], y: i32, half_extents: [i32; 3]) -> StaticCollider {
    StaticCollider {
        id,
        position: [at[0], y, at[1]],
        half_extents,
    }
}

/// Each bridge's deck and rails, three bodies per bridge from [`ids::CROSSINGS`]' start.
fn colliders() -> Vec<StaticCollider> {
    let first = *ids::CROSSINGS.start();
    BRIDGES
        .iter()
        .zip((first..).step_by(3))
        .flat_map(|(&bridge, id)| {
            let deck = body(
                id,
                xz(bridge, 0, 0),
                bridge.foot + DECK_TOP - DECK_THICKNESS / 2,
                extents(bridge, HALF_LEVEL, DECK_THICKNESS / 2, HALF_WIDTH),
            );
            let rail = |id, side: i32| {
                body(
                    id,
                    xz(bridge, 0, side * RAIL_OFFSET),
                    bridge.foot + (END_FOOT + RAIL_TOP) / 2,
                    extents(bridge, HALF_LENGTH, (RAIL_TOP - END_FOOT) / 2, RAIL_HALF),
                )
            };
            [deck, rail(id + 1, -1), rail(id + 2, 1)]
        })
        .collect()
}

/// Each bridge's two end ramps, rising from its ends to the deck, two bodies per bridge from
/// [`ids::CROSSINGS`]' start plus 50.
#[must_use]
pub fn ramps() -> Vec<Ramp> {
    let first = ids::CROSSINGS.start() + 50;
    let half_run = (HALF_LENGTH - HALF_LEVEL) / 2;
    BRIDGES
        .iter()
        .zip((first..).step_by(2))
        .flat_map(|(&bridge, id)| {
            [(-1, id), (1, id + 1)].map(|(side, body)| {
                let [x, z] = xz(bridge, side * (HALF_LEVEL + half_run), 0);
                // Each ramp rises toward the deck, away from its own end.
                let rises = match (bridge.along_x, side < 0) {
                    (true, true) => RampRise::PosX,
                    (true, false) => RampRise::NegX,
                    (false, true) => RampRise::PosZ,
                    (false, false) => RampRise::NegZ,
                };
                Ramp {
                    body,
                    position: [x, bridge.foot + (END_FOOT + DECK_TOP) / 2, z],
                    half_extents: extents(bridge, half_run, (DECK_TOP - END_FOOT) / 2, HALF_WIDTH),
                    rises,
                }
            })
        })
        .collect()
}

/// Whether collider `id` is a footbridge's body, which the bridge's art draws.
#[must_use]
pub fn is_crossing(id: u32) -> bool {
    ids::CROSSINGS.contains(&id)
}

pub const AREAS: &[(AreaId, &str, XzBounds)] = &[];
pub const ROADS: &[Road] = &[];

pub(super) const REGION: super::Region = super::Region {
    areas: AREAS,
    roads: ROADS,
    colliders,
};
