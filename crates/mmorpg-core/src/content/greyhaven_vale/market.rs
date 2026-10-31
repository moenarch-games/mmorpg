//! The Greyhaven Outpost's market square (#421): where the clients draw asset-tooling's
//! reusable `settlement-props` (`settlement.props.<item>`, variant `default`), and what the
//! zone collides with there (#577).
//!
//! The release's descriptor composes a 12 × 9 m market square of 21 props from three families
//! (market: stalls, handcart, crates, barrels; street: lantern post, bench, signpost;
//! dressing: fence, planter, firewood) around a clear 3 m walkway along its X axis, in a
//! footprint-base-centre frame. [`SQUARE`] transcribes that composition and [`Item::proxies`]
//! the descriptor's solid collision proxies from the release. The square stands on the grass
//! before house 106, between it and the Millbrook Road, turned a half turn so its fence and
//! stalls back onto the house and its walkway opens toward the spawn plaza. Every proxy
//! collides as the axis-aligned box round it in the zone, so the walkway stays open and the
//! props are solid.
use super::TERRAIN;
use crate::StaticCollider;
use crate::trig::{self, TRIG_ONE, YAW_QUARTER_TURN};

/// One settlement prop.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Item {
    Stall,
    Crate,
    Barrel,
    Cart,
    Fence,
    Firewood,
    LanternPost,
    Bench,
    Planter,
    Signpost,
}

impl Item {
    /// Its name in the bundle, `settlement.props.<name>`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stall => "stall",
            Self::Crate => "crate",
            Self::Barrel => "barrel",
            Self::Cart => "cart",
            Self::Fence => "fence",
            Self::Firewood => "firewood",
            Self::LanternPost => "lantern-post",
            Self::Bench => "bench",
            Self::Planter => "planter",
            Self::Signpost => "signpost",
        }
    }

    /// The descriptor's solid collision proxies, `(centre, half extents)` in units in the
    /// item's own frame (feet at y = 0, front toward +Z).
    #[must_use]
    pub const fn proxies(self) -> &'static [([i32; 3], [i32; 3])] {
        match self {
            Self::Barrel => &[([0, 45, 0], [31, 45, 31])],
            Self::Bench => &[([0, 23, 0], [80, 23, 21])],
            Self::Cart => &[([0, 44, 38], [68, 44, 122])],
            Self::Crate => &[([0, 35, 0], [36, 35, 36])],
            Self::Fence => &[([0, 57, 0], [100, 57, 8])],
            Self::Firewood => &[([0, 57, 0], [87, 57, 42])],
            Self::LanternPost => &[([0, 153, 0], [20, 153, 20])],
            Self::Planter => &[([0, 25, 0], [60, 25, 25])],
            Self::Signpost => &[([0, 148, 0], [22, 148, 22])],
            Self::Stall => &[([0, 50, 4], [124, 50, 74]), ([0, 238, 14], [130, 23, 92])],
        }
    }
}

/// The house whose frontage the square dresses.
pub const HOUSE: u32 = 106;

/// The square's centre relative to [`HOUSE`]'s centre (x, z units): 6 m north of the house's
/// centre puts the square's back fence 1.7 m in front of its north wall.
const OFFSET: [i32; 2] = [50, -900];

/// The square's turn about +Y, in quarter turns: its +Z front faces north (−Z).
const QUARTERS: u8 = 2;

/// One composed prop: item, translation in units (y stacks it) and yaw in degrees, in the
/// square's footprint-base-centre frame.
type Entry = (Item, [i32; 3], i16);

