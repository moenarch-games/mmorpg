//! Optional discovery caches (#913, owner choice B 2026-11-17: risk/reward exploration).
//!
//! A cache is an authored reward spot off the safe roads: a hunter's chest, a lookout's deck, a
//! smuggler's crates. Its guards are ordinary content spawns with the ordinary aggro, leash,
//! respawn and loot rules; while any of them stands alive the cache stays shut. A living player
//! whose feet come within [`DiscoveryCache::reach`] of the spot, and within
//! [`CACHE_HEIGHT_UNITS`] of its height (so a lookout's deck must be climbed to), opens it: the
//! authored item and copper go into the player's bags, once per character.
//!
//! The opened set is a durable character fact and the [`crate::RewardKey::Cache`] receipt guards
//! hosts that track rewards, so neither a replay, a reconnect nor a recovery pays a cache twice.
//! A full bag leaves the cache shut for that player until there is room.

use std::sync::Arc;

use crate::{
    CreatureId, ItemId, ItemStack, LootRewards, PlayerId, ZoneError, ZoneSimulation, settle_loot,
};

/// A player's feet open a cache only this close to its spot's height: 1 m.
pub const CACHE_HEIGHT_UNITS: i32 = 100;

/// One authored discovery cache (#913).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryCache {
    /// Stable nonzero ID, the durable receipt's key.
    pub id: u32,
    pub name: String,
    /// The spot: feet X, height and Z in units.
    pub at: [i32; 3],
    /// How near on XZ a player's feet must come, in units.
    pub reach: i32,
    /// Content spawns that keep the cache shut while any of them is alive.
    pub guards: Vec<CreatureId>,
    /// The authored reward: one item and some copper.
    pub item: ItemId,
    pub copper: u32,
}

impl DiscoveryCache {
    /// Whether feet at `feet` stand at the cache.
    #[must_use]
    pub fn within(&self, feet: [i32; 3]) -> bool {
        crate::reach::within_cylinder(self.at, self.reach, CACHE_HEIGHT_UNITS, feet)
    }
}

impl ZoneSimulation {
    /// Tick step after movement (#913): every living player standing at a cache it has not yet
    /// opened, once the cache's guards are down, opens it and takes its reward.
    pub(crate) fn open_caches(&mut self) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for cache in content.caches() {
            if cache
                .guards
                .iter()
                .any(|guard| self.creatures.get(guard).is_some_and(|c| c.is_alive()))
            {
                continue;
            }
            let ids: Vec<PlayerId> = self
                .players
                .iter()
                .filter(|(_, player)| player.is_alive() && !player.caches.contains(&cache.id))
                .map(|(&id, _)| id)
                .collect();
            for player_id in ids {
                let feet = self.player_feet(player_id)?;
                let key = crate::RewardKey::Cache { cache: cache.id };
                // Like a full bag, a tick without room for its receipt leaves the cache
                // closed until a later tick (#1112).
                if !cache.within(feet)
                    || !crate::rewards::allowed(&self.rewards, player_id, key)
                    || !crate::rewards::fits(&self.rewards, 1)
                {
                    continue;
                }
                let player = self
                    .players
                    .get_mut(&player_id)
                    .ok_or_else(|| ZoneError::new("cache opener is absent"))?;
                let rewards = LootRewards {
                    money: cache.copper,
                    item: Some(
                        ItemStack::new(cache.item, 1)
                            .map_err(|_| ZoneError::new("a cache's item is unknown"))?,
                    ),
                    quest_item: None,
                };
                let mut bag = player.inventory.clone();
                let mut copper = player.copper;
                if settle_loot(&mut bag, &mut copper, rewards).is_err() {
                    continue;
                }
                let Some(revision) = player.inventory_revision.checked_add(1) else {
                    continue;
                };
                player.inventory = bag;
                player.copper = copper;
                player.inventory_revision = revision;
                player.caches.insert(cache.id);
                crate::rewards::record(&mut self.rewards, player_id, key)?;
            }
        }
        Ok(())
    }
}
