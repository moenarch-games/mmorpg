//! The party-private Redbrand Hollow depths (#404 part 2, ADR 0009).
//!
//! Each entering solo player or party (up to five) plays its own zone, a [`DepthsRun`]: a
//! private simulation of one authored dungeon whose encounter composition depends only on the
//! entering party size, frozen when the run is created. Who may start or resume a run, the
//! frozen roster and how long an emptied run stays resumable are the run directory's in the
//! control plane; a depths zone only holds the [`DepthsRun`] its host binds to it with a trusted
//! input, so replay and recovery reproduce it.
//!
//! - **Threshold.** A public zone with a [`DepthsThreshold`] reports the living players
//!   standing in its strip as [`ThresholdRequest`]s; the host resolves each against the run
//!   directory and either starts a fenced transfer or refuses with a notice.
//! - **Composition** ([`DepthsSpawn::party`], [`DepthsSpawn::after_fallen`]): which creatures
//!   stand, and which arrive as reinforcements once enough of their chamber has fallen, depends
//!   on the frozen party size. Creature numbers never scale with hit points or damage.
//! - **Chambers** clear in any order; clearing the first unlocks the checkpoint
//!   ([`DepthsSite::checkpoint`]) where released and wiped players stand again.
//! - **Rewards.** A chamber's creatures never respawn within a run, so kill rewards are paid at
//!   most once per run creature, across wipes, reconnects and recovery.
//! - **Wipe.** When every player in the zone is dead for [`WIPE_DELAY_TICKS`], the uncleared
//!   chambers' survivors stand down and go home at full health, and the players stand up at the
//!   checkpoint (or the entrance). Fallen creatures stay fallen.
//! - **The sealed boss chamber** stands empty and walled off for #404 part 3.

use std::sync::Arc;

use crate::creature::Life;
use crate::entity::body_id;
use crate::physics::Vec3i;
use crate::snapshot::{CreatureAi, QueuedIntent};
use crate::{
    CharacterId, CharacterLocation, CreatureId, EntityRef, ErrorCode, MAX_PARTY_MEMBERS, PlayerId,
    QuestId, XzBounds, ZoneError, ZoneId, ZoneSimulation,
};

/// A wipe resolves after every present player has been dead this long (5 s).
pub const WIPE_DELAY_TICKS: u64 = crate::long_ticks_from_ms(5_000) as u64;

/// A public zone's way into the depths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DepthsThreshold {
    /// Standing here (and alive, out of combat) asks to enter.
    pub strip: XzBounds,
    /// Party members standing here enter with the player who touches the strip.
    pub gather: XzBounds,
    /// Completing this quest authorises a new run.
    pub qualifying_quest: QuestId,
    /// Where a character returning from the depths stands, feet on the ground, in this zone's
    /// coordinates: the safe public entrance. Facing is `facing`.
    pub entrance: [i32; 3],
    pub facing: u16,
}

/// One creature of a run, with its place in the party-size table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DepthsSpawn {
    pub creature: CreatureId,
    /// The entering party sizes this creature stands for, inclusive.
    pub party: [u8; 2],
    /// When set, the creature only arrives after this many of its chamber's other creatures
    /// have fallen: the reinforcement mechanic of larger parties.
    pub after_fallen: Option<u8>,
}

/// One encounter chamber: its creatures in spawn-table order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthsChamber {
    pub name: String,
    pub spawns: Vec<DepthsSpawn>,
}

/// A depths zone's authored encounter table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthsSite {
    pub chambers: Vec<DepthsChamber>,
    /// Where released and wiped players stand before the checkpoint, feet XZ.
    pub entrance: [i32; 2],
    /// Where they stand once the first chamber is cleared.
    pub checkpoint: [i32; 2],
}

impl DepthsSpawn {
    /// Whether this creature stands for an entering party of `profile`.
    #[must_use]
    pub const fn stands_for(&self, profile: u8) -> bool {
        self.party[0] <= profile && profile <= self.party[1]
    }
}

impl DepthsSite {
    pub(crate) fn is_site_creature(&self, creature: CreatureId) -> bool {
        self.chambers
            .iter()
            .flat_map(|chamber| &chamber.spawns)
            .any(|spawn| spawn.creature == creature)
    }

