//! Captain Ulric Thorn and his shared loot (#404 part 3, ADR 0010, Atlas 5.1.28, 5.1.31 to
//! 5.1.35, 5.1.37, 5.1.83).
//!
//! Thorn stands only once every chamber of a run is cleared. The scaling is his adds and his
//! mechanics, chosen from the run's frozen party size (5.1.26, 5.1.27), never health or damage
//! multipliers: at half health he calls his adds and starts his telegraphed area attack, and
//! parties large enough face a second area attack in his last phase. Intimidation is flavour.
//!
//! - **Attempts** (5.1.35): the first engagement inside his chamber snapshots the roster
//!   characters present, by character identity. A wipe or an evade ends the attempt: he stands
//!   again at full health, his adds go, his mechanics reset, and nothing already cleared or
//!   earned changes (5.1.28).
//! - **Completion.** Thorn's death ends the run's fighting; the run keeps its room for looting
//!   and leaving and can never be fought again.
//! - **The shared pool** (5.1.32): each kill drops one or two items, more with a larger party.
//!   Every drop is identified by `(run, drop index)` and has at most one winner. A drop opens a
//!   tick-based Need/Greed/Pass window (5.1.34) for the attempt's eligible characters; a Pass
//!   by everyone, or a non-answer at the deadline, leaves it manually claimable until the run
//!   ends (5.1.83).
//! - **Committed grants.** Every outcome (the kill and each settled drop) raises the run's
//!   outcome version. An item reaches a bag, and boss experience and the durable victory
//!   evidence (5.1.37) are given, only once the host reports that the run directory recorded
//!   that version ([`ZoneSimulation::confirm_boss_outcome`]), and only with a receipt in the
//!   character record in the same step. A crash can lose an uncommitted grant, never duplicate
//!   a committed one: a restored run keeps its outcome and never rerolls.

use std::collections::BTreeSet;

use crate::creature::Life;
use crate::snapshot::CreatureAi;
use crate::{
    AbilityId, CharacterId, CreatureId, DepthsSpawn, EntityRef, ErrorCode, ItemId,
    MAX_PARTY_MEMBERS, PlayerId, XzBounds, ZoneError, ZoneEvent, ZoneSimulation, item_template,
};

/// The most items one kill can drop.
pub const MAX_BOSS_DROPS: usize = 2;
/// Thorn's health percent at which he calls his adds and starts his area attack.
pub const BOSS_ADDS_PERCENT: u32 = 50;
/// Thorn's health percent at which his last phase begins.
pub const BOSS_LAST_PHASE_PERCENT: u32 = 25;

/// The authored boss of a depths site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthsBoss {
    pub creature: CreatureId,
    /// Standing here when combat begins makes a roster character eligible (5.1.35).
    pub area: XzBounds,
    /// The adds he calls at half health, chosen by the frozen party size.
    pub adds: Vec<DepthsSpawn>,
    /// The shared pool's items.
    pub pool: Vec<ItemId>,
    /// `pool` items drop per entering party size `1..=MAX_PARTY_MEMBERS`.
    pub drops_by_party: [u8; MAX_PARTY_MEMBERS],
    /// The Need/Greed window, in ticks (5.1.34).
    pub roll_ticks: u32,
    /// The telegraphed area attack he uses from half health.
    pub area_attack: AbilityId,
    /// The second area attack of his last phase, for parties of at least `final_attack_party`.
    pub final_attack: AbilityId,
    pub final_attack_party: u8,
}

impl DepthsBoss {
    pub(crate) fn validate(&self, content: &crate::ZoneContent) -> Result<(), ZoneError> {
        let distinct = self
            .adds
            .iter()
            .map(|add| add.creature)
            .collect::<BTreeSet<_>>();
        if content.creature_spawn(self.creature).is_none()
            || self.adds.iter().any(|add| {
                add.party[0] < 1
                    || usize::from(add.party[1]) > MAX_PARTY_MEMBERS
                    || add.party[0] > add.party[1]
                    || content.creature_spawn(add.creature).is_none()
            })
            || distinct.len() != self.adds.len()
            || distinct.contains(&self.creature)
        {
            return Err(ZoneError::new("a depths boss needs distinct authored adds"));
        }
        if self.pool.is_empty()
            || self
                .pool
                .iter()
                .any(|item| item_template(*item).is_none_or(|item| item.max_stack != 1))
            || self.drops_by_party.iter().any(|drops| {
                *drops == 0 || usize::from(*drops) > MAX_BOSS_DROPS.min(self.pool.len())
            })
            || self.roll_ticks == 0
            || crate::ability_by_id(self.area_attack).is_none()
            || crate::ability_by_id(self.final_attack).is_none()
            || self.final_attack_party == 0
        {
            return Err(ZoneError::new("invalid depths boss loot or attacks"));
        }
        Ok(())
    }
}