/// The descriptor's `compositions.square`: 12 × 9 m, walkway `|z| ≤ 1.5 m` along X.
#[rustfmt::skip]
pub const SQUARE: [Entry; 21] = [
    (Item::Stall, [-320, 0, -300], 0),
    (Item::Stall, [0, 0, -300], 0),
    (Item::Crate, [-490, 0, -260], 12),
    (Item::Crate, [-490, 70, -260], 30),
    (Item::Crate, [-495, 0, -350], -8),
    (Item::Barrel, [160, 0, -230], 0),
    (Item::Barrel, [165, 0, -300], 0),
    (Item::Cart, [275, 0, -330], 0),
    (Item::Fence, [-500, 0, -430], 0),
    (Item::Fence, [-300, 0, -430], 0),
    (Item::Fence, [-100, 0, -430], 0),
    (Item::Fence, [100, 0, -430], 0),
    (Item::Fence, [300, 0, -430], 0),
    (Item::Fence, [500, 0, -430], 0),
    (Item::Firewood, [490, 0, -375], 0),
    (Item::LanternPost, [-160, 0, 230], 180),
    (Item::Bench, [0, 0, 240], 180),
    (Item::Planter, [170, 0, 250], 180),
    (Item::Barrel, [-360, 0, 260], 0),
    (Item::Signpost, [440, 0, 220], 180),
    (Item::Crate, [-440, 0, 250], 25),
];

/// The square's half extents (x, z units) in its own frame.
pub const HALF_EXTENTS: [i32; 2] = [600, 450];

/// One prop placed in the zone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    pub item: Item,
    /// The prop origin in units: its footprint base centre, `y` above the ground (a stacked
    /// crate's lift).
    pub position: [i32; 3],
    /// Rotation about +Y, 65 536 steps per turn, 0 keeping the prop's +Z front on +Z.
    pub yaw: u16,
}

/// Turns `[x, z]` by `quarters` quarter turns about +Y (+Z toward +X).
const fn turn([x, z]: [i32; 2], quarters: u8) -> [i32; 2] {
    match quarters % 4 {
        0 => [x, z],
        1 => [z, -x],
        2 => [-x, -z],
        _ => [-z, x],
    }
}

/// The square's centre (x, z units) before a house centred at `house`.
#[must_use]
pub const fn centre(house: [i32; 2]) -> [i32; 2] {
    [house[0] + OFFSET[0], house[1] + OFFSET[1]]
}

/// Every prop of the square before a house centred at `house`, in [`SQUARE`] order.
#[must_use]
pub fn placements(house: [i32; 2]) -> Vec<Placement> {
    let [cx, cz] = centre(house);
    let turned = i32::from(QUARTERS) * 90;
    SQUARE
        .iter()
        .map(|&(item, [x, y, z], degrees)| {
            let [dx, dz] = turn([x, z], QUARTERS);
            let degrees = (i32::from(degrees) + turned).rem_euclid(360);
            Placement {
                item,
                position: [cx + dx, y, cz + dz],
                yaw: u16::try_from(degrees * i32::from(YAW_QUARTER_TURN) / 90 % 65_536)
                    .expect("a yaw below one turn"),
            }
        })
        .collect()
}

/// The square's colliders before `house`, one per proxy with IDs from `first_id`: each the
/// axis-aligned box round its turned proxy, on the ground there.
#[must_use]
pub fn colliders(house: &StaticCollider, first_id: u32) -> Vec<StaticCollider> {
    let scale = |value: i32, factor: i32| {
        i32::try_from((i64::from(value) * i64::from(factor)).div_euclid(i64::from(TRIG_ONE)))
            .unwrap_or(0)
    };
    let mut colliders = Vec::new();
    for placement in placements([house.position[0], house.position[2]]) {
        let (sin, cos) = (trig::sin(placement.yaw), trig::cos(placement.yaw));
        let [x, lift, z] = placement.position;
        let ground = TERRAIN.height_at(x, z);
        for &([px, py, pz], [hx, hy, hz]) in placement.item.proxies() {
            // `ry`: x' = x cos + z sin, z' = −x sin + z cos.
            let offset = [
                scale(px, cos) + scale(pz, sin),
                scale(pz, cos) - scale(px, sin),
            ];
            let half = [
                scale(hx, cos.abs()) + scale(hz, sin.abs()) + 1,
                hy,
                scale(hx, sin.abs()) + scale(hz, cos.abs()) + 1,
            ];
            let id = first_id + u32::try_from(colliders.len()).unwrap_or(u32::MAX);
            colliders.push(StaticCollider {
                id,
                position: [x + offset[0], ground + lift + py, z + offset[1]],
                half_extents: half,
            });
        }
    }
    colliders
}
