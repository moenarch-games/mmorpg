//! The Outpost market square (#421): where both clients draw asset-tooling's
//! `settlement-props`.
//!
//! [`mmorpg_scenery::settlement_props::placements`] composes the square before house 106; here
//! each prop becomes an entry-to-world matrix on the drawn surface. The props are static:
//! clients place them once. While a client does not draw them (its bundle has not loaded or
//! failed), the static scene draws [`fallback`] boxes in their place
//! ([`crate::Motion::Fallback`] for [`crate::Kit::SettlementProps`]). Presentation only.
use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::{Scenery, settlement_props};

use crate::{
    Cull,
    batching::matrix,
    geometry::{Affine, compose, ry, translate},
    lod::{EntryLods, Placement},
};

/// The bundle entries the clients draw, sorted; [`entries`] derives the same list from the
/// square's content.
pub const ENTRIES: [&str; 10] = [
    "settlement.props.barrel",
    "settlement.props.bench",
    "settlement.props.cart",
    "settlement.props.crate",
    "settlement.props.fence",
    "settlement.props.firewood",
    "settlement.props.lantern-post",
    "settlement.props.planter",
    "settlement.props.signpost",
    "settlement.props.stall",
];

/// One drawn prop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    /// The bundle entry, `settlement.props.<prop>`.
    pub entry: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

/// The prop entries the clients load, sorted and once each.
#[must_use]
pub fn entries() -> Vec<&'static str> {
    settlement_props::entries()
}

/// Every placed prop's entry and entry-to-world frame on `scenery`'s drawn surface.
pub(crate) fn frames(scenery: &Scenery) -> Vec<(&'static str, Affine)> {
    settlement_props::placements(scenery)
        .iter()
        .map(|placement| {
            let [x, y, z] = placement.position;
            let lift = scenery.surface_at(x, z);
            let yaw = f64::from(placement.yaw) / 65_536. * std::f64::consts::TAU;
            (
                placement.entry,
                compose(&[translate(metres(x), metres(y + lift), metres(z)), ry(yaw)]),
            )
        })
        .collect()
}

/// Every placed prop on `scenery`.
#[must_use]
pub fn parts(scenery: &Scenery) -> Vec<Part> {
    frames(scenery)
        .into_iter()
        .map(|(entry, frame)| Part {
            entry,
            model: matrix(frame),
        })
        .collect()
}

/// Every placed prop on `scenery` as the native client's placed-kit library takes it: the
/// props' packaged textures, drawn at their own single level.
#[must_use]
pub fn placements(scenery: &Scenery) -> Vec<Placement> {
    parts(scenery)
        .into_iter()
        .map(|part| Placement {
            entry: part.entry,
            model: part.model,
            scale: 1.,
            cull: Cull::Far,
        })
        .collect()
}

/// Each prop's one level (the bundle ships only `default`) and its bounds, from the
/// `settlement-descriptor`, which has no `lodSelection`.
///
/// # Errors
///
/// When the descriptor is not JSON or an asset lacks a key or bounds.
pub fn descriptor_lods(descriptor: &str) -> Result<Vec<EntryLods>, String> {
    let descriptor: serde_json::Value =
        serde_json::from_str(descriptor).map_err(|error| error.to_string())?;
    let point = |value: &serde_json::Value| -> Option<[f64; 3]> {
        let value = value.as_array().filter(|c| c.len() == 3)?;
        let mut out = [0.; 3];
        for (axis, coordinate) in value.iter().enumerate() {
            out[axis] = coordinate.as_f64()?;
        }
        Some(out)
    };
    descriptor["assets"]
        .as_array()
        .ok_or("the descriptor lists no assets")?
        .iter()
        .map(|asset| {
            let entry = asset["key"].as_str().ok_or("an asset without key")?;
            let (min, max) = point(&asset["bounds"]["min"])
                .zip(point(&asset["bounds"]["max"]))
                .ok_or_else(|| format!("{entry} has no bounds"))?;
            let centre = std::array::from_fn(|axis| (min[axis] + max[axis]) / 2.);
            let radius = (0..3)
                .map(|axis| ((max[axis] - min[axis]) / 2.).powi(2))
                .sum::<f64>()
                .sqrt();
            Ok(EntryLods {
                entry: entry.to_owned(),
                centre,
                radius,
                levels: vec![("default".to_owned(), 0.)],
                bounds: Some([min, max]),
            })
        })
        .collect()
}

const WOOD: u32 = 0x8d6641;
const DARK: u32 = 0x5f4430;
const CANVAS: u32 = 0xd9cbad;
const IRON: u32 = 0x55585e;
const SOIL: u32 = 0x6b5a3e;

/// The fallback boxes of `entry` in its own frame: the descriptor's solid collision proxies
/// the zone collides with (`mmorpg_core::greyhaven_vale::market`), as
/// `(centre, half extents, colour)` in metres.
pub(crate) fn fallback(entry: &str) -> Vec<([f64; 3], [f64; 3], u32)> {
    use mmorpg_core::greyhaven_vale::market::Item;
    let Some(item) = settlement_props::item(entry) else {
        return Vec::new();
    };
    let colour = match item {
        Item::Fence | Item::Signpost => DARK,
        Item::LanternPost => IRON,
        Item::Planter => SOIL,
        _ => WOOD,
    };
    item.proxies()
        .iter()
        .enumerate()
        .map(|(index, (centre, half))| {
            // A stall's second proxy is its canvas awning.
            let colour = if item == Item::Stall && index == 1 {
                CANVAS
            } else {
                colour
            };
            (centre.map(metres), half.map(metres), colour)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_prop_has_a_fallback() {
        let keys = entries();
        assert_eq!(keys.len(), 10, "{keys:?}");
        assert!(keys.iter().all(|key| !fallback(key).is_empty()), "{keys:?}");
    }

    #[test]
    fn entries_match_the_square() {
        assert_eq!(entries(), ENTRIES);
    }
}