/// One character's answer to a roll window (5.1.34).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RollChoice {
    Need,
    Greed,
    Pass,
}

/// The state of one boss drop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DropState {
    /// An open window: the tick it closes and the answers so far, in character order.
    Rolling {
        deadline: u64,
        choices: Vec<(CharacterId, RollChoice)>,
    },
    /// Everyone passed: any eligible character may claim it until the run ends (5.1.83).
    Held,
    /// Decided: `roll` is 0 for a manual claim. Reserved for `winner` from this point on;
    /// `version` is the outcome version that decided it and `delivered` says the item reached
    /// the winner's bag.
    Awarded {
        winner: CharacterId,
        roll: u8,
        need: bool,
        version: u32,
        delivered: bool,
        /// Whether a full bag was already reported to the winner.
        full_notified: bool,
    },
}

/// One item of the shared pool; `(run_id, its index)` is its stable identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BossDrop {
    pub item: ItemId,
    pub state: DropState,
}

/// A run's boss outcome. Canonical; a host also records it in the run directory so a restarted
/// host restores it instead of a second kill.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BossOutcome {
    /// Raised by the kill and by every settled drop.
    pub version: u32,
    /// The newest version the run directory has confirmed.
    pub confirmed: u32,
    /// Seeds the deterministic rolls of this kill.
    pub roll_seed: u64,
    /// Characters eligible for this kill's loot and credit, in character order (5.1.35).
    pub eligible: Vec<CharacterId>,
    pub drops: Vec<BossDrop>,
    /// Characters whose boss credit (experience, victory evidence) was given.
    pub credited: Vec<CharacterId>,
}

/// The fight currently running.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BossAttempt {
    /// Characters present when combat began, in character order (5.1.35).
    pub eligible: Vec<CharacterId>,
    /// 0 before half health, 1 until his last phase, then 2.
    pub phase: u8,
    pub adds_called: bool,
    /// The last phase's two attacks alternate.
    pub next_is_final: bool,
}

/// The boss part of a run. Canonical.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BossProgress {
    pub attempt: Option<BossAttempt>,
    pub outcome: Option<BossOutcome>,
    /// Whether the chamber-open notice was sent.
    pub announced: bool,
}

/// What the host records in the run directory: a version, the outcome as of that version and
/// the characters who still need the run (answers or an undelivered award).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BossReport {
    pub run_id: u64,
    pub outcome: BossOutcome,
    pub outstanding: Vec<CharacterId>,
}

/// One drop as a player sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropView {
    /// An open window: the tick it closes, this viewer's answer and whether it may Need.
    Rolling {
        deadline: u64,
        answer: Option<RollChoice>,
        may_need: bool,
    },
    /// Nobody took it; `claimable` when this viewer may.
    Held { claimable: bool },
    /// Decided; `pending` until it reached the winner's bag, `roll` 0 for a manual claim.
    Awarded {
        won: bool,
        roll: u8,
        need: bool,
        pending: bool,
    },
}

/// A viewer's copy of the shared pool; `None` for characters not eligible for this kill.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthsLootView {
    pub run_id: u64,
    pub drops: Vec<DepthsDropView>,
}

/// One drop of [`DepthsLootView`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DepthsDropView {
    pub drop: u8,
    pub item: ItemId,
    pub state: DropView,
}

impl ZoneSimulation {
    fn depths_boss(&self) -> Option<&DepthsBoss> {
        self.content.depths_site()?.boss.as_ref()
    }

    /// Whether `creature` is this run's boss.
    pub(crate) fn is_depths_boss(&self, creature: CreatureId) -> bool {
        self.depths_boss()
            .is_some_and(|boss| boss.creature == creature)
    }

