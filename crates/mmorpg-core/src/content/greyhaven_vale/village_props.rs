//! The villages' everyday clutter (#851): asset-tooling's `settlement.props` haystacks, hay bales,
//! grain sacks, washing lines, vegetable beds, kennels, a wheelbarrow, chopping blocks, a barrel
//! group and a crate stack, by the barns, houses and camps of the settlements (#818), the
//! farmland's hamlets (#424) and Millbrook Farm. Each stands on clear ground, off every road,
//! field, shore and resident's way, and collides as its descriptor's solid proxies.
//!
//! The second batch (#891, [`SETTLEMENT_PROPS`]) dresses the places with their trades: market
//! goods on the Outpost's stalls and a lower market by the brook, the smithy's forge, the
//! tannery's hide frames, the inns' clutter and signs, Millbrook's mill on the brook, a post
//! mill on Barrowdown's knoll, the farmyards' troughs, coops and skeps, the fields' scarecrows
//! and the fisher's jetty on Westmere Tarn. Several play a looping node clip (the mill wheels,
//! the windmill's sails, the forge's bellows, the signs, the vanes and the scarecrows), which
//! is presentation only.
//!
//! Positions are each prop's origin in units; a prop turns by whole quarters, so its proxies stay
//! axis-aligned boxes in the zone. A prop stands on the terrain's lowest point under it, or on
//! an explicit floor: a table top, a stall's counter, a wall's mounting height, an inn's floor
//! or the water.

use super::{TERRAIN, ids};
use crate::StaticCollider;

/// One placed prop: its `settlement.props.<name>`, origin, quarter turns and, when it does not
/// stand on the terrain, the height of its feet in units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prop {
    pub name: &'static str,
    pub position: [i32; 2],
    pub quarters: u8,
    pub floor: Option<i32>,
}

const fn prop(name: &'static str, position: [i32; 2], quarters: u8) -> Prop {
    Prop {
        name,
        position,
        quarters,
        floor: None,
    }
}

