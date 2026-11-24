//! Greyhaven's crags from asset-tooling's `crag-kit` (#668, asset-tooling#480): its
//! freestanding sculpted outcrops ([`OUTCROPS`]), placed by the world dressing
//! ([`crate::world_dressing`], #683).
//!
//! The Bandit Highlands' crags are groups of these outcrops (#1195,
//! `content/greyhaven/dressing/crags.json`): a climbable crag is a ledged buttress over talus
//! steps, a few with climbing ivy, a sheer one a cluster of smooth spires. The zone's collision
//! is generated from the same art, so a climber's hands meet the drawn rock without a stand-off
//! push; the kit's 3 m box modules (faces, corners and caps) are no longer placed. While a client
//! does not draw the kit, the outcrops' collision surfaces are their fallback
//! ([`crate::Kit::Crag`]). Presentation only.
use serde_json::Value;

use mmorpg_scenery::Scenery;

pub use crate::lod::{EntryLods, Lods, Placement, Shown};

/// The freestanding outcrops the world dressing places, sorted.
pub const OUTCROPS: [&str; 5] = [
    "crag.kit.outcrop-buttress-1",
    "crag.kit.outcrop-buttress-2",
    "crag.kit.outcrop-spire",
    "crag.kit.outcrop-talus-1",
    "crag.kit.outcrop-talus-2",
];
/// Every entry the clients load: the outcrops.
pub const KIT_ENTRIES: [&str; 5] = OUTCROPS;

/// Every outcrop the world dressing places on `s` (#683). Only Greyhaven Vale has them.
#[must_use]
pub fn placements(s: &Scenery) -> Vec<Placement> {
    crate::world_dressing::placements(s, crate::Kit::Crag)
}

/// Every placed outcrop's levels from the bundle's `crag-descriptor` attachment JSON
/// ([`KIT_ENTRIES`]); the box modules and the macro texture are not placed.
///
/// # Errors
///
/// For malformed JSON or levels of detail.
pub fn descriptor_lods(json: &str) -> Result<Vec<EntryLods>, String> {
    let mut value: Value =
        serde_json::from_str(json).map_err(|error| format!("crag descriptor: {error}"))?;
    // The outcrops are listed apart from the modules, in the same shape.
    let outcrops = value["outcrops"]["entries"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    value["assets"] = Value::Array(
        outcrops
            .into_iter()
            .filter(|asset| {
                asset["key"]
                    .as_str()
                    .is_some_and(|key| KIT_ENTRIES.contains(&key))
            })
            .collect(),
    );
    crate::lod::descriptor_lods(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_dressing::{Grip, committed};
    use mmorpg_core::greyhaven_vale::{SHEER_CRAGS, highlands, ids};

    #[test]
    fn the_kit_loads_every_outcrop_the_dressing_places() {
        let s = mmorpg_scenery::greyhaven_vale_scenery();
        let assets = crate::world_dressing::assets(&s, crate::Kit::Crag);
        assert!(!assets.is_empty());
        assert!(
            assets.iter().all(|asset| OUTCROPS.contains(asset)),
            "{assets:?}"
        );
        assert_eq!(
            placements(&s).len(),
            crate::world_dressing::placements(&s, crate::Kit::Crag).len()
        );
    }

    /// Each highland crag is a group of outcrops (#1195) whose first stands on the crag's
    /// footprint with the crag's collider; a sheer crag has no climbable rock, a climbable one
    /// has, and the spires never climb.
    #[test]
    fn every_highland_crag_is_an_outcrop_group_sheer_where_the_content_is_sheer() {
        let crags: Vec<_> = committed()
            .iter()
            .filter(|p| {
                p.collider
                    .is_some_and(|id| ids::HIGHLAND_CRAGS.contains(&id))
            })
            .collect();
        for (index, &(min, max, _)) in highlands::CRAGS.iter().enumerate() {
            let collider = ids::HIGHLAND_CRAGS.start() + index as u32;
            let core = crags
                .iter()
                .find(|p| p.collider == Some(collider))
                .unwrap_or_else(|| panic!("crag {index} has no core outcrop"));
            let [x, _, z] = core.position;
            assert!(
                (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z),
                "crag {index}"
            );
            let group: Vec<_> = crags
                .iter()
                .filter(|p| p.id.starts_with(&format!("highland-crag-{index}-")))
                .collect();
            assert!(group.len() >= 3, "crag {index}");
            let climbs = group.iter().any(|p| p.grip == Some(Grip::Rock));
            assert_eq!(climbs, !SHEER_CRAGS.contains(&index), "crag {index}");
        }
        assert!(
            crags
                .iter()
                .filter(|p| p.asset == "crag.kit.outcrop-spire")
                .all(|p| p.grip == Some(Grip::Sheer))
        );
    }

    #[test]
    fn the_fetched_descriptor_maps_every_outcrop_and_its_levels() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/public/scenery/crag.kit"
        );
        let Some(manifest) = crate::fetched_bundles::read("scenery/crag.kit/manifest.json") else {
            return;
        };
        let manifest: Value = serde_json::from_str(&manifest).unwrap();
        let attachment = manifest["assets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|asset| asset["attachments"].as_array().unwrap())
            .find(|a| a["name"] == "crag-descriptor")
            .unwrap();
        let json =
            std::fs::read_to_string(format!("{path}/{}", attachment["path"].as_str().unwrap()))
                .unwrap();
        let lods = descriptor_lods(&json).unwrap();
        assert_eq!(lods.len(), KIT_ENTRIES.len());
        assert!(lods.iter().all(|l| l.levels.len() == 3));
    }
}