    /// Whether a death of `creature` pays no ordinary kill rewards: the boss's rewards are
    /// committed through the outcome, his adds pay nothing so a retry cannot farm them.
    pub(crate) fn pays_no_kill_rewards(&self, creature: CreatureId) -> bool {
        self.depths_boss().is_some_and(|boss| {
            boss.creature == creature || boss.adds.iter().any(|add| add.creature == creature)
        })
    }

    /// The live player of `character`, if any.
    fn player_of(&self, character: &CharacterId) -> Option<PlayerId> {
        self.players
            .iter()
            .find(|(_, state)| state.character.as_ref() == Some(character))
            .map(|(id, _)| *id)
    }

    /// Whether the player stands in the boss area and belongs to the roster; `Some(character)`.
    fn present_character(
        &self,
        player: PlayerId,
        area: XzBounds,
        run: &crate::DepthsRun,
    ) -> Option<CharacterId> {
        let state = self.players.get(&player)?;
        let character = state.character.clone()?;
        let at = self.player_position(player).ok()?;
        (run.admits(&character) && area.contains(at.x, at.z)).then_some(character)
    }

    /// The first engagement of the boss: snapshots the eligible characters (5.1.35).
    pub(crate) fn begin_boss_attempt(&mut self, creature: CreatureId) {
        if !self.is_depths_boss(creature) {
            return;
        }
        let Some(boss) = self.depths_boss() else {
            return;
        };
        let area = boss.area;
        let Some(run) = self.depths.as_ref() else {
            return;
        };
        if run.boss.outcome.is_some() || run.boss.attempt.is_some() {
            return;
        }
        let mut eligible: Vec<CharacterId> = self
            .players
            .keys()
            .filter_map(|player| self.present_character(*player, area, run))
            .collect();
        eligible.sort();
        eligible.dedup();
        if let Some(run) = self.depths.as_mut() {
            run.boss.attempt = Some(BossAttempt {
                eligible,
                phase: 0,
                adds_called: false,
                next_is_final: false,
            });
        }
    }

    /// The ability the boss uses now, when it has one: `None` before his area attack starts.
    pub(crate) fn depths_boss_ability(&self, creature: CreatureId) -> Option<AbilityId> {
        let boss = self.depths_boss()?;
        if boss.creature != creature {
            return None;
        }
        let run = self.depths.as_ref()?;
        let attempt = run.boss.attempt.as_ref()?;
        match attempt.phase {
            0 => None,
            1 => Some(boss.area_attack),
            _ if run.profile() >= boss.final_attack_party && attempt.next_is_final => {
                Some(boss.final_attack)
            }
            _ => Some(boss.area_attack),
        }
    }

    /// The boss started one of his attacks: the last phase's two alternate.
    pub(crate) fn boss_cast_started(&mut self, creature: CreatureId) {
        if self.is_depths_boss(creature)
            && let Some(attempt) = self
                .depths
                .as_mut()
                .and_then(|run| run.boss.attempt.as_mut())
            && attempt.phase == 2
        {
            attempt.next_is_final = !attempt.next_is_final;
        }
    }

    /// Tick step of the boss: appears when the chambers are cleared, advances the fight, closes
    /// roll windows and delivers committed rewards.
    pub(crate) fn advance_boss(
        &mut self,
        site: &crate::DepthsSite,
        now: u64,
    ) -> Result<(), ZoneError> {
        let Some(boss) = site.boss.as_ref() else {
            return Ok(());
        };
        let Some(run) = self.depths.as_ref() else {
            return Ok(());
        };
        let finished = run.boss.outcome.is_some();
        let all_cleared = run.cleared == crate::depths::chamber_mask(site.chambers.len());
        if !finished && all_cleared {
            self.raise_boss(boss)?;
        }
        if finished {
            self.settle_rolls(now)?;
            self.deliver_boss_rewards()?;
        } else {
            self.advance_attempt(boss)?;
        }
        Ok(())
    }

    /// Spawns Thorn the first tick his chamber is due; a retry finds him already standing.
    fn raise_boss(&mut self, boss: &DepthsBoss) -> Result<(), ZoneError> {
        let waiting = self
            .creatures
            .get(&boss.creature)
            .is_some_and(|creature| matches!(creature.life, Life::Despawned { died_at: 0 }));
        if waiting {
            self.spawn_creature(boss.creature)?;
        }
        let Some(run) = self.depths.as_mut() else {
            return Ok(());
        };
        if !run.boss.announced {
            run.boss.announced = true;
            let players: Vec<_> = self.players.keys().copied().collect();
            for player in players {
                self.notify(
                    player,
                    ZoneEvent::Error {
                        code: ErrorCode::DepthsBossOpen,
                        target: None,
                    },
                );
            }
        }
        Ok(())
    }

