//! The basin's ore veins (#917): three iron veins in the rock at the foot of the basin's north
//! buttress, a short walk north of the Outpost and east of the western boars' woods. Each is a
//! shared, contested node (Atlas 5.3.16): one gather takes its two ore and spends it for
//! everyone for two minutes. The world dressing draws `gatherables.kit.ore-vein` (and its
//! spent look) at each centre over the same generated collider.

use crate::{GatherNode, IRON_ORE, long_ticks_from_ms};

/// Each vein's centre XZ in units, in wire order.
pub const VEINS: [[i32; 2]; 3] = [[-3_150, -4_300], [-3_900, -4_650], [-4_450, -3_500]];

/// Ore per gather.
pub const ORE_PER_VEIN: u16 = 2;

/// The basin's veins as gather nodes.
#[must_use]
pub fn veins() -> Vec<GatherNode> {
    VEINS
        .iter()
        .map(|&at| GatherNode {
            at,
            item: IRON_ORE,
            quantity: ORE_PER_VEIN,
            respawn_ticks: long_ticks_from_ms(120_000),
        })
        .collect()
}