/// A prop whose feet stand at `floor`: on furniture, a counter, a wall bracket or the water.
const fn on(name: &'static str, position: [i32; 2], quarters: u8, floor: i32) -> Prop {
    Prop {
        name,
        position,
        quarters,
        floor: Some(floor),
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

/// The Greyhaven inn's floor, its tables' and its bar counter's tops (`super::inn`).
const INN_FLOOR: i32 = super::inn::ORIGIN[1];
const INN_TABLE: i32 = INN_FLOOR + 76;
const INN_BAR: i32 = INN_FLOOR + 112;
/// The Ford Inn's and the Barrow Rest's floors (`super::cottages`) and their tables' tops.
const FORD_FLOOR: i32 = 977;
const BARROW_FLOOR: i32 = 815;
const TABLE: i32 = 76;
/// A hanging sign's bracket, 2.1 m up its wall.
const SIGN: i32 = 210;

/// The second batch (#891), clear of every road, doorway, path and resident's way
/// (`clear-spot`, `--indoors` inside the inns), each colliding as its descriptor's proxies.
#[rustfmt::skip]
pub const SETTLEMENT_PROPS: [Prop; 61] = [
    // The Outpost's market: goods on the stalls' counters (1 m up); below the square, a lower
    // market on the brook's bank by the third stall, with cloth, pots and a butcher's rack.
    on("apple-crate", [2_700, 2_960], 2, 100),
    on("bread-basket", [2_850, 2_960], 2, 100),
    on("cheese-board", [2_420, 2_960], 2, 100),
    on("fish-crate", [-1_200, 2_920], 2, 88),
    prop("cloth-bolts", [-1_890, 2_800], 1),
    prop("pottery", [-1_660, 2_900], 0),
    prop("hanging-meats", [-2_100, 2_550], 1),
    // The smithy's open forge behind it, its sign over the yard; the tannery's frames on the
    // brook west of the tanner's house.
    prop("forge", [2_700, -250], 0),
    on("shop-sign-smith", [2_550, 800], 0, SIGN),
    prop("hide-frame", [-2_900, 2_900], 1),
    prop("hide-frame", [-3_000, 2_700], 1),
    // The tailor's and the baker's signs over the square, and the tailor's loom by her door.
    on("shop-sign-tailor", [-900, 3_500], 2, SIGN - 30),
    on("shop-sign-baker", [1_000, 3_500], 2, SIGN - 10),
    prop("loom", [-1_300, 3_400], 2),
    // Banners by the south gate.
    prop("banner-village", [-450, 5_400], 0),
    prop("pennant-village", [450, 5_400], 0),
    // The Greyhaven inn: its sign over the square; casks, shelves, a dresser, a settle by the
    // hearth, a rug in the doorway, candles on the tables and the bar, a candelabrum in the
    // front corner and a shield on the back wall.
    on("inn-sign", [1_680, 500], 0, INN_FLOOR + SIGN),
    on("cask-rack", [2_110, 250], 3, INN_FLOOR),
    on("jar-shelf", [1_550, -430], 0, INN_FLOOR),
    on("dresser", [1_350, -420], 0, INN_FLOOR),
    on("settle", [1_050, -40], 3, INN_FLOOR),
    on("rug", [1_500, 300], 0, INN_FLOOR),
    on("candles", [1_070, -220], 0, INN_TABLE),
    on("candles", [1_070, 140], 0, INN_TABLE),
    on("candles", [1_340, -120], 0, INN_TABLE),
    on("candles", [1_910, -310], 0, INN_BAR),
    on("candelabra", [2_100, 400], 0, INN_FLOOR),
    on("wall-shield", [1_050, -450], 0, INN_FLOOR + 150),
    // Millbrook's watermill on the village brook, its wheel in the stream, and a water wheel
    // downstream.
    on("watermill", [-1_040, 6_500], 0, -25),
    on("water-wheel", [-880, 6_880], 0, -40),
    // Millbrook Farm: a scarecrow in the field, troughs before the barn, the vane on its ridge,
    // and a coop, skeps and a churn in the farmhouse's yard.
    prop("scarecrow", [7_300, 7_000], 0),
    prop("feed-trough", [7_850, 3_250], 0),
    prop("water-trough", [8_150, 3_250], 0),
    on("weather-vane", [8_000, 4_400], 0, 2_319),
    prop("hen-coop", [6_050, 3_800], 2),
    prop("beehive-skeps", [6_350, 3_800], 2),
    prop("butter-churn", [6_200, 3_950], 0),
    // Barrowdown: a post mill on the knoll, bunting and a market day's awning stall, scale and
    // potter's wheel on the green, a banner at the lane, the barn's vane; the Barrow Rest's sign
    // over its door, and inside casks, a rug and candles.
    prop("windmill", [54_000, 61_500], 0),
    prop("bunting", [54_350, 66_150], 0),
    prop("stall-awning", [55_150, 65_600], 2),
    prop("merchant-scale", [55_330, 65_560], 2),
    prop("potters-wheel", [55_550, 65_600], 2),
    prop("banner-village", [55_000, 63_000], 0),
    on("weather-vane", [52_000, 69_000], 0, 2_034),
    on("inn-sign", [56_200, 67_050], 2, BARROW_FLOOR + SIGN),
    on("cask-rack", [56_530, 67_665], 2, BARROW_FLOOR),
    on("rug", [56_440, 67_460], 2, BARROW_FLOOR),
    on("candles", [56_590, 67_230], 0, BARROW_FLOOR + TABLE),
    // Fenford: a banner by the waystone; the Ford Inn's sign, bar counter and candles.
    prop("banner-village", [104_000, 23_500], 0),
    on("inn-sign", [106_200, 20_350], 0, FORD_FLOOR + SIGN),
    on("bar-counter", [105_870, 19_745], 0, FORD_FLOOR),
    on("candles", [105_810, 20_170], 0, FORD_FLOOR + TABLE),
    // The hamlets: scarecrows in two fields, a plough at a field's edge.
    prop("scarecrow", [-15_700, 32_300], 0),
    prop("scarecrow", [10_300, 49_900], 0),
    prop("plough", [-13_150, 32_300], 0),
    // The hunters' camp: a spit over its own fire and a hide on its frame.
    prop("spit-roast", [-48_200, 79_000], 0),
    prop("hide-frame", [-48_400, 78_900], 1),
    // Westmere Lodge: a windlass over a bare curb in the yard; below it on the tarn's shore,
    // drying nets, a jetty and a rowing boat moored beside it on the water.
    prop("well-windlass", [-65_500, 6_500], 0),
    prop("fishing-nets", [-70_200, 4_100], 1),
    on("jetty", [-70_750, 4_400], 0, -2_000),
    on("rowing-boat", [-70_950, 4_580], 0, -2_025),
];

/// The bare stone curb under the well windlass (#891): `(centre, half extents)` in units in its
/// frame, between the frame's posts. The windlass is only the frame and the drum.
pub const CURB: ([i32; 3], [i32; 3]) = ([0, 45, 0], [80, 45, 80]);

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
        b"apple-crate" => &[([0, 19, 0], [30, 19, 20])],
        b"bread-basket" => &[([0, 12, 0], [28, 12, 28])],
        b"cheese-board" => &[([0, 12, 0], [40, 12, 23])],
        b"fish-crate" => &[([0, 12, 0], [35, 12, 23])],
        b"cloth-bolts" => &[([0, 48, 10], [60, 48, 40])],
        b"pottery" => &[([-35, 33, -12], [22, 33, 22]), ([0, 18, 4], [55, 18, 34])],
        b"merchant-scale" => &[([0, 62, 0], [25, 62, 20])],
        b"stall-awning" => &[([0, 43, 0], [129, 43, 74]), ([0, 217, 0], [150, 48, 92])],
        b"hanging-meats" => &[([0, 99, 0], [85, 99, 25])],
        b"cask-rack" => &[([0, 51, 0], [70, 51, 36])],
        b"candles" => &[([0, 12, 0], [12, 12, 12])],
        b"candelabra" => &[([0, 84, 0], [30, 84, 30])],
        b"jar-shelf" => &[([0, 92, 0], [62, 92, 16])],
        b"rug" => &[([0, 1, 0], [100, 1, 65])],
        b"bar-counter" => &[([0, 53, 3], [100, 53, 33])],
        b"settle" => &[([0, 23, 0], [75, 23, 25]), ([0, 85, -21], [75, 40, 6])],
        b"dresser" => &[([0, 45, 2], [63, 45, 25]), ([0, 143, -9], [64, 53, 14])],
        b"rowing-boat" => &[([0, 34, 0], [180, 34, 60])],
        b"jetty" => &[([0, 33, 0], [150, 18, 80]), ([0, -53, 0], [90, 68, 80])],
        b"fishing-nets" => &[([0, 111, 0], [132, 111, 12]), ([18, 38, 77], [93, 38, 32])],
        b"banner-village" => &[([0, 186, 0], [18, 186, 18])],
        b"pennant-village" => &[([0, 216, 0], [18, 216, 18])],
        b"bunting" => &[
            ([-300, 160, 0], [14, 160, 14]),
            ([300, 160, 0], [14, 160, 14]),
        ],
        b"wall-shield" => &[([0, 74, 8], [62, 74, 8])],
        b"inn-sign" => &[([0, 43, 56], [9, 43, 56])],
        b"scarecrow" => &[([0, 100, 0], [6, 100, 6]), ([0, 141, 0], [25, 56, 25])],
        b"weather-vane" => &[([0, 50, 0], [10, 50, 10])],
        b"beehive-skeps" => &[([0, 46, 0], [70, 46, 22])],
        b"water-trough" => &[([0, 28, 0], [80, 28, 31])],
        b"feed-trough" => &[([0, 23, 0], [78, 23, 28])],
        b"hen-coop" => &[([0, 70, -33], [50, 70, 47]), ([23, 25, 41], [11, 25, 39])],
        b"plough" => &[([0, 48, 3], [33, 48, 142])],
        b"watermill" => &[
            ([-52, 220, 0], [198, 220, 168]),
            ([-112, 23, 195], [55, 23, 27]),
            ([214, 147, 0], [50, 147, 90]),
        ],
        b"water-wheel" => &[
            ([-58, 69, 0], [10, 69, 90]),
            ([58, 69, 0], [10, 69, 90]),
            ([0, 137, 0], [34, 137, 137]),
        ],
        b"windmill" => &[
            ([0, 160, 40], [152, 160, 152]),
            ([0, 500, 43], [125, 180, 158]),
            ([0, 160, -188], [38, 160, 83]),
        ],
        b"forge" => &[
            ([53, 45, -9], [75, 45, 63]),
            ([53, 219, -59], [75, 131, 13]),
            ([-92, 45, 4], [54, 45, 33]),
            ([153, 25, 44], [27, 25, 27]),
        ],
        b"well-windlass" => &[
            ([-95, 83, 0], [10, 83, 62]),
            ([95, 83, 0], [10, 83, 62]),
            CURB,
        ],
        b"spit-roast" => &[
            ([0, 8, 0], [60, 8, 60]),
            ([-78, 52, 0], [6, 52, 10]),
            ([78, 52, 0], [6, 52, 10]),
        ],
        b"butter-churn" => &[([0, 74, 0], [22, 74, 22])],
        b"loom" => &[([0, 103, 0], [92, 103, 30])],
        b"potters-wheel" => &[([0, 54, 19], [43, 54, 39]), ([0, 26, -39], [43, 26, 19])],
        b"hide-frame" => &[([0, 98, 0], [97, 98, 35])],
        b"shop-sign-smith" => &[([0, 43, 52], [9, 43, 52])],
        b"shop-sign-baker" => &[([0, 43, 52], [9, 43, 52])],
        b"shop-sign-tailor" => &[([0, 43, 52], [9, 43, 52])],
        _ => &[],
    }
}