    /// The running attempt: called adds and area attacks by health, the end of an attempt.
    fn advance_attempt(&mut self, boss: &DepthsBoss) -> Result<(), ZoneError> {
        let Some(run) = self.depths.as_ref() else {
            return Ok(());
        };
        let Some(attempt) = run.boss.attempt.clone() else {
            return Ok(());
        };
        let profile = run.profile();
        let Some(creature) = self.creatures.get(&boss.creature) else {
            return Ok(());
        };
        if !creature.is_alive() {
            return Ok(());
        }
        // An evade or a wipe already took him out of the fight.
        if creature.ai != CreatureAi::Engaged {
            return self.end_boss_attempt(boss);
        }
        let content = std::sync::Arc::clone(&self.content);
        let (_, template) = Self::creature_content(&content, boss.creature)?;
        let max_health = creature.max_health(&content, template).max(1);
        let percent = u64::from(creature.health) * 100 / u64::from(max_health);
        let top_threat = creature
            .threat
            .iter()
            .max_by_key(|entry| entry.threat)
            .map(|entry| entry.entity);
        let mut changed = attempt.clone();
        if percent <= u64::from(BOSS_ADDS_PERCENT) && !attempt.adds_called {
            changed.adds_called = true;
            changed.phase = changed.phase.max(1);
            for add in &boss.adds {
                if add.stands_for(profile) {
                    self.spawn_creature(add.creature)?;
                    if let Some(target) = top_threat {
                        self.engage(add.creature, target, false)?;
                    }
                }
            }
            if let Some(creature) = self.creatures.get_mut(&boss.creature) {
                creature.ability_timer = 0;
            }
        }
        if percent <= u64::from(BOSS_LAST_PHASE_PERCENT) {
            changed.phase = 2;
        }
        if changed != attempt
            && let Some(run) = self.depths.as_mut()
        {
            run.boss.attempt = Some(changed);
        }
        Ok(())
    }

    /// Ends a running attempt: his adds stand down, ready to be called again.
    pub(crate) fn end_boss_attempt(&mut self, boss: &DepthsBoss) -> Result<(), ZoneError> {
        let adds: Vec<_> = boss.adds.iter().map(|add| add.creature).collect();
        for add in adds {
            self.stand_down(add, 0)?;
        }
        if let Some(run) = self.depths.as_mut() {
            run.boss.attempt = None;
        }
        Ok(())
    }

    /// The boss fell: the run is complete and the shared pool opens.
    pub(crate) fn boss_defeated(
        &mut self,
        creature: CreatureId,
        now: u64,
    ) -> Result<(), ZoneError> {
        if !self.is_depths_boss(creature) {
            return Ok(());
        }
        let Some(boss) = self.depths_boss().cloned() else {
            return Ok(());
        };
        let Some(run) = self.depths.as_ref() else {
            return Ok(());
        };
        if run.boss.outcome.is_some() {
            return Ok(());
        }
        let eligible = match run.boss.attempt.as_ref() {
            Some(attempt) if !attempt.eligible.is_empty() => attempt.eligible.clone(),
            // A killing blow with no snapshot still pays those standing in the room.
            _ => {
                let area = boss.area;
                let mut characters: Vec<CharacterId> = self
                    .players
                    .keys()
                    .filter_map(|player| self.present_character(*player, area, run))
                    .collect();
                characters.sort();
                characters.dedup();
                characters
            }
        };
        let profile = usize::from(run.profile().max(1));
        let count = usize::from(boss.drops_by_party[profile.min(MAX_PARTY_MEMBERS) - 1]);
        let roll_seed = self.loot_rng.next_u64();
        // The drops are distinct items of the pool, in a deterministic shuffle.
        let mut items = boss.pool.clone();
        for index in (1..items.len()).rev() {
            let swap = usize::try_from(self.loot_rng.next_u64() % (index as u64 + 1)).unwrap_or(0);
            items.swap(index, swap);
        }
        items.truncate(count);
        let deadline = now.saturating_add(u64::from(boss.roll_ticks));
        let drops = items
            .into_iter()
            .map(|item| BossDrop {
                item,
                state: DropState::Rolling {
                    deadline,
                    choices: Vec::new(),
                },
            })
            .collect();
        for add in boss.adds.iter().map(|add| add.creature) {
            self.stand_down(add, 0)?;
        }
        if let Some(run) = self.depths.as_mut() {
            run.boss.attempt = None;
            run.boss.outcome = Some(BossOutcome {
                version: 1,
                confirmed: 0,
                roll_seed,
                eligible,
                drops,
                credited: Vec::new(),
            });
        }
        Ok(())
    }