    pub(crate) fn validate(&self, content: &crate::ZoneContent) -> Result<(), ZoneError> {
        if self.chambers.is_empty() || self.chambers.len() > usize::from(MAX_CHAMBERS) {
            return Err(ZoneError::new("a depths site needs one to eight chambers"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for chamber in &self.chambers {
            for spawn in &chamber.spawns {
                if spawn.party[0] < 1
                    || usize::from(spawn.party[1]) > MAX_PARTY_MEMBERS
                    || spawn.party[0] > spawn.party[1]
                {
                    return Err(ZoneError::new("a depths spawn needs party sizes 1..=5"));
                }
                if content.creature_spawn(spawn.creature).is_none() || !seen.insert(spawn.creature)
                {
                    return Err(ZoneError::new(
                        "a depths spawn must name a distinct creature of the zone",
                    ));
                }
            }
        }
        for profile in 1..=MAX_PARTY_MEMBERS as u8 {
            if self
                .chambers
                .iter()
                .all(|chamber| !chamber.spawns.iter().any(|spawn| spawn.stands_for(profile)))
            {
                return Err(ZoneError::new("every party size needs enemies"));
            }
        }
        Ok(())
    }
}

/// The most chambers a site may author (the `cleared` mask is one byte).
pub const MAX_CHAMBERS: u8 = 8;

/// The state a host binds a depths zone to: the frozen parts of a [`DepthsRun`], and the
/// progress the run directory already recorded for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthsRunStart {
    pub run_id: u64,
    /// The frozen roster, in entry order; `profile` is its length.
    pub roster: Vec<CharacterId>,
    pub cleared: u8,
    pub checkpoint: bool,
}

/// A private run bound to a depths zone. Canonical: replay and recovery see it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthsRun {
    pub run_id: u64,
    pub roster: Vec<CharacterId>,
    /// Bit `i` is set once chamber `i` is cleared.
    pub cleared: u8,
    /// Whether the first chamber's checkpoint is unlocked.
    pub checkpoint: bool,
    /// Tick at which an all-dead zone's wipe resolves.
    pub wipe_at: Option<u64>,
}

impl DepthsRun {
    /// The entering party size that fixed this run's composition; roster size never changes.
    #[must_use]
    pub fn profile(&self) -> u8 {
        u8::try_from(self.roster.len()).unwrap_or(u8::MAX)
    }

    /// Whether `character` is part of the frozen roster.
    #[must_use]
    pub fn admits(&self, character: &CharacterId) -> bool {
        self.roster.contains(character)
    }

    pub(crate) fn validate(&self, site: &DepthsSite) -> Result<(), ZoneError> {
        if self.roster.is_empty()
            || self.roster.len() > MAX_PARTY_MEMBERS
            || self.run_id == 0
            || (site.chambers.len() < 8 && self.cleared >> site.chambers.len() != 0)
            || self
                .roster
                .iter()
                .enumerate()
                .any(|(index, character)| self.roster[..index].contains(character))
        {
            return Err(ZoneError::new("invalid depths run"));
        }
        Ok(())
    }
}

/// A player standing in a threshold strip, asking the host to enter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdRequest {
    pub player: PlayerId,
    /// The player and its gathered party members, in ID order, each with whether it has
    /// completed the qualifying quest.
    pub candidates: Vec<(PlayerId, bool)>,
}

/// A character's arrival in a depths zone: the zone-side coordinates the host offers a transfer
/// toward (body centre on the ground, facing into the chamber).
#[must_use]
pub const fn depths_arrival(zone: ZoneId) -> CharacterLocation {
    CharacterLocation {
        zone_id: zone,
        position: crate::redbrand_depths::ARRIVAL,
        facing: crate::redbrand_depths::ARRIVAL_FACING,
    }
}

impl ZoneSimulation {
    /// Whether this zone has an authored threshold into the depths.
    #[must_use]
    pub fn has_depths_threshold(&self) -> bool {
        self.content.depths_threshold().is_some()
    }

    /// Declares that this host resolves threshold requests (hosts with a run directory and the
    /// fenced transfer, and the browser's local host). Host-phase configuration, not canonical.
    pub fn enable_depths_entry(&mut self) {
        self.transfers.depths_entry_enabled = true;
    }

    /// The living players in a threshold strip, in player-ID order: each with its gathered
    /// party. Empty unless [`Self::enable_depths_entry`] was called.
    #[must_use]
    pub fn threshold_requests(&self) -> Vec<ThresholdRequest> {
        let Some(threshold) = self.content.depths_threshold() else {
            return Vec::new();
        };
        if !self.transfers.depths_entry_enabled {
            return Vec::new();
        }
        let qualifies = |player: PlayerId| {
            self.players
                .get(&player)
                .is_some_and(|state| state.quests.is_completed(threshold.qualifying_quest))
        };
        self.players
            .iter()
            .filter(|(player, state)| {
                state.is_alive()
                    && state.combat_timer == 0
                    && !self.transfers.waystone_departures.contains_key(*player)
            })
            .filter_map(|(&player, _)| {
                let at = self.player_position(player).ok()?;
                threshold.strip.contains(at.x, at.z).then_some(player)
            })
            .map(|player| {
                let mut candidates = vec![(player, qualifies(player))];
                for mate in self.party_mates(player) {
                    let gathered = self
                        .players
                        .get(&mate)
                        .is_some_and(|state| state.is_alive())
                        && self
                            .player_position(mate)
                            .is_ok_and(|at| threshold.gather.contains(at.x, at.z));
                    if gathered {
                        candidates.push((mate, qualifies(mate)));
                    }
                }
                ThresholdRequest { player, candidates }
            })
            .collect()
    }

