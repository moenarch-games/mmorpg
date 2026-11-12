//! The Sunken Shrine (#431): where both clients draw asset-tooling's `shrine-kit` modules.
//!
//! The modules are world dressing (#683): `content/greyhaven/dressing/sunken-shrine.json`
//! places each over the zone's shrine colliders, and here each becomes an entry-to-world
//! matrix. The modules are static: clients place them once. Presentation only: the colliders
//! decide.
use mmorpg_scenery::Scenery;

use crate::{batching::matrix, world_dressing};

/// The asset-key prefix of the shrine kit's modules.
const PREFIX: &str = "shrine.kit.";

/// One drawn kit module.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub key: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
}

/// The kit entries the clients load, sorted and once each.
#[must_use]
pub fn entries() -> Vec<&'static str> {
    let mut keys: Vec<_> = world_dressing::with_prefix(PREFIX)
        .map(|placed| placed.asset.as_str())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Every placed kit module on `scenery`; none outside Greyhaven Vale.
#[must_use]
pub fn parts(scenery: &Scenery) -> Vec<Part> {
    world_dressing::placed(scenery)
        .iter()
        .filter(|placed| placed.asset.starts_with(PREFIX))
        .map(|placed| Part {
            key: placed.asset.as_str(),
            model: matrix(placed.frame()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kit_module_is_placed_once_or_more() {
        let keys = entries();
        assert_eq!(keys.len(), 14, "{keys:?}");
        assert!(keys.iter().all(|key| key.starts_with(PREFIX)));
        assert_eq!(parts(&mmorpg_scenery::greyhaven_vale_scenery()).len(), 41);
    }
}