    /// One answer to a roll window (5.1.34). Refusals change nothing; settlements are final.
    pub(crate) fn answer_roll(
        &mut self,
        player: PlayerId,
        run_id: u64,
        drop: u8,
        choice: RollChoice,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let refuse = |code| Ok(Some((code, None)));
        let Some(character) = self
            .players
            .get(&player)
            .and_then(|state| state.character.clone())
        else {
            return refuse(ErrorCode::NotLootOwner);
        };
        let Some(run) = self.depths.as_ref().filter(|run| run.run_id == run_id) else {
            return refuse(ErrorCode::InvalidLoot);
        };
        let Some(outcome) = run.boss.outcome.as_ref() else {
            return refuse(ErrorCode::InvalidLoot);
        };
        if !outcome.eligible.contains(&character) {
            return refuse(ErrorCode::NotLootOwner);
        }
        let Some(entry) = outcome.drops.get(usize::from(drop)) else {
            return refuse(ErrorCode::InvalidLoot);
        };
        let DropState::Rolling { choices, .. } = &entry.state else {
            return refuse(ErrorCode::InvalidLoot);
        };
        if choices.iter().any(|(answered, _)| *answered == character) {
            return refuse(ErrorCode::InvalidLoot);
        }
        if choice == RollChoice::Need && !equippable(entry.item) {
            return refuse(ErrorCode::NotEquippable);
        }
        if let Some(run) = self.depths.as_mut()
            && let Some(outcome) = run.boss.outcome.as_mut()
            && let Some(BossDrop {
                state: DropState::Rolling { choices, .. },
                ..
            }) = outcome.drops.get_mut(usize::from(drop))
        {
            let index = choices
                .binary_search_by(|(answered, _)| answered.cmp(&character))
                .unwrap_or_else(|index| index);
            choices.insert(index, (character, choice));
            let complete = choices.len() == outcome.eligible.len();
            if complete {
                self.settle_drop(usize::from(drop))?;
            }
        }
        Ok(None)
    }

    /// Takes a held drop (5.1.83): one claimant, then settled like a won roll.
    pub(crate) fn claim_drop(
        &mut self,
        player: PlayerId,
        run_id: u64,
        drop: u8,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let refuse = |code| Ok(Some((code, None)));
        let Some(character) = self
            .players
            .get(&player)
            .and_then(|state| state.character.clone())
        else {
            return refuse(ErrorCode::NotLootOwner);
        };
        let Some(run) = self.depths.as_ref().filter(|run| run.run_id == run_id) else {
            return refuse(ErrorCode::InvalidLoot);
        };
        let Some(outcome) = run.boss.outcome.as_ref() else {
            return refuse(ErrorCode::InvalidLoot);
        };
        if !outcome.eligible.contains(&character) {
            return refuse(ErrorCode::NotLootOwner);
        }
        let Some(entry) = outcome.drops.get(usize::from(drop)) else {
            return refuse(ErrorCode::InvalidLoot);
        };
        if entry.state != DropState::Held {
            return refuse(ErrorCode::InvalidLoot);
        }
        let item = entry.item;
        let mut bag = self
            .players
            .get(&player)
            .ok_or_else(|| ZoneError::new("loot claimant is absent"))?
            .inventory
            .clone();
        if bag.insert(item, 1).is_err() {
            return refuse(ErrorCode::InventoryFull);
        }
        if let Some(run) = self.depths.as_mut()
            && let Some(outcome) = run.boss.outcome.as_mut()
        {
            outcome.version += 1;
            let version = outcome.version;
            if let Some(entry) = outcome.drops.get_mut(usize::from(drop)) {
                entry.state = DropState::Awarded {
                    winner: character,
                    roll: 0,
                    need: false,
                    version,
                    delivered: false,
                    full_notified: false,
                };
            }
        }
        Ok(None)
    }

