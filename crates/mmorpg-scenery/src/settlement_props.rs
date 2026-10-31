//! The Greyhaven Outpost's market square (#421): where the clients draw asset-tooling's
//! reusable `settlement-props` (`settlement.props.<prop>`, variant `default`).
//!
//! `mmorpg_core::greyhaven_vale::market` is the one definition of the square: its composition
//! before house 106 and the collision proxies the zone collides with (#577). This module
//! names each placed prop's bundle entry for the clients.
use mmorpg_core::greyhaven_vale::market::{self, Item};

use crate::Scenery;

/// The props' rolling release the clients draw.
pub const RELEASE: &str = "settlement-props";

/// The house whose frontage the square dresses.
pub const HOUSE: u32 = market::HOUSE;

/// The square's half extents (x, z units) in its own frame.
pub const HALF_EXTENTS: [i32; 2] = market::HALF_EXTENTS;

/// One prop placed in the zone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    /// The bundle entry, `settlement.props.<prop>`.
    pub entry: &'static str,
    pub item: Item,
    /// The prop origin in units: its footprint base centre, `y` above the drawn surface (a
    /// stacked crate's lift).
    pub position: [i32; 3],
    /// Rotation about +Y, 65 536 steps per turn, 0 keeping the prop's +Z front on +Z.
    pub yaw: u16,
}

/// The bundle entry of `item`.
#[must_use]
pub const fn entry(item: Item) -> &'static str {
    match item {
        Item::Stall => "settlement.props.stall",
        Item::Crate => "settlement.props.crate",
        Item::Barrel => "settlement.props.barrel",
        Item::Cart => "settlement.props.cart",
        Item::Fence => "settlement.props.fence",
        Item::Firewood => "settlement.props.firewood",
        Item::LanternPost => "settlement.props.lantern-post",
        Item::Bench => "settlement.props.bench",
        Item::Planter => "settlement.props.planter",
        Item::Signpost => "settlement.props.signpost",
    }
}

/// The item a bundle entry draws.
#[must_use]
pub fn item(entry: &str) -> Option<Item> {
    market::SQUARE
        .iter()
        .map(|(item, ..)| *item)
        .find(|item| self::entry(*item) == entry)
}

/// House [`HOUSE`]'s centre on `scenery` (x, z units), if it has one.
fn house(scenery: &Scenery) -> Option<[i32; 2]> {
    scenery
        .props
        .iter()
        .find(|prop| prop.collider == Some(HOUSE))
        .map(|house| [house.position[0], house.position[2]])
}

/// The square's centre (x, z units), or `None` when `scenery` has no [`HOUSE`] to front.
#[must_use]
pub fn centre(scenery: &Scenery) -> Option<[i32; 2]> {
    house(scenery).map(market::centre)
}

/// Every prop of the square on `scenery`, in [`market::SQUARE`] order; none without [`HOUSE`].
#[must_use]
pub fn placements(scenery: &Scenery) -> Vec<Placement> {
    house(scenery).map_or_else(Vec::new, |house| {
        market::placements(house)
            .into_iter()
            .map(|placement| Placement {
                entry: entry(placement.item),
                item: placement.item,
                position: placement.position,
                yaw: placement.yaw,
            })
            .collect()
    })
}

/// The prop entries the square uses, sorted and once each.
#[must_use]
pub fn entries() -> Vec<&'static str> {
    let mut entries: Vec<_> = market::SQUARE
        .iter()
        .map(|(item, ..)| entry(*item))
        .collect();
    entries.sort_unstable();
    entries.dedup();
    entries
}
