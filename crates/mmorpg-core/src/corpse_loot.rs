//! Corpse claim authority. Reward rules and bag settlement stay in `loot`;
//! this module owns eligibility, death fencing and the single commit point.

use crate::creature::Life;
use crate::{
    CreatureId, EntityRef, ErrorCode, LootRewards, LootSettlementError, PlayerId, ZoneError,
    ZoneSimulation, settle_loot,
};

/// Inclusive distance between authoritative body/corpse centres (3 m).
pub const LOOT_REACH_UNITS: i32 = 300;

/// One creature death; a respawn cannot reuse the earlier death tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LootClaim {
    pub creature: CreatureId,
    pub died_at: u64,
}

/// The selected eligible corpse's complete remaining rewards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LootView {
    pub claim: LootClaim,
    pub rewards: LootRewards,
}

pub(crate) fn has_rewards(rewards: LootRewards) -> bool {
    rewards.money != 0 || rewards.item.is_some() || rewards.quest_item.is_some()
}

enum Eligibility {
    Available(LootView),
    Refused(ErrorCode),
}

impl ZoneSimulation {
    fn loot_eligibility(
        &self,
        player_id: PlayerId,
        claim: LootClaim,
        now: u64,
    ) -> Result<Eligibility, ZoneError> {
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("loot owner is absent"))?;
        if !player.is_alive() {
            return Ok(Eligibility::Refused(ErrorCode::YouAreDead));
        }
        let Some(creature) = self.creatures.get(&claim.creature) else {
            return Ok(Eligibility::Refused(ErrorCode::InvalidLoot));
        };
        let Life::Corpse { died_at, position } = creature.life else {
            return Ok(Eligibility::Refused(ErrorCode::InvalidLoot));
        };
        if died_at != claim.died_at
            || now >= died_at.saturating_add(u64::from(crate::unit::CORPSE_TICKS))
        {
            return Ok(Eligibility::Refused(ErrorCode::InvalidLoot));
        }
        if creature.tapped_by != Some(player_id) {
            return Ok(Eligibility::Refused(ErrorCode::NotLootOwner));
        }
        let from = self.player_position(player_id)?;
        let distance: i128 = [from.x, from.y, from.z]
            .into_iter()
            .zip(position)
            .map(|(a, b)| (i128::from(a) - i128::from(b)).pow(2))
            .sum();
        if distance > i128::from(LOOT_REACH_UNITS).pow(2) {
            return Ok(Eligibility::Refused(ErrorCode::OutOfRange));
        }
        let Some(rewards) = creature.loot.filter(|rewards| has_rewards(*rewards)) else {
            return Ok(Eligibility::Refused(ErrorCode::EmptyLoot));
        };
        Ok(Eligibility::Available(LootView { claim, rewards }))
    }

    pub(crate) fn loot_view_for(
        &self,
        player_id: PlayerId,
        creature_id: CreatureId,
    ) -> Result<Option<LootView>, ZoneError> {
        let Some(creature) = self.creatures.get(&creature_id) else {
            return Ok(None);
        };
        let Life::Corpse { died_at, .. } = creature.life else {
            return Ok(None);
        };
        let claim = LootClaim {
            creature: creature_id,
            died_at,
        };
        Ok(match self.loot_eligibility(player_id, claim, self.tick)? {
            Eligibility::Available(view) => Some(view),
            Eligibility::Refused(_) => None,
        })
    }

    pub(crate) fn claim_loot(
        &mut self,
        player_id: PlayerId,
        claim: LootClaim,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let target = Some(EntityRef::Creature(claim.creature));
        let view = match self.loot_eligibility(player_id, claim, self.tick + 1)? {
            Eligibility::Available(view) => view,
            Eligibility::Refused(code) => return Ok(Some((code, target))),
        };
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("loot owner is absent"))?;
        let mut bag = player.inventory.clone();
        let mut copper = player.copper;
        if let Err(error) = settle_loot(&mut bag, &mut copper, view.rewards) {
            let code = match error {
                LootSettlementError::MoneyOverflow => ErrorCode::MoneyOverflow,
                LootSettlementError::Inventory(_) => ErrorCode::InventoryFull,
            };
            return Ok(Some((code, target)));
        }
        let revision = if bag != player.inventory {
            let Some(revision) = player.inventory_revision.checked_add(1) else {
                return Ok(Some((ErrorCode::InvalidInventoryMove, target)));
            };
            Some(revision)
        } else {
            None
        };
        let creature = self
            .creatures
            .get_mut(&claim.creature)
            .ok_or_else(|| ZoneError::new("claimed corpse is absent"))?;
        // All checks precede the commit; refusals leave both player and corpse intact.
        player.inventory = bag;
        player.copper = copper;
        if let Some(revision) = revision {
            player.inventory_revision = revision;
            player.inventory_changed_at = self.tick + 1;
        }
        creature.loot = None;
        if let Some(stack) = view.rewards.quest_item {
            self.report_collected(player_id, stack.item());
        }
        Ok(None)
    }
}