    fn settle_rolls(&mut self, now: u64) -> Result<(), ZoneError> {
        let Some(run) = self.depths.as_ref() else {
            return Ok(());
        };
        let Some(outcome) = run.boss.outcome.as_ref() else {
            return Ok(());
        };
        let due: Vec<usize> = outcome
            .drops
            .iter()
            .enumerate()
            .filter(|(_, drop)| match &drop.state {
                DropState::Rolling { deadline, choices } => {
                    now >= *deadline || choices.len() == outcome.eligible.len()
                }
                _ => false,
            })
            .map(|(index, _)| index)
            .collect();
        for index in due {
            self.settle_drop(index)?;
        }
        Ok(())
    }

    /// Resolves one window: Need before Greed, the highest deterministic roll wins. A window
    /// with no Need or Greed holds the item for a manual claim (5.1.83).
    fn settle_drop(&mut self, index: usize) -> Result<(), ZoneError> {
        let Some(run) = self.depths.as_mut() else {
            return Ok(());
        };
        let run_id = run.run_id;
        let Some(outcome) = run.boss.outcome.as_mut() else {
            return Ok(());
        };
        let seed = outcome.roll_seed;
        let Some(drop) = outcome.drops.get_mut(index) else {
            return Ok(());
        };
        let DropState::Rolling { choices, .. } = &drop.state else {
            return Ok(());
        };
        let drop_index = u8::try_from(index).unwrap_or(u8::MAX);
        drop.state = match resolve_roll(seed, run_id, drop_index, choices) {
            Some(win) => {
                outcome.version += 1;
                DropState::Awarded {
                    winner: win.winner,
                    roll: win.roll,
                    need: win.need,
                    version: outcome.version,
                    delivered: false,
                    full_notified: false,
                }
            }
            None => {
                outcome.version += 1;
                DropState::Held
            }
        };
        Ok(())
    }

    /// Records that the run directory holds the outcome up to `version`, unblocking the
    /// rewards it decided. Stale, repeated or foreign confirmations change nothing.
    pub fn confirm_boss_outcome(&mut self, run_id: u64, version: u32) {
        if let Some(run) = self.depths.as_mut()
            && run.run_id == run_id
            && let Some(outcome) = run.boss.outcome.as_mut()
        {
            outcome.confirmed = outcome.confirmed.max(version.min(outcome.version));
        }
    }

    /// The report a host records in the run directory, once there is a boss outcome.
    #[must_use]
    pub fn boss_report(&self) -> Option<BossReport> {
        let run = self.depths.as_ref()?;
        let outcome = run.boss.outcome.as_ref()?;
        let mut outstanding: BTreeSet<CharacterId> = BTreeSet::new();
        for drop in &outcome.drops {
            match &drop.state {
                DropState::Rolling { choices, .. } => {
                    outstanding.extend(
                        outcome
                            .eligible
                            .iter()
                            .filter(|character| {
                                !choices.iter().any(|(answered, _)| answered == *character)
                            })
                            .cloned(),
                    );
                }
                DropState::Awarded {
                    winner,
                    delivered: false,
                    ..
                } => {
                    outstanding.insert(winner.clone());
                }
                _ => {}
            }
        }
        Some(BossReport {
            run_id: run.run_id,
            outcome: outcome.clone(),
            outstanding: outstanding.into_iter().collect(),
        })
    }

    /// Gives every committed reward to the characters present: items that fit, boss credit.
    fn deliver_boss_rewards(&mut self) -> Result<(), ZoneError> {
        let Some(run) = self.depths.as_ref() else {
            return Ok(());
        };
        let run_id = run.run_id;
        let Some(outcome) = run.boss.outcome.as_ref() else {
            return Ok(());
        };
        if outcome.confirmed == 0 {
            return Ok(());
        }
        let confirmed = outcome.confirmed;
        let pending: Vec<(usize, ItemId, CharacterId)> = outcome
            .drops
            .iter()
            .enumerate()
            .filter_map(|(index, drop)| match &drop.state {
                DropState::Awarded {
                    winner,
                    version,
                    delivered: false,
                    ..
                } if *version <= confirmed => Some((index, drop.item, winner.clone())),
                _ => None,
            })
            .collect();
        let eligible = outcome.eligible.clone();
        let uncredited: Vec<CharacterId> = eligible
            .into_iter()
            .filter(|character| !outcome.credited.contains(character))
            .collect();
        for (index, item, winner) in pending {
            self.deliver_drop(run_id, index, item, winner)?;
        }
        for character in uncredited {
            self.give_boss_credit(run_id, character)?;
        }
        Ok(())
    }

