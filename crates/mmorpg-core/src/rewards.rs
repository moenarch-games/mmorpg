//! Deterministic entitlement receipts. The core decides whether a reward may be
//! applied; storage later commits the emitted IDs with the resulting character
//! facts. Durable character/world identity prefixes belong to the host adapter.
//!
//! Reward capacity never aborts a tick (#1112, Atlas 3.2.i1112). A reward a player asks
//! for (claiming corpse loot, turning in a quest) that does not fit is refused with
//! [`crate::ErrorCode::RewardsBusy`]; a cache or a discovery that does not fit simply waits,
//! like a cache behind a full bag. A reward the zone pays on its own (kill credit, raid and
//! cart payouts) that does not fit joins the canonical [`RewardHistory::deferred`] backlog and
//! is paid first thing in a later tick, oldest first.
use crate::{AreaId, CreatureId, ItemId, MAX_PLAYERS_PER_ZONE, PlayerId, QuestId, ZoneError};
use std::collections::{BTreeMap, BTreeSet};

/// The receipts a zone reserves for each character it can hold (#1112): a full zone of
/// characters with this many receipts each fits the default bound.
pub const RECEIPTS_PER_CHARACTER: u32 = 256;
/// Default zone-wide receipt bound: [`RECEIPTS_PER_CHARACTER`] for each of the zone's
/// [`MAX_PLAYERS_PER_ZONE`] players.
pub const DEFAULT_MAX_KNOWN_RECEIPTS: u32 = MAX_PLAYERS_PER_ZONE as u32 * RECEIPTS_PER_CHARACTER;
/// Zone payouts one player can earn in a single tick at most (a raid's participation and
/// defence, a cart defence and a kill), so the default tick bound pays a full zone at once.
pub const ZONE_PAYOUTS_PER_PLAYER: u16 = 4;
/// Default per-tick grant bound: [`ZONE_PAYOUTS_PER_PLAYER`] for each of the zone's players.
pub const DEFAULT_MAX_TICK_GRANTS: u16 = MAX_PLAYERS_PER_ZONE as u16 * ZONE_PAYOUTS_PER_PLAYER;
/// Deferred zone payouts a zone holds at most: eight for each player it can hold. Only
/// limits configured below the defaults, or a zone whose receipt bound is exhausted, reach
/// it; a payout beyond it is withheld without consuming its receipt.
pub const MAX_DEFERRED_REWARDS: usize = MAX_PLAYERS_PER_ZONE * 8;

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
    /// A night raid's participation reward (#531): once per incident and character.
    RaidParticipation {
        incident: u32,
    },
    /// A defended night raid's bonus (#916): once per incident and character.
    RaidDefence {
        incident: u32,
    },
    /// The first visit to a discovery area (#916): once per area and character.
    Discovery {
        area: AreaId,
    },
    /// An opened discovery cache (#913): once per cache and character.
    Cache {
        cache: u32,
    },
    /// Defending a merchant's cart from its ambush (#992): once per incident and character.
    CartDefence {
        incident: u32,
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
    /// Whether the zone pays this reward on its own, so it waits in the deferred backlog when
    /// its tick has no room (#1112).
    #[must_use]
    pub const fn zone_paid(self) -> bool {
        matches!(
            self,
            Self::KillExperience { .. }
                | Self::KillObjective { .. }
                | Self::RaidParticipation { .. }
                | Self::RaidDefence { .. }
                | Self::CartDefence { .. }
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
            Self::QuestCompletion { .. }
            | Self::RaidParticipation { .. }
            | Self::RaidDefence { .. }
            | Self::Discovery { .. }
            | Self::Cache { .. }
            | Self::CartDefence { .. } => None,
        }
    }
    pub fn validate(self) -> Result<(), ZoneError> {
        if self.creature_life().is_some_and(|(_, life)| life == 0) {
            return Err(ZoneError::new("reward creature life must be nonzero"));
        }
        match self {
            Self::KillObjective {
                quest, objective, ..
            } if quest.get() == 0 || usize::from(objective) >= crate::MAX_QUEST_OBJECTIVES => {
                Err(ZoneError::new("invalid reward quest objective"))
            }
            Self::QuestCompletion { quest } if quest.get() == 0 => {
                Err(ZoneError::new("invalid reward quest"))
            }
            Self::RaidParticipation { incident: 0 }
            | Self::RaidDefence { incident: 0 }
            | Self::CartDefence { incident: 0 } => {
                Err(ZoneError::new("invalid reward raid incident"))
            }
            Self::Cache { cache: 0 } => Err(ZoneError::new("invalid reward cache")),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RewardGrant {
    pub player: PlayerId,
    pub key: RewardKey,
}

/// What a deferred zone payout pays once it fits (#1112). A kill objective pays nothing
/// here: its quest progress is re-read when it is paid.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct RewardPayout {
    pub experience: u32,
    pub copper: u32,
}

/// A zone payout that did not fit its tick (#1112). The backlog orders entries by the tick
/// they were earned in, then recipient and key, so a recovered zone pays them in exactly the
/// order the unbroken run does.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DeferredReward {
    pub since: u64,
    pub grant: RewardGrant,
    pub payout: RewardPayout,
}

/// Exact consumed receipts, this completed tick's newly granted receipts and the zone
/// payouts still waiting for room. Limits are supplied by host configuration and persisted
/// for deterministic replay. A bound never evicts a still-relevant ID and never fails a
/// tick: callers ask [`Self::fits`] first and refuse, wait or defer (#1112).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardHistory {
    pub max_known_ids: u32,
    pub max_tick_grants: u16,
    pub world: BTreeSet<RewardKey>,
    pub players: BTreeMap<PlayerId, BTreeSet<RewardKey>>,
    pub grants: BTreeSet<RewardGrant>,
    /// Zone payouts waiting for room, oldest first; at most [`MAX_DEFERRED_REWARDS`].
    pub deferred: BTreeSet<DeferredReward>,
}
impl RewardHistory {
    pub fn new(max_known_ids: u32, max_tick_grants: u16) -> Result<Self, ZoneError> {
        if max_known_ids == 0 || max_tick_grants == 0 {
            return Err(ZoneError::new("reward tracking limits must be nonzero"));
        }
        Ok(Self {
            max_known_ids,
            max_tick_grants,
            world: BTreeSet::new(),
            players: BTreeMap::new(),
            grants: BTreeSet::new(),
            deferred: BTreeSet::new(),
        })
    }
    #[must_use]
    pub fn known_count(&self) -> usize {
        self.world.len() + self.players.values().map(BTreeSet::len).sum::<usize>()
    }
    /// New receipts this tick can still record: the smaller of the tick's remaining grants
    /// and the zone's remaining receipts.
    #[must_use]
    pub fn room(&self) -> usize {
        let grants = usize::from(self.max_tick_grants).saturating_sub(self.grants.len());
        let known = (self.max_known_ids as usize).saturating_sub(self.known_count());
        grants.min(known)
    }
    /// Whether `receipts` new receipts fit this tick.
    #[must_use]
    pub fn fits(&self, receipts: usize) -> bool {
        self.room() >= receipts
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
                && !self.is_deferred(player, key)
        }
    }
    /// Whether the payout already waits in the backlog. Only zone-paid keys can, so other
    /// lookups never scan it.
    fn is_deferred(&self, player: PlayerId, key: RewardKey) -> bool {
        key.zone_paid()
            && !self.deferred.is_empty()
            && self
                .deferred
                .iter()
                .any(|entry| entry.grant == RewardGrant { player, key })
    }
    /// Load already committed receipts before character activation. Loading does
    /// not emit a new grant and is idempotent across paged trusted inputs. A receipt
    /// beyond the zone's bound refuses the admission it belongs to (#1112).
    pub fn load(&mut self, player: PlayerId, key: RewardKey) -> Result<(), ZoneError> {
        key.validate()?;
        if player == 0 {
            return Err(ZoneError::new("reward recipient must be nonzero"));
        }
        if !self.allows(player, key) {
            return Ok(());
        }
        if self.known_count() >= self.max_known_ids as usize {
            return Err(ZoneError::new(
                "the zone's reward receipt bound cannot hold this character's stored receipts",
            ));
        }
        self.insert(player, key);
        Ok(())
    }
    fn insert(&mut self, player: PlayerId, key: RewardKey) {
        if key.world_owned() {
            self.world.insert(key);
        } else {
            self.players.entry(player).or_default().insert(key);
        }
    }
    /// Called only after the core has accepted the reward and checked [`Self::fits`], so
    /// an error here is a broken core invariant, never a capacity outcome.
    pub fn record(&mut self, player: PlayerId, key: RewardKey) -> Result<(), ZoneError> {
        if !self.allows(player, key) {
            return Err(ZoneError::new("reward already consumed"));
        }
        if !self.fits(1) {
            return Err(ZoneError::new("reward recorded without room"));
        }
        key.validate()?;
        if player == 0 {
            return Err(ZoneError::new("reward recipient must be nonzero"));
        }
        self.insert(player, key);
        self.grants.insert(RewardGrant { player, key });
        Ok(())
    }
    /// Queues a zone payout for a later tick. Beyond [`MAX_DEFERRED_REWARDS`] the payout is
    /// withheld without consuming its receipt.
    pub fn defer(&mut self, since: u64, player: PlayerId, key: RewardKey, payout: RewardPayout) {
        if self.deferred.len() < MAX_DEFERRED_REWARDS {
            self.deferred.insert(DeferredReward {
                since,
                grant: RewardGrant { player, key },
                payout,
            });
        }
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
    /// A departing player's receipts, grants and still deferred payouts leave with it; an
    /// unpaid payout never consumed its receipt.
    pub fn remove_player(&mut self, player: PlayerId) {
        self.players.remove(&player);
        self.grants.retain(|grant| grant.player != player);
        if !self.deferred.is_empty() {
            self.deferred.retain(|entry| entry.grant.player != player);
        }
    }
    pub fn validate(&self) -> Result<(), ZoneError> {
        if self.players.len() > crate::MAX_PLAYERS_PER_ZONE
            || self.max_known_ids == 0
            || self.max_tick_grants == 0
            || self.known_count() > self.max_known_ids as usize
            || self.grants.len() > usize::from(self.max_tick_grants)
            || self.deferred.len() > MAX_DEFERRED_REWARDS
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
        let mut waiting = BTreeSet::new();
        for entry in &self.deferred {
            let RewardGrant { player, key } = entry.grant;
            key.validate()?;
            if player == 0
                || !key.zone_paid()
                || !waiting.insert(entry.grant)
                || self
                    .players
                    .get(&player)
                    .is_some_and(|keys| keys.contains(&key))
            {
                return Err(ZoneError::new("invalid deferred reward"));
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
/// Whether `receipts` new receipts fit this tick; always without reward tracking.
pub(crate) fn fits(history: &Option<RewardHistory>, receipts: usize) -> bool {
    history
        .as_ref()
        .is_none_or(|history| history.fits(receipts))
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
    /// Pays a reward the zone grants on its own (kill credit, raid and cart payouts) whose
    /// receipt the caller found unconsumed. Without room this tick, or behind older waiting
    /// payouts, it joins the deferred backlog instead (#1112); it never fails on capacity.
    pub(crate) fn pay_zone_reward(
        &mut self,
        player: PlayerId,
        key: RewardKey,
        payout: RewardPayout,
    ) -> Result<(), ZoneError> {
        let since = self.tick;
        if let Some(history) = &mut self.rewards
            && (!history.deferred.is_empty() || !history.fits(1))
        {
            history.defer(since, player, key, payout);
            return Ok(());
        }
        if self.apply_payout(player, key, payout)? {
            record(&mut self.rewards, player, key)?;
        }
        Ok(())
    }

    /// Applies a zone payout to a present player; `false` when nothing was paid (an absent
    /// player, or a kill objective that is no longer open), so no receipt is consumed.
    fn apply_payout(
        &mut self,
        player_id: PlayerId,
        key: RewardKey,
        payout: RewardPayout,
    ) -> Result<bool, ZoneError> {
        if let RewardKey::KillObjective {
            quest, objective, ..
        } = key
        {
            return Ok(self.advance_kill_objective(player_id, quest, objective));
        }
        let Some(player) = self.players.get_mut(&player_id) else {
            return Ok(false);
        };
        player.copper = player.copper.saturating_add(payout.copper);
        self.grant_experience(player_id, payout.experience)?;
        Ok(true)
    }

    /// Tick step before intents (#1112): pays deferred zone payouts oldest first while the
    /// tick has room. Payouts whose player left or whose objective closed are dropped
    /// without a receipt. Work: one visit per paid or dropped entry, so at most the tick's
    /// grant bound plus the dropped entries.
    pub(crate) fn pay_deferred_rewards(&mut self) -> Result<(), ZoneError> {
        let Some(history) = &self.rewards else {
            return Ok(());
        };
        if history.deferred.is_empty() {
            return Ok(());
        }
        let mut room = history.room();
        while room > 0 {
            let Some(entry) = self
                .rewards
                .as_mut()
                .and_then(|history| history.deferred.pop_first())
            else {
                break;
            };
            self.tick_work.deferred_rewards_visited += 1;
            let RewardGrant { player, key } = entry.grant;
            if self.apply_payout(player, key, entry.payout)? {
                record(&mut self.rewards, player, key)?;
                room -= 1;
            }
        }
        Ok(())
    }

    /// Configure a fresh authoritative world before its genesis checkpoint. The host's
    /// defaults are [`DEFAULT_MAX_KNOWN_RECEIPTS`] and [`DEFAULT_MAX_TICK_GRANTS`].
    pub fn enable_reward_tracking(
        &mut self,
        max_known_ids: u32,
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