    /// Queues a host-chosen departure of `player` toward `destination`, as a waystone in
    /// another zone does: the host's departure service freezes it on its next pass.
    pub fn queue_departure(
        &mut self,
        player: PlayerId,
        destination: CharacterLocation,
    ) -> Result<(), ZoneError> {
        if !self.players.contains_key(&player) {
            return Err(ZoneError::new("unknown player"));
        }
        self.transfers
            .waystone_departures
            .insert(player, destination);
        Ok(())
    }

    /// Queues a notice ([`ErrorCode`] chat line) for `player`; it reaches the player's events
    /// in the next tick, in order with its commands.
    pub fn queue_notice(&mut self, player: PlayerId, code: ErrorCode) -> Result<(), ZoneError> {
        let state = self
            .players
            .get_mut(&player)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        if state.intents.len() < crate::MAX_PENDING_INTENTS {
            state
                .intents
                .push(QueuedIntent::from(crate::PlayerIntent::Notice(code)));
        }
        Ok(())
    }

    /// The run bound to this zone, if any.
    #[must_use]
    pub const fn depths_run(&self) -> Option<&DepthsRun> {
        self.depths.as_ref()
    }

    /// Binds a new private run to this depths zone: any earlier run's residue is cleared and
    /// the uncleared chambers' creatures for this party size stand at their posts. Binding the
    /// run already held is a no-op. A zone without a depths site refuses.
    pub fn begin_depths_run(&mut self, start: DepthsRunStart) -> Result<bool, ZoneError> {
        let content = Arc::clone(&self.content);
        let Some(site) = content.depths_site() else {
            return Err(ZoneError::new("this zone is not a depths zone"));
        };
        if self
            .depths
            .as_ref()
            .is_some_and(|run| run.run_id == start.run_id)
        {
            return Ok(false);
        }
        let run = DepthsRun {
            run_id: start.run_id,
            roster: start.roster,
            cleared: start.cleared,
            checkpoint: start.checkpoint,
            wipe_at: None,
        };
        run.validate(site)?;
        for spawn in site.chambers.iter().flat_map(|chamber| &chamber.spawns) {
            self.stand_down(spawn.creature, 0)?;
        }
        self.projectiles.clear();
        self.debris.clear();
        self.forced.clear();
        for (index, chamber) in site.chambers.iter().enumerate() {
            if run.cleared & (1 << index) != 0 {
                continue;
            }
            for spawn in &chamber.spawns {
                if spawn.stands_for(run.profile()) && spawn.after_fallen.is_none() {
                    self.spawn_creature(spawn.creature)?;
                }
            }
        }
        self.depths = Some(run);
        Ok(true)
    }

    /// Takes a creature out of the run: no body, no life, never respawning.
    fn stand_down(&mut self, id: CreatureId, died_at: u64) -> Result<(), ZoneError> {
        let entity = EntityRef::Creature(id);
        self.world.remove_body(body_id(entity));
        if self.interest.remove(entity) {
            self.interest_work.bucket_removes += 1;
        }
        for player in self.players.values_mut() {
            if player.target == Some(entity) {
                player.target = None;
            }
        }
        if !self.creatures.contains_key(&id) {
            self.reserve_raider(id)?;
        }
        if let Some(state) = self.creatures.get_mut(&id) {
            state.life = Life::Despawned { died_at };
            state.health = 0;
            state.ai = CreatureAi::Idle {
                timer: 0,
                destination: None,
            };
            state.threat.clear();
            state.swing_timer = 0;
            state.combat_timer = 0;
            state.tapped_by = None;
            state.loot = None;
            state.loot_party.clear();
            state.cast = None;
            state.auras.clear();
            state.encounter = None;
            state.reset_abilities();
        }
        Ok(())
    }

    /// Whether `id` has fallen in this run (a corpse, or despawned after dying).
    fn has_fallen(&self, id: CreatureId) -> bool {
        self.creatures
            .get(&id)
            .is_some_and(|creature| match creature.life {
                Life::Corpse { .. } => true,
                Life::Despawned { died_at } => died_at > 0,
                Life::Alive => false,
            })
    }

