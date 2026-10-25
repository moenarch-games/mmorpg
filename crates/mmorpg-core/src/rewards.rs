//! Deterministic entitlement receipts. The core decides whether a reward may be
//! applied; storage later commits the emitted IDs with the resulting character
//! facts. Durable character/world identity prefixes belong to the host adapter.
use crate::{CreatureId, ItemId, PlayerId, QuestId, ZoneError};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RewardKey {
    KillExperience {
        creature: CreatureId,
        life: u64,
    },
    KillObjective {
        creature: CreatureId,
        life: u64,
        quest: QuestId,
        objective: u8,
    },
    LootMoney {
        creature: CreatureId,
        life: u64,
    },
    LootItem {
        creature: CreatureId,
        life: u64,
    },
    LootQuestItem {
        creature: CreatureId,
        life: u64,
        item: ItemId,
    },
    QuestCompletion {
        quest: QuestId,
    },
}
impl RewardKey {
    #[must_use]
    pub const fn world_owned(self) -> bool {
        matches!(
            self,
            Self::LootMoney { .. } | Self::LootItem { .. } | Self::LootQuestItem { .. }
        )
    }
    #[must_use]
    pub const fn creature_life(self) -> Option<(CreatureId, u64)> {
        match self {
            Self::KillExperience { creature, life }
            | Self::KillObjective { creature, life, .. }
            | Self::LootMoney { creature, life }
            | Self::LootItem { creature, life }
            | Self::LootQuestItem { creature, life, .. } => Some((creature, life)),
            Self::QuestCompletion { .. } => None,
        }
    }
    pub fn validate(self) -> Result<(), ZoneError> {
        if self.creature_life().is_some_and(|(_, life)| life == 0) {
            return Err(ZoneError::new("reward creature life must be nonzero"));
        }
        match self {
            Self::KillObjective {
                quest, objective, ..
            } if quest.bit().is_none() || usize::from(objective) >= crate::MAX_QUEST_OBJECTIVES => {
                Err(ZoneError::new("invalid reward quest objective"))
            }
            Self::QuestCompletion { quest } if quest.bit().is_none() => {
                Err(ZoneError::new("invalid reward quest"))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RewardGrant {
    pub player: PlayerId,
    pub key: RewardKey,
}

/// Exact consumed receipts, plus this completed tick's newly granted receipts.
/// Limits are supplied by host configuration and persisted for deterministic
/// replay. Crossing a bound fails closed; it never evicts a still-relevant ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardHistory {
    pub max_known_ids: u16,
    pub max_tick_grants: u16,
    pub world: BTreeSet<RewardKey>,
    pub players: BTreeMap<PlayerId, BTreeSet<RewardKey>>,
    pub grants: BTreeSet<RewardGrant>,
}
impl RewardHistory {
    pub fn new(max_known_ids: u16, max_tick_grants: u16) -> Result<Self, ZoneError> {
        if max_known_ids == 0 || max_tick_grants == 0 {
            return Err(ZoneError::new("reward tracking limits must be nonzero"));
        }
        Ok(Self {
            max_known_ids,
            max_tick_grants,
            world: BTreeSet::new(),
            players: BTreeMap::new(),
            grants: BTreeSet::new(),
        })
    }
    #[must_use]
    pub fn known_count(&self) -> usize {
        self.world.len() + self.players.values().map(BTreeSet::len).sum::<usize>()
    }
    #[must_use]
    pub fn allows(&self, player: PlayerId, key: RewardKey) -> bool {
        if key.world_owned() {
            !self.world.contains(&key)
        } else {
            !self
                .players
                .get(&player)
                .is_some_and(|keys| keys.contains(&key))
        }
    }
    /// Load already committed receipts before character activation. Loading does
    /// not emit a new grant and is idempotent across paged trusted inputs.
    pub fn load(&mut self, player: PlayerId, key: RewardKey) -> Result<(), ZoneError> {
        key.validate()?;
        if player == 0 {
            return Err(ZoneError::new("reward recipient must be nonzero"));
        }
        if !self.allows(player, key) {
            return Ok(());
        }
        if self.known_count() >= usize::from(self.max_known_ids) {
            return Err(ZoneError::new("reward receipt capacity reached"));
        }
        if key.world_owned() {
            self.world.insert(key);
        } else {
            self.players.entry(player).or_default().insert(key);
        }
        Ok(())
    }
    /// Called only after the core has accepted the reward. Capacity errors abort
    /// the unpublished tick; the host must recover its last committed state.
    pub fn record(&mut self, player: PlayerId, key: RewardKey) -> Result<(), ZoneError> {
        if !self.allows(player, key) {
            return Err(ZoneError::new("reward already consumed"));
        }
        if self.grants.len() >= usize::from(self.max_tick_grants) {
            return Err(ZoneError::new("tick reward capacity reached"));
        }
        self.load(player, key)?;
        self.grants.insert(RewardGrant { player, key });
        Ok(())
    }
    pub fn begin_tick(&mut self) {
        self.grants.clear();
    }
    /// Earlier lives can never recur in a running authority. Explicit weaker
    /// world restore must reload relevant durable receipts before activation.
    pub fn spawned(&mut self, creature: CreatureId, life: u64) {
        let keep = |key: &RewardKey| {
            !key.creature_life()
                .is_some_and(|(id, previous)| id == creature && previous < life)
        };
        self.world.retain(keep);
        for keys in self.players.values_mut() {
            keys.retain(keep);
        }
    }
    pub fn remove_player(&mut self, player: PlayerId) {
        self.players.remove(&player);
        self.grants.retain(|grant| grant.player != player);
    }
    pub fn validate(&self) -> Result<(), ZoneError> {
        if self.players.len() > crate::MAX_PLAYERS_PER_ZONE
            || self.max_known_ids == 0
            || self.max_tick_grants == 0
            || self.known_count() > usize::from(self.max_known_ids)
            || self.grants.len() > usize::from(self.max_tick_grants)
        {
            return Err(ZoneError::new("invalid reward history limits"));
        }
        for key in &self.world {
            key.validate()?;
            if !key.world_owned() {
                return Err(ZoneError::new("personal reward in world receipt set"));
            }
        }
        for (player, keys) in &self.players {
            if *player == 0 {
                return Err(ZoneError::new("invalid reward history player"));
            }
            for key in keys {
                key.validate()?;
                if key.world_owned() {
                    return Err(ZoneError::new("world reward in personal receipt set"));
                }
            }
        }
        for grant in &self.grants {
            grant.key.validate()?;
            if grant.player == 0 || self.allows(grant.player, grant.key) {
                return Err(ZoneError::new("emitted reward has no consumed receipt"));
            }
        }
        Ok(())
    }
}

pub(crate) fn allowed(history: &Option<RewardHistory>, player: PlayerId, key: RewardKey) -> bool {
    history
        .as_ref()
        .is_none_or(|history| history.allows(player, key))
}
pub(crate) fn record(
    history: &mut Option<RewardHistory>,
    player: PlayerId,
    key: RewardKey,
) -> Result<(), ZoneError> {
    if let Some(history) = history {
        history.record(player, key)?;
    }
    Ok(())
}
impl crate::ZoneSimulation {
    /// Configure a fresh authoritative world before its genesis checkpoint.
    pub fn enable_reward_tracking(
        &mut self,
        max_known_ids: u16,
        max_tick_grants: u16,
    ) -> Result<(), ZoneError> {
        if self.tick != 0 || self.rewards.is_some() {
            return Err(ZoneError::new(
                "reward tracking must be configured once before the first tick",
            ));
        }
        self.rewards = Some(RewardHistory::new(max_known_ids, max_tick_grants)?);
        Ok(())
    }
    pub fn load_reward_receipt(
        &mut self,
        player: PlayerId,
        key: RewardKey,
    ) -> Result<(), ZoneError> {
        key.validate()?;
        let history = self
            .rewards
            .as_mut()
            .ok_or_else(|| ZoneError::new("reward tracking is disabled"))?;
        if !key.world_owned() && !self.players.contains_key(&player) {
            return Err(ZoneError::new("reward receipt player is absent"));
        }
        if key
            .creature_life()
            .is_some_and(|(creature, _)| !self.creatures.contains_key(&creature))
        {
            return Err(ZoneError::new("reward receipt creature is absent"));
        }
        if key.creature_life().is_some_and(|(id, life)| {
            self.creatures
                .get(&id)
                .is_some_and(|creature| life < creature.generation)
        }) {
            return Ok(());
        }
        history.load(player, key)
    }
    /// The adapter uses this only while a session is still an unbound placeholder.
    pub fn clear_loaded_reward_receipts(&mut self, player: PlayerId) -> Result<(), ZoneError> {
        if !self.players.contains_key(&player) {
            return Err(ZoneError::new("receipt player is absent"));
        }
        self.rewards
            .as_mut()
            .ok_or_else(|| ZoneError::new("reward tracking is disabled"))?
            .remove_player(player);
        Ok(())
    }
    pub fn creature_lives(&self) -> impl Iterator<Item = (CreatureId, u64)> + '_ {
        self.creatures
            .iter()
            .map(|(id, state)| (*id, state.generation))
    }
    #[must_use]
    pub fn reward_history(&self) -> Option<&RewardHistory> {
        self.rewards.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reward_history_preserves_exact_personal_and_world_receipts_across_reset() {
        let creature = CreatureId::new(1);
        let xp = RewardKey::KillExperience { creature, life: 3 };
        let money = RewardKey::LootMoney { creature, life: 3 };
        let mut history = RewardHistory::new(4, 4).unwrap();
        history.record(1, xp).unwrap();
        history.record(1, money).unwrap();
        assert!(!history.allows(1, xp));
        assert!(history.allows(2, xp));
        assert!(!history.allows(2, money));
        let mut reset = RewardHistory::new(4, 4).unwrap();
        reset.load(1, xp).unwrap();
        reset.load(1, money).unwrap();
        assert!(reset.grants.is_empty());
        assert!(!reset.allows(1, xp));
        assert!(!reset.allows(2, money));
        assert!(reset.allows(1, RewardKey::KillExperience { creature, life: 2 })); // gaps are not high-water marks
        reset.spawned(creature, 4);
        assert!(reset.world.is_empty());
        assert!(reset.allows(1, RewardKey::KillExperience { creature, life: 4 }));
        history.validate().unwrap();
    }
}