    fn deliver_drop(
        &mut self,
        run_id: u64,
        index: usize,
        item: ItemId,
        winner: CharacterId,
    ) -> Result<(), ZoneError> {
        let Some(player_id) = self.player_of(&winner) else {
            return Ok(());
        };
        let bit = 1_u8 << index;
        let Some(player) = self.players.get_mut(&player_id) else {
            return Ok(());
        };
        let received = player.depths.has_drop(run_id, bit);
        if !received {
            let mut bag = player.inventory.clone();
            if bag.insert(item, 1).is_err() {
                let first_notice = self
                    .depths
                    .as_mut()
                    .and_then(|run| run.boss.outcome.as_mut())
                    .and_then(|outcome| outcome.drops.get_mut(index))
                    .is_some_and(|drop| match &mut drop.state {
                        DropState::Awarded { full_notified, .. } => {
                            !std::mem::replace(full_notified, true)
                        }
                        _ => false,
                    });
                if first_notice {
                    self.notify(
                        player_id,
                        ZoneEvent::Error {
                            code: ErrorCode::InventoryFull,
                            target: None,
                        },
                    );
                }
                return Ok(());
            }
            player.inventory = bag;
            player.inventory_revision = player.inventory_revision.wrapping_add(1);
            player.depths.record_drop(run_id, bit);
        }
        if let Some(DropState::Awarded { delivered, .. }) = self
            .depths
            .as_mut()
            .and_then(|run| run.boss.outcome.as_mut())
            .and_then(|outcome| outcome.drops.get_mut(index))
            .map(|drop| &mut drop.state)
        {
            *delivered = true;
        }
        Ok(())
    }

    /// Boss experience and the durable victory evidence (5.1.37): once per character and run.
    fn give_boss_credit(&mut self, run_id: u64, character: CharacterId) -> Result<(), ZoneError> {
        let Some(player_id) = self.player_of(&character) else {
            return Ok(());
        };
        let Some(player) = self.players.get_mut(&player_id) else {
            return Ok(());
        };
        let already = player.depths.credited(run_id);
        if !already {
            let first_victory = !player.depths.thorn_defeated;
            player.depths.record_credit(run_id);
            if first_victory {
                self.report_thorn_defeated(player_id);
            }
            let (_, template) = Self::creature_content(
                &self.content,
                self.depths_boss()
                    .ok_or_else(|| ZoneError::new("the depths boss is gone"))?
                    .creature,
            )?;
            let level = template.max_level;
            self.reward_kill_experience(player_id, level)?;
        }
        if let Some(run) = self.depths.as_mut()
            && let Some(outcome) = run.boss.outcome.as_mut()
        {
            let index = outcome
                .credited
                .binary_search(&character)
                .unwrap_or_else(|index| index);
            if outcome.credited.get(index) != Some(&character) {
                outcome.credited.insert(index, character);
            }
        }
        Ok(())
    }

    /// The viewer's copy of the shared pool; `None` unless eligible for this kill.
    #[must_use]
    pub(crate) fn depths_loot_view(&self, viewer: PlayerId) -> Option<DepthsLootView> {
        let run = self.depths.as_ref()?;
        let outcome = run.boss.outcome.as_ref()?;
        let character = self.players.get(&viewer)?.character.as_ref()?;
        if !outcome.eligible.contains(character) {
            return None;
        }
        let drops = outcome
            .drops
            .iter()
            .enumerate()
            .map(|(index, drop)| DepthsDropView {
                drop: u8::try_from(index).unwrap_or(u8::MAX),
                item: drop.item,
                state: match &drop.state {
                    DropState::Rolling { deadline, choices } => DropView::Rolling {
                        deadline: *deadline,
                        answer: choices
                            .iter()
                            .find(|(answered, _)| answered == character)
                            .map(|(_, choice)| *choice),
                        may_need: equippable(drop.item),
                    },
                    DropState::Held => DropView::Held { claimable: true },
                    DropState::Awarded {
                        winner,
                        roll,
                        need,
                        delivered,
                        ..
                    } => DropView::Awarded {
                        won: winner == character,
                        roll: *roll,
                        need: *need,
                        pending: !delivered,
                    },
                },
            })
            .collect();
        Some(DepthsLootView {
            run_id: run.run_id,
            drops,
        })
    }
}

