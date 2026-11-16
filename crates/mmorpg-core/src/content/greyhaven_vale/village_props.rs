//! The villages' everyday clutter (#851): asset-tooling's `settlement.props` haystacks, hay bales,
//! grain sacks, washing lines, vegetable beds, kennels, a wheelbarrow, chopping blocks, a barrel
//! group and a crate stack, by the barns, houses and camps of the settlements (#818), the
//! farmland's hamlets (#424) and Millbrook Farm. Each stands on clear ground, off every road,
//! field, shore and resident's way, and collides as its descriptor's solid proxies.
//!
//! Positions are each prop's footprint centre on the ground in units; a prop turns by whole
//! quarters, so its proxies stay axis-aligned boxes in the zone.

use super::{TERRAIN, ids};
use crate::StaticCollider;

/// One placed prop: its `settlement.props.<name>`, footprint centre and quarter turns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prop {
    pub name: &'static str,
    pub position: [i32; 2],
    pub quarters: u8,
}

const fn prop(name: &'static str, position: [i32; 2], quarters: u8) -> Prop {
    Prop {
        name,
        position,
        quarters,
    }
}

/// Every placed prop.
#[rustfmt::skip]
pub const PROPS: [Prop; 24] = [
    // Barrowdown: hay west of the barn, a wheelbarrow before it, washing between the two west
    // cottages, a kennel by the dog's round and a vegetable bed beside the east cottage.
    prop("haystack", [50_700, 69_000], 0),
    prop("hay-bale", [50_800, 67_900], 0),
    prop("wheelbarrow", [52_750, 68_050], 0),
    prop("washing-line", [51_600, 65_100], 0),
    prop("kennel", [54_400, 69_400], 0),
    prop("vegetable-bed", [57_800, 63_600], 1),
    // Fenford: sacks by the windmill, washing between the west cottages, a vegetable bed and
    // barrels behind the Ford Inn.
    prop("grain-sacks", [102_500, 24_400], 0),
    prop("washing-line", [100_200, 20_800], 0),
    prop("vegetable-bed", [101_400, 19_200], 1),
    prop("barrel-group", [106_900, 21_000], 0),
    // Westmere Lodge and the hunters' camp: firewood blocks, the lodge's stores, a kennel.
    prop("chopping-block", [-66_200, 6_725], 0),
    prop("crate-stack", [-63_400, 4_500], 1),
    prop("chopping-block", [-47_350, 79_825], 0),
    prop("kennel", [-47_400, 78_400], 0),
    // Millbrook Farm: hay east of the barn.
    prop("haystack", [9_300, 4_400], 0),
    prop("hay-bale", [9_200, 3_300], 0),
    // The hamlets: a haystack by each barn, sacks by the windmills, washing and a garden.
    prop("haystack", [-12_000, 35_600], 0),
    prop("haystack", [14_600, 42_300], 0),
    prop("haystack", [-9_400, 55_800], 0),
    prop("grain-sacks", [-4_350, 57_300], 0),
    prop("grain-sacks", [-7_350, 36_700], 0),
    prop("washing-line", [-11_400, 33_200], 0),
    prop("washing-line", [12_000, 43_700], 0),
    prop("vegetable-bed", [-4_200, 53_775], 1),
];

/// The descriptor's solid collision proxies of `settlement.props.<name>`, `(centre, half
/// extents)` in units in its own frame (feet at y = 0, front toward +Z).
#[must_use]
pub const fn proxies(name: &str) -> &'static [([i32; 3], [i32; 3])] {
    match name.as_bytes() {
        b"haystack" => &[([0, 115, 0], [112, 115, 112])],
        b"hay-bale" => &[([0, 22, 0], [50, 22, 23])],
        b"wheelbarrow" => &[([0, 38, -12], [36, 38, 86])],
        b"washing-line" => &[
            ([-180, 102, 0], [5, 102, 25]),
            ([180, 102, 0], [5, 102, 25]),
        ],
        b"kennel" => &[([0, 49, 0], [48, 49, 58])],
        b"vegetable-bed" => &[([0, 15, 0], [90, 15, 50])],
        b"grain-sacks" => &[([12, 30, 5], [72, 30, 50])],
        b"barrel-group" => &[([0, 45, -35], [64, 45, 32]), ([0, 32, 32], [45, 32, 30])],
        b"chopping-block" => &[([0, 25, 0], [26, 25, 26]), ([-84, 20, 0], [22, 20, 35])],
        b"crate-stack" => &[([26, 35, 0], [99, 35, 42]), ([2, 101, -6], [40, 31, 40])],
        _ => &[],
    }
}

impl Prop {
    /// Its ground height: the terrain's lowest under its proxies' corners, so on a slope it
    /// sinks a little rather than floats.
    #[must_use]
    pub fn ground(&self) -> i32 {
        proxies(self.name)
            .iter()
            .flat_map(|&(centre, half)| {
                [[-1, -1], [-1, 1], [1, -1], [1, 1]].map(|[sx, sz]| {
                    let [dx, dz] = self.turn([centre[0] + sx * half[0], centre[2] + sz * half[2]]);
                    TERRAIN.height_at(self.position[0] + dx, self.position[1] + dz)
                })
            })
            .min()
            .unwrap_or_else(|| TERRAIN.height_at(self.position[0], self.position[1]))
    }

    /// A frame offset turned into the zone.
    #[must_use]
    pub const fn turn(&self, [x, z]: [i32; 2]) -> [i32; 2] {
        match self.quarters % 4 {
            0 => [x, z],
            1 => [z, -x],
            2 => [-x, -z],
            _ => [-z, x],
        }
    }

    fn colliders(&self, first: u32) -> impl Iterator<Item = StaticCollider> + '_ {
        let ground = self.ground();
        (first..)
            .zip(proxies(self.name))
            .map(move |(id, &(centre, half))| {
                let [dx, dz] = self.turn([centre[0], centre[2]]);
                let [hx, hz] = if self.quarters % 2 == 1 {
                    [half[2], half[0]]
                } else {
                    [half[0], half[2]]
                };
                StaticCollider {
                    id,
                    position: [
                        self.position[0] + dx,
                        ground + centre[1],
                        self.position[1] + dz,
                    ],
                    half_extents: [hx, half[1], hz],
                }
            })
    }
}

/// Collider IDs each prop reserves in [`ids::VILLAGE_PROPS`].
const PARTS: u32 = 4;

/// Every prop's proxies, [`PARTS`] IDs apart in [`ids::VILLAGE_PROPS`].
pub(super) fn colliders() -> Vec<StaticCollider> {
    PROPS
        .iter()
        .zip((*ids::VILLAGE_PROPS.start()..).step_by(PARTS as usize))
        .flat_map(|(prop, first)| prop.colliders(first))
        .collect()
}

/// Whether collider `id` is one of the village props'.
#[must_use]
pub fn is_village_prop(id: u32) -> bool {
    ids::VILLAGE_PROPS.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_prop_has_its_proxies() {
        for prop in PROPS {
            assert!(
                !proxies(prop.name).is_empty(),
                "{} has no proxies",
                prop.name
            );
        }
        assert!(colliders().len() <= ids::VILLAGE_PROPS.clone().count());
    }
}
