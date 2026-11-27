//! Greyhaven's optional discoveries (#913, owner choice B 2026-11-17: risk/reward exploration):
//! three danger pockets off the safe roads and settlements, each with a clue a player can read
//! from the path, a fight and an authored reward ([`crate::DiscoveryCache`]).
//!
//! - **Wolfrun hunter's cache**, in the woods' quiet south-western corner beyond the den pack's
//!   southern outing: a snapped bow and spilled arrows on the way in, then a dead hunter's
//!   lean-to and chest, held by Old Scarback, a level 5 elite bear. A new hunter can take the risk
//!   early or come back stronger; the woods back toward the trail are the retreat, and the bear
//!   leashes like any creature.
//! - **Greyfang lookout**, beside the Greyfang Approach where it turns west onto the ridge: a
//!   tattered Redbrand banner by the road and a lit signal brazier mark a plank lookout. A
//!   Bandit and a Crossbowman hold its foot; its cache is on the deck, up the ladder.
//! - **Eastwater Fen supply cache**, in the fen's low ground off its trail: a red-ragged marker
//!   pole points at a swamped punt and oilcloth-wrapped crates, where two Marsh Lurkers wait in
//!   ambush.
//!
//! The art is world dressing (`content/greyhaven/dressing/discoveries.json`, asset-tooling's
//! `discoveries` kit) over generated colliders [`COLLIDERS`]; the lookout's ladder climbs.
//! Spawn IDs 2200–2239 are the discoveries'.

use crate::content::CreatureSpawn;
use crate::{CreatureId, DiscoveryCache, ItemId};

use super::units::templates::{MARSH_LURKER, OLD_SCARBACK, REDBRAND_BANDIT, REDBRAND_CROSSBOWMAN};

/// The discoveries' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 2_200..=2_239;
/// The colliders generated from the sites' art.
pub const COLLIDERS: std::ops::RangeInclusive<u32> = 65_150..=65_160;
/// The Greyfang lookout's ladder, among [`COLLIDERS`].
pub const LOOKOUT_LADDER: u32 = 65_154;
/// Guards idle within 2 m of their posts.
pub const WANDER_RADIUS_UNITS: i32 = 200;

/// The caches' IDs.
pub const HUNTERS_CACHE: u32 = 1;
pub const LOOKOUT_CACHE: u32 = 2;
pub const FEN_CACHE: u32 = 3;

/// Yaw 0 faces +Z (south), 32 768 north, 16 384 east and 49 152 west.
const NORTH_EAST: u16 = 24_576;
const WEST: u16 = 49_152;
const WEST_SOUTH_WEST: u16 = 40_000;

/// `(spawn id, template, feet XZ, facing, levels, wander radius)`.
type Row = super::units::SpawnRow;

#[rustfmt::skip]
const SPAWNS: [Row; 5] = [
    // Old Scarback behind the hunter's lean-to.
    (2_200, OLD_SCARBACK, [-11_478, 4_855], NORTH_EAST, [5, 5], WANDER_RADIUS_UNITS),
    // The lookout's foot.
    (2_210, REDBRAND_BANDIT, [11_080, -16_880], WEST, [4, 5], WANDER_RADIUS_UNITS),
    (2_211, REDBRAND_CROSSBOWMAN, [11_200, -16_340], WEST, [4, 5], WANDER_RADIUS_UNITS),
    // Marsh Lurkers in ambush by the fen's crates.
    (2_220, MARSH_LURKER, [16_975, -1_638], WEST_SOUTH_WEST, [3, 3], 100),
    (2_221, MARSH_LURKER, [16_723, -1_221], WEST_SOUTH_WEST, [3, 3], 100),
];

/// The discoveries' guards.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    super::units::spawns_of(&SPAWNS)
}

/// The three caches: the hunter's chest, the lookout's deck and the fen's crates, each with
/// existing gear and some copper.
#[must_use]
pub fn caches() -> Vec<DiscoveryCache> {
    let cache = |id, name: &str, at, reach, guards: &[u32], item, copper| DiscoveryCache {
        id,
        name: name.to_owned(),
        at,
        reach,
        guards: guards.iter().copied().map(CreatureId::new).collect(),
        item: ItemId::new(item),
        copper,
    };
    vec![
        // A Militia Spear in the chest under the lean-to.
        cache(
            HUNTERS_CACHE,
            "Wolfrun Hunter's Cache",
            [-11_425, 1_100, 4_618],
            150,
            &[2_200],
            23,
            30,
        ),
        // A Greyfang Scout Jerkin on the lookout's deck, 2.25 m up.
        cache(
            LOOKOUT_CACHE,
            "Greyfang Lookout",
            [11_460, 1_332, -16_570],
            120,
            &[2_210, 2_211],
            13,
            40,
        ),
        // A Pine Buckler among the smugglers' crates.
        cache(
            FEN_CACHE,
            "Eastwater Fen Supply Cache",
            [16_962, 708, -1_263],
            150,
            &[2_220, 2_221],
            5,
            45,
        ),
    ]
}
