//! Divine Reckoning (#936, Atlas 5.2.i936): a Cleric remembers the harm each enemy deals.
//!
//! Every hit a living creature lands on a Cleric, or on a non-hostile player within
//! [`RECKONING_ALLY_RADIUS`] of an in-combat Cleric, adds the health it took to that Cleric's
//! pool against that one creature, up to the ability's cap. Healing never lowers a pool. Props,
//! boulders and environmental damage add nothing, and only a creature's own hits count, so one
//! enemy's harm never lands on another. A pool ends when its creature dies or stops fighting,
//! and every pool ends when the Cleric dies or leaves combat. Casting Divine Reckoning on a
//! creature converts a share of its pool into a holy strike and consumes only that pool.
//!
//! Pools are canonical (ascending creature ID), so recovery and replay continue exactly.

use crate::ability::{AbilityEffect, ability_by_id, ids};
use crate::snapshot::CreatureAi;
use crate::{CreatureId, EntityRef, PlayerClass, PlayerId, ZoneError, ZoneSimulation};

/// Harm by a creature to a non-hostile player this close (XZ units) to the Cleric counts.
pub const RECKONING_ALLY_RADIUS: i32 = 3_000;
/// A Cleric remembers at most this many enemies at once; harm by a further one is not stored.
pub const MAX_RECKONING_POOLS: usize = 16;

/// The harm one creature dealt within a Cleric's reach this encounter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Harm {
    pub creature: CreatureId,
    pub amount: u16,
}

/// Divine Reckoning's `(percent, pool cap)` from the catalog.
pub(crate) fn reckoning_rule() -> (u16, u16) {
    match ability_by_id(ids::DIVINE_RECKONING).map(|ability| ability.effect) {
        Some(AbilityEffect::Reckoning { percent, pool }) => (percent, pool),
        _ => (0, 0),
    }
}

/// The harm stored against `creature` in `pools`.
pub(crate) fn stored(pools: &[Harm], creature: CreatureId) -> u16 {
    pools
        .binary_search_by_key(&creature, |harm| harm.creature)
        .map_or(0, |index| pools[index].amount)
}

impl ZoneSimulation {
    /// `creature_id` took `dealt` health from `victim`: every Cleric it concerns remembers it.
    pub(crate) fn remember_harm(
        &mut self,
        creature_id: CreatureId,
        victim: PlayerId,
        dealt: u32,
    ) -> Result<(), ZoneError> {
        let creature = EntityRef::Creature(creature_id);
        if dealt == 0 || self.is_prop(creature) || self.is_boulder(creature_id) {
            return Ok(());
        }
        let at = self.player_position(victim)?;
        let (_, cap) = reckoning_rule();
        let clerics: Vec<PlayerId> = self
            .players
            .iter()
            .filter(|(_, player)| {
                player.is_alive()
                    && player
                        .class
                        .is_some_and(|choice| choice.class == PlayerClass::Cleric)
            })
            .map(|(&id, _)| id)
            .collect();
        for cleric in clerics {
            if cleric != victim {
                // In combat as the zone counts it: lately fighting, or on a threat table.
                if !self.player_in_combat(cleric) || self.players_are_hostile(cleric, victim) {
                    continue;
                }
                let from = self.player_position(cleric)?;
                let (dx, dz) = (i64::from(from.x - at.x), i64::from(from.z - at.z));
                if dx * dx + dz * dz > i64::from(RECKONING_ALLY_RADIUS).pow(2) {
                    continue;
                }
            }
            self.update_player(cleric, |player| {
                let pools = &mut player.reckoning;
                let index = match pools.binary_search_by_key(&creature_id, |harm| harm.creature) {
                    Ok(index) => index,
                    Err(_) if pools.len() >= MAX_RECKONING_POOLS => return,
                    Err(index) => {
                        pools.insert(
                            index,
                            Harm {
                                creature: creature_id,
                                amount: 0,
                            },
                        );
                        index
                    }
                };
                let added = u16::try_from(dealt).unwrap_or(u16::MAX);
                pools[index].amount = pools[index].amount.saturating_add(added).min(cap);
            });
        }
        Ok(())
    }

    /// Ends the pools of dead or disengaged Clerics and of creatures that stopped fighting.
    pub(crate) fn expire_harm(&mut self) {
        if self
            .players
            .values()
            .all(|player| player.reckoning.is_empty())
        {
            return;
        }
        let threatened = self.threatened_players();
        let creatures = &self.creatures;
        for (player_id, player) in &mut self.players {
            if player.reckoning.is_empty() {
                continue;
            }
            if !player.is_alive()
                || !crate::threat_cache::in_combat(player, *player_id, &threatened)
            {
                player.reckoning.clear();
                continue;
            }
            player.reckoning.retain(|harm| {
                creatures.get(&harm.creature).is_some_and(|creature| {
                    creature.is_alive() && creature.ai == CreatureAi::Engaged
                })
            });
        }
    }

    /// The stored harm a Divine Reckoning by `player_id` would consume on `target`.
    pub(crate) fn harm_against(&self, player_id: PlayerId, target: Option<EntityRef>) -> u16 {
        match (self.players.get(&player_id), target) {
            (Some(player), Some(EntityRef::Creature(creature))) => {
                stored(&player.reckoning, creature)
            }
            _ => 0,
        }
    }

    /// Consumes `player_id`'s pool against `target` and returns the strike it converts to.
    pub(crate) fn consume_harm(&mut self, player_id: PlayerId, target: EntityRef) -> u32 {
        let EntityRef::Creature(creature) = target else {
            return 0;
        };
        let (percent, _) = reckoning_rule();
        let mut harm = 0;
        self.update_player(player_id, |player| {
            if let Ok(index) = player
                .reckoning
                .binary_search_by_key(&creature, |harm| harm.creature)
            {
                harm = player.reckoning.remove(index).amount;
            }
        });
        u32::from(harm) * u32::from(percent) / 100
    }
}