/// Whether a character may roll Need on `item`: the equipment rules (5.1.33). The catalog has
/// no class or level restriction beyond having an equipment slot.
#[must_use]
pub fn equippable(item: ItemId) -> bool {
    item_template(item).is_some_and(|item| item.slot.is_some())
}

/// The result of one settled window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RollWin {
    pub winner: CharacterId,
    pub roll: u8,
    pub need: bool,
}

/// Deterministic Need/Greed resolution over final choices. `None` when nobody took a chance.
#[must_use]
pub fn resolve_roll(
    seed: u64,
    run_id: u64,
    drop: u8,
    choices: &[(CharacterId, RollChoice)],
) -> Option<RollWin> {
    let tier: Vec<_> = choices
        .iter()
        .filter(|(_, choice)| *choice == RollChoice::Need)
        .collect();
    let need = !tier.is_empty();
    let tier = if need {
        tier
    } else {
        choices
            .iter()
            .filter(|(_, choice)| *choice == RollChoice::Greed)
            .collect()
    };
    let scored: Vec<_> = tier
        .iter()
        .map(|(character, _)| {
            let (roll, tie) = roll_for(seed, run_id, drop, character);
            (character, roll, tie)
        })
        .collect();
    let (winner, roll, _) = pick_winner(&scored)?;
    Some(RollWin {
        winner: (**winner).clone(),
        roll: *roll,
        need,
    })
}

/// The highest roll wins; ties fall to a deterministic tie value, then to character order.
#[must_use]
pub fn pick_winner<'a>(
    scored: &'a [(&'a CharacterId, u8, u32)],
) -> Option<&'a (&'a CharacterId, u8, u32)> {
    scored.iter().max_by(|left, right| {
        (left.1, left.2)
            .cmp(&(right.1, right.2))
            .then_with(|| right.0.cmp(left.0))
    })
}

/// A character's stable `(roll 1..=100, tie value)` for one drop: independent of tick order, so
/// it cannot change across a retried or replayed settlement.
#[must_use]
pub fn roll_for(seed: u64, run_id: u64, drop: u8, character: &CharacterId) -> (u8, u32) {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    for byte in run_id
        .to_be_bytes()
        .into_iter()
        .chain([drop])
        .chain(character.as_str().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let roll = u8::try_from(1 + (hash >> 40) % 100).unwrap_or(1);
    (roll, (hash & 0xffff_ffff) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> CharacterId {
        CharacterId::new(name).unwrap()
    }

    #[test]
    fn need_outranks_greed_whatever_the_rolls() {
        let choices = vec![
            (id("aria"), RollChoice::Greed),
            (id("bren"), RollChoice::Need),
            (id("cole"), RollChoice::Pass),
        ];
        for seed in 0..64 {
            let win = resolve_roll(seed, 7, 0, &choices).unwrap();
            assert_eq!(win.winner, id("bren"));
            assert!(win.need);
        }
    }

    #[test]
    fn only_passes_take_nothing() {
        let choices = vec![(id("aria"), RollChoice::Pass)];
        assert_eq!(resolve_roll(1, 1, 0, &choices), None);
        assert_eq!(resolve_roll(1, 1, 0, &[]), None);
    }

    #[test]
    fn a_tie_is_broken_deterministically_never_rerolled() {
        let (a, b) = (id("aria"), id("bren"));
        let scored = [(&a, 50, 9), (&b, 50, 9)];
        // Equal rolls and tie values fall to character order, on either input order.
        assert_eq!(pick_winner(&scored).unwrap().0, &a);
        let swapped = [(&b, 50, 9), (&a, 50, 9)];
        assert_eq!(pick_winner(&swapped).unwrap().0, &a);
        // The tie value decides before character order does.
        let scored = [(&a, 50, 1), (&b, 50, 2)];
        assert_eq!(pick_winner(&scored).unwrap().0, &b);
        // Every roll stays in range and repeats exactly.
        for character in [&a, &b] {
            let (roll, _) = roll_for(3, 1, 0, character);
            assert!((1..=100).contains(&roll));
            assert_eq!(roll_for(3, 1, 0, character), roll_for(3, 1, 0, character));
        }
    }
}