    /// Tick step: reinforcements, cleared chambers and wipes.
    pub(crate) fn advance_depths(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (Some(site), Some(run)) = (content.depths_site(), self.depths.clone()) else {
            return Ok(());
        };
        let profile = run.profile();
        let mut cleared = run.cleared;
        for (index, chamber) in site.chambers.iter().enumerate() {
            if cleared & (1 << index) != 0 {
                continue;
            }
            let standing: Vec<_> = chamber
                .spawns
                .iter()
                .filter(|spawn| spawn.stands_for(profile))
                .collect();
            let fallen = standing
                .iter()
                .filter(|spawn| self.has_fallen(spawn.creature))
                .count();
            for spawn in &standing {
                let pending = self.creatures.get(&spawn.creature).is_some_and(|creature| {
                    matches!(creature.life, Life::Despawned { died_at: 0 })
                });
                if pending
                    && spawn
                        .after_fallen
                        .is_some_and(|needed| fallen >= usize::from(needed))
                {
                    self.spawn_creature(spawn.creature)?;
                }
            }
            if !standing.is_empty() && fallen == standing.len() {
                cleared |= 1 << index;
            }
        }
        let wiped = !self.players.is_empty() && self.players.values().all(|p| !p.is_alive());
        let wipe_at = match (wiped, run.wipe_at) {
            (true, Some(at)) => Some(at),
            (true, None) => Some(now.saturating_add(WIPE_DELAY_TICKS)),
            (false, _) => None,
        };
        let due = wiped && wipe_at.is_some_and(|at| now >= at);
        if let Some(state) = self.depths.as_mut() {
            state.cleared = cleared;
            if !state.checkpoint && cleared & 1 != 0 {
                state.checkpoint = true;
            }
            state.wipe_at = if due { None } else { wipe_at };
        }
        if due {
            self.resolve_wipe(site, cleared)?;
        }
        Ok(())
    }

    /// Survivors of uncleared chambers go home at full health; every player stands up again.
    fn resolve_wipe(&mut self, site: &DepthsSite, cleared: u8) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for (index, chamber) in site.chambers.iter().enumerate() {
            if cleared & (1 << index) != 0 {
                continue;
            }
            for spawn in &chamber.spawns {
                let alive = self
                    .creatures
                    .get(&spawn.creature)
                    .is_some_and(crate::creature::CreatureState::is_alive);
                if !alive {
                    continue;
                }
                let (spawn_data, template) = Self::creature_content(&content, spawn.creature)?;
                let id = spawn.creature;
                let entity = EntityRef::Creature(id);
                let position = Vec3i::new(
                    spawn_data.position[0],
                    self.stand_y(
                        spawn_data.position[0],
                        spawn_data.position[1],
                        template.half_extents,
                    ),
                    spawn_data.position[1],
                );
                let body = body_id(entity);
                self.world
                    .set_position(body, position)
                    .map_err(crate::zone::physics_error)?;
                self.world
                    .set_velocity(body, Vec3i::ZERO)
                    .map_err(crate::zone::physics_error)?;
                self.place_in_interest(entity, position.x, position.z);
                if let Some(creature) = self.creatures.get_mut(&id) {
                    creature.health = template.max_health(creature.level);
                    creature.threat.clear();
                    creature.swing_timer = 0;
                    creature.combat_timer = 0;
                    creature.tapped_by = None;
                    creature.cast = None;
                    creature.auras.clear();
                    creature.reset_abilities();
                    creature.ai = CreatureAi::Idle {
                        timer: crate::WANDER_WAIT_TICKS[0],
                        destination: None,
                    };
                }
            }
        }
        let fallen: Vec<_> = self
            .players
            .iter()
            .filter(|(_, player)| !player.is_alive())
            .map(|(&player, _)| player)
            .collect();
        for player in fallen {
            self.release_spirit(player)?;
            self.notify(
                player,
                crate::ZoneEvent::Error {
                    code: ErrorCode::DepthsWipe,
                    target: None,
                },
            );
        }
        Ok(())
    }

    /// A character arriving here from a Redbrand Depths instance whose run no longer holds it
    /// stands at the threshold's safe public entrance, not at a spawn slot: `(body centre,
    /// facing)`. `None` for every other record.
    pub(crate) fn depths_entrance_for(
        &self,
        record: &crate::CharacterRecord,
    ) -> Option<([i32; 3], u16)> {
        if !crate::redbrand_depths::is_depths_zone(record.location.zone_id) {
            return None;
        }
        let threshold = self.content.depths_threshold()?;
        Some((threshold.entrance, threshold.facing))
    }

    /// Where a released or wiped player stands in a depths zone; `None` elsewhere.
    pub(crate) fn depths_release_point(&self) -> Option<[i32; 2]> {
        let site = self.content.depths_site()?;
        let run = self.depths.as_ref()?;
        Some(if run.checkpoint {
            site.checkpoint
        } else {
            site.entrance
        })
    }

    /// Whether `character` may be admitted to this zone's run: always when no run is bound.
    #[must_use]
    pub fn depths_admits(&self, character: &CharacterId) -> bool {
        self.depths.as_ref().is_none_or(|run| run.admits(character))
    }
}
