//! The Greyhaven Outpost's market square (#421): where the clients draw asset-tooling's
//! reusable `settlement-props` (`settlement.props.<prop>`, variant `default`).
//!
//! The release's descriptor composes a 12 × 9 m market square of 21 props from three families
//! (market: stalls, handcart, crates, barrels; street: lantern post, bench, signpost;
//! dressing: fence, planter, firewood) around a clear 3 m walkway along its X axis, in a
//! footprint-base-centre frame. [`SQUARE`] transcribes that composition from the pinned
//! release. The square stands on the grass before house 106, between it and the Millbrook
//! Road, turned a half turn so its fence and stalls back onto the house and its walkway opens
//! toward the spawn plaza. The props are presentation only: they have no colliders, picking
//! or interactions, so the walkway, the road and the plaza stay exactly as traversable as
//! before and every authoritative collider is unchanged.
use mmorpg_core::trig::YAW_QUARTER_TURN;

use crate::Scenery;

/// The props' rolling release the clients draw.
pub const RELEASE: &str = "settlement-props";

/// The house whose frontage the square dresses.
pub const HOUSE: u32 = 106;

/// The square's centre relative to [`HOUSE`]'s centre (x, z units): 6 m north of the house's
/// centre puts the square's back fence 1.7 m in front of its north wall.
const OFFSET: [i32; 2] = [50, -900];

/// The square's turn about +Y, in quarter turns: its +Z front faces north (−Z).
const QUARTERS: u8 = 2;

/// One composed prop: entry, translation in units (y stacks it) and yaw in degrees, in the
/// square's footprint-base-centre frame.
type Item = (&'static str, [i32; 3], i16);

/// The descriptor's `compositions.square`: 12 × 9 m, walkway `|z| ≤ 1.5 m` along X.
#[rustfmt::skip]
pub const SQUARE: [Item; 21] = [
    ("settlement.props.stall", [-320, 0, -300], 0),
    ("settlement.props.stall", [0, 0, -300], 0),
    ("settlement.props.crate", [-490, 0, -260], 12),
    ("settlement.props.crate", [-490, 70, -260], 30),
    ("settlement.props.crate", [-495, 0, -350], -8),
    ("settlement.props.barrel", [160, 0, -230], 0),
    ("settlement.props.barrel", [165, 0, -300], 0),
    ("settlement.props.cart", [275, 0, -330], 0),
    ("settlement.props.fence", [-500, 0, -430], 0),
    ("settlement.props.fence", [-300, 0, -430], 0),
    ("settlement.props.fence", [-100, 0, -430], 0),
    ("settlement.props.fence", [100, 0, -430], 0),
    ("settlement.props.fence", [300, 0, -430], 0),
    ("settlement.props.fence", [500, 0, -430], 0),
    ("settlement.props.firewood", [490, 0, -375], 0),
    ("settlement.props.lantern-post", [-160, 0, 230], 180),
    ("settlement.props.bench", [0, 0, 240], 180),
    ("settlement.props.planter", [170, 0, 250], 180),
    ("settlement.props.barrel", [-360, 0, 260], 0),
    ("settlement.props.signpost", [440, 0, 220], 180),
    ("settlement.props.crate", [-440, 0, 250], 25),
];

/// The square's half extents (x, z units) in its own frame.
pub const HALF_EXTENTS: [i32; 2] = [600, 450];

/// One prop placed in the zone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    /// The bundle entry, `settlement.props.<prop>`.
    pub entry: &'static str,
    /// The prop origin in units: its footprint base centre, `y` above the drawn surface (a
    /// stacked crate's lift).
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

/// The square's centre (x, z units), or `None` when `scenery` has no [`HOUSE`] to front.
#[must_use]
pub fn centre(scenery: &Scenery) -> Option<[i32; 2]> {
    let house = scenery
        .props
        .iter()
        .find(|prop| prop.collider == Some(HOUSE))?;
    Some([house.position[0] + OFFSET[0], house.position[2] + OFFSET[1]])
}

/// Every prop of the square on `scenery`, in [`SQUARE`] order; none without [`HOUSE`].
#[must_use]
pub fn placements(scenery: &Scenery) -> Vec<Placement> {
    let Some([cx, cz]) = centre(scenery) else {
        return Vec::new();
    };
    let turned = i32::from(QUARTERS) * 90;
    SQUARE
        .iter()
        .map(|&(entry, [x, y, z], degrees)| {
            let [dx, dz] = turn([x, z], QUARTERS);
            let degrees = (i32::from(degrees) + turned).rem_euclid(360);
            Placement {
                entry,
                position: [cx + dx, y, cz + dz],
                yaw: u16::try_from(degrees * i32::from(YAW_QUARTER_TURN) / 90 % 65_536)
                    .expect("a yaw below one turn"),
            }
        })
        .collect()
}

/// The prop entries the square uses, sorted and once each.
#[must_use]
pub fn entries() -> Vec<&'static str> {
    let mut entries: Vec<_> = SQUARE.iter().map(|(entry, ..)| *entry).collect();
    entries.sort_unstable();
    entries.dedup();
    entries
}