impl Prop {
    /// Its feet's height: its floor, or the terrain's lowest under its proxies' corners, so on a
    /// slope it sinks a little rather than floats.
    #[must_use]
    pub fn ground(&self) -> i32 {
        if let Some(floor) = self.floor {
            return floor;
        }
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
            .filter(|_| !WALKED_ON.contains(&self.name))
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

/// Props flat enough to walk over, which draw but do not collide: a rug.
const WALKED_ON: [&str; 1] = ["rug"];

/// Collider IDs each prop reserves in [`ids::VILLAGE_PROPS`] and [`ids::SETTLEMENT_PROPS`].
const PARTS: u32 = 4;

/// Every prop with its first collider ID: [`PROPS`] in [`ids::VILLAGE_PROPS`], then
/// [`SETTLEMENT_PROPS`] in [`ids::SETTLEMENT_PROPS`], [`PARTS`] IDs apart.
fn numbered() -> impl Iterator<Item = (&'static Prop, u32)> {
    let step = PARTS as usize;
    PROPS
        .iter()
        .zip((*ids::VILLAGE_PROPS.start()..).step_by(step))
        .chain(
            SETTLEMENT_PROPS
                .iter()
                .zip((*ids::SETTLEMENT_PROPS.start()..).step_by(step)),
        )
}

/// Every prop's proxies.
pub(super) fn colliders() -> Vec<StaticCollider> {
    numbered()
        .flat_map(|(prop, first)| prop.colliders(first))
        .collect()
}

/// Whether collider `id` is one of the village props'.
#[must_use]
pub fn is_village_prop(id: u32) -> bool {
    ids::VILLAGE_PROPS.contains(&id) || ids::SETTLEMENT_PROPS.contains(&id)
}

/// Whether collider `id` is a village prop standing on the terrain, whose ground the scenery
/// levels; one on a floor (furniture, a wall, the water) leaves the ground as it is.
#[must_use]
pub fn on_terrain(id: u32) -> bool {
    numbered().any(|(prop, first)| {
        prop.floor.is_none() && (first..first + PARTS).contains(&id) && is_village_prop(id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_prop_has_its_proxies() {
        for prop in PROPS.iter().chain(&SETTLEMENT_PROPS) {
            assert!(
                (1..=PARTS as usize).contains(&proxies(prop.name).len()),
                "{} has no proxies or too many",
                prop.name
            );
        }
        assert!(numbered().all(|(_, first)| is_village_prop(first + PARTS - 1)));
    }
}
