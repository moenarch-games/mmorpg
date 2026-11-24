//! Quest actions at NPCs (#1019): using an item on an NPC, being heard by one and escorting
//! one, all through [`crate::ZoneCommand::QuestAction`], checked in the tick.
//!
//! - **Action**: a living player with the weapon sheathed, outside a handoff corridor and within
//!   quest reach of a visible NPC performs the first open action objective at that NPC
//!   ([`crate::QuestObjective::action_npc`]) of its active quests in log order whose earlier
//!   objectives are all done, so a quest's steps come in their authored order. Nothing open is
//!   an invalid quest; refusals change nothing.
//! - **Use item**: takes one of the item from the bag and completes the objective. Showing a
//!   [`Confrontation`]'s evidence to its NPC turns the NPC hostile: its reserved foe stands beside
//!   it and attacks the player, and the NPC is gone while the foe lives or lies dead. A foe that
//!   gives up the fight leaves the world, and the NPC is back.
//! - **Interview**: an NPC with an [`NpcAttitude`] hears only a player it trusts: one whose class
//!   it does not distrust, one who turned in its recommendation quest, or one whose party mate it
//!   trusts stands within reach of it to speak for them (the sponsor). Otherwise it refuses with
//!   [`crate::ErrorCode::Distrusted`]. Sponsoring completes nothing for the sponsor.
//! - **Escort**: starts an [`EscortState`]: the NPC follows the player's trail at a wounded
//!   walk, keeping [`ESCORT_GAP_UNITS`] behind. Inside the objective's area it has arrived: the
//!   objective is done for the escorting player and for each party mate with the same objective
//!   open within quest reach, and the NPC rests there for [`ESCORT_REST_TICKS`] before it is back
//!   at its post. A player more than [`ESCORT_LEASH_UNITS`] ahead for [`ESCORT_LEASH_TICKS`], a
//!   dead, absent or quitting player fails it ([`crate::ErrorCode::EscortLost`]): the NPC is back
//!   at its post at once and the player asks again. One player escorts an NPC at a time.
//! - **Delivery** (#1077): takes the load of the objective's source out of the player's arms
//!   ([`crate::LoadState`]); empty arms are refused with [`crate::ErrorCode::NotCarrying`].
//! - **Defence**: starts the raid site's defence ([`crate::raid`]) at once when the site is ready;
//!   otherwise [`crate::ErrorCode::SiteNotReady`]. It completes nothing itself.
//!
//! The escorts are canonical zone state, so snapshots, replay and recovery continue them; the
//! log stores the completed objectives like a talk objective's.

use std::sync::Arc;

use crate::creature::Life;
use crate::events::push_event;
use crate::{
    CreatureId, EntityRef, ErrorCode, ItemId, NpcId, PlayerClass, PlayerId, QuestId,
    QuestObjective, ZoneError, ZoneEvent, ZoneSimulation,
};

/// An escorted NPC stops this close behind the player (2 m).
pub const ESCORT_GAP_UNITS: i64 = 200;
/// An escort fails once the player is this far from the NPC (30 m) ...
pub const ESCORT_LEASH_UNITS: i64 = 3_000;
/// ... for this long (10 s).
pub const ESCORT_LEASH_TICKS: u16 = crate::ticks_from_ms(10_000);
/// A wounded NPC walks this many units per tick (1.8 m/s).
pub const ESCORT_UNITS_PER_TICK: i64 = crate::units_per_tick(180) as i64;
/// The player's trail keeps a point at least this far from the last (1 m) ...
pub const ESCORT_TRAIL_SPACING_UNITS: i64 = 100;
/// ... and at most this many points; the oldest goes first.
pub const MAX_ESCORT_TRAIL: usize = 64;
/// An escorted NPC rests where it arrived this long (1 min) before it is back at its post.
pub const ESCORT_REST_TICKS: u32 = crate::long_ticks_from_ms(60_000);

/// How an NPC regards those who would interview it (#1019).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NpcAttitude {
    pub npc: NpcId,
    /// The classes whose visible practice it distrusts.
    pub distrusts: Vec<PlayerClass>,
    /// A trusted party mate within reach may speak for a player it distrusts.
    pub sponsor: bool,
    /// Having turned in this quest is a recommendation it accepts.
    pub recommendation: Option<QuestId>,
}

/// Why an NPC hears a player, or that it does not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Trusted,
    Recommended,
    Sponsored(PlayerId),
    Refused,
}

/// An NPC who turns hostile when shown `evidence` (#1019): its reserved `foe` stands and fights.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Confrontation {
    pub npc: NpcId,
    pub evidence: ItemId,
    pub foe: CreatureId,
}

/// One running escort (#1019), canonical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscortState {
    pub npc: NpcId,
    /// The escorting player.
    pub player: PlayerId,
    /// The NPC's feet XZ, in units.
    pub position: [i32; 2],
    pub facing: u16,
    /// Units per tick of its last step.
    pub velocity: [i8; 2],
    /// The player's trail ahead of the NPC, oldest first, at most [`MAX_ESCORT_TRAIL`] points.
    pub trail: Vec<[i32; 2]>,
    /// Ticks the player has been beyond the leash.
    pub behind: u16,
    /// Zero while it follows; once it arrived, the tick it is back at its post.
    pub rests_until: u64,
}

type Refusal = Option<(ErrorCode, Option<EntityRef>)>;

fn distance(a: [i32; 2], b: [i32; 2]) -> i64 {
    i64::try_from(crate::trig::xz_length(a[0] - b[0], a[1] - b[1])).unwrap_or(i64::MAX)
}

impl ZoneSimulation {
    /// Whether `npc` is out of the world: a [`Confrontation`] NPC whose foe stands or lies dead.
    #[must_use]
    pub fn npc_hidden(&self, npc: NpcId) -> bool {
        self.content
            .confrontation(npc)
            .is_some_and(|confrontation| {
                self.creatures
                    .get(&confrontation.foe)
                    .is_some_and(|foe| !matches!(foe.life, Life::Despawned { .. }))
            })
    }

    /// The running escort of `npc`, if any.
    #[must_use]
    pub fn escort(&self, npc: NpcId) -> Option<&EscortState> {
        self.escorts.get(&npc)
    }

    /// Whether `npc` hears `player_id` (#1019): see the module documentation.
    pub(crate) fn admission(
        &self,
        player_id: PlayerId,
        npc: NpcId,
    ) -> Result<Admission, ZoneError> {
        let Some(attitude) = self.content.attitude(npc) else {
            return Ok(Admission::Trusted);
        };
        let distrusted = |player: PlayerId| {
            self.players
                .get(&player)
                .and_then(|state| state.class)
                .is_some_and(|choice| attitude.distrusts.contains(&choice.class))
        };
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("interviewing player is absent"))?;
        if !distrusted(player_id) {
            return Ok(Admission::Trusted);
        }
        if attitude
            .recommendation
            .is_some_and(|quest| player.quests.is_completed(quest))
        {
            return Ok(Admission::Recommended);
        }
        if attitude.sponsor {
            for mate in self.party_mates(player_id) {
                if mate != player_id
                    && self
                        .players
                        .get(&mate)
                        .is_some_and(|state| state.is_alive())
                    && !distrusted(mate)
                    && self.within_quest_reach(mate, npc)?
                {
                    return Ok(Admission::Sponsored(mate));
                }
            }
        }
        Ok(Admission::Refused)
    }

    /// The `QuestAction` intent (#1019) at `npc`: see the module documentation.
    pub(crate) fn quest_action(
        &mut self,
        player_id: PlayerId,
        npc: NpcId,
    ) -> Result<Refusal, ZoneError> {
        let target = Some(EntityRef::Npc(npc));
        let content = Arc::clone(&self.content);
        if content.npc(npc).is_none() || self.npc_hidden(npc) {
            return Ok(Some((ErrorCode::InvalidTarget, target)));
        }
        if !self.within_quest_reach(player_id, npc)? {
            return Ok(Some((ErrorCode::OutOfRange, target)));
        }
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("acting player is absent"))?;
        let mut open = None;
        'quests: for entry in &player.quests.entries {
            let Some(quest) = content.quest(entry.quest) else {
                continue;
            };
            let current = crate::quest::progress(quest, entry, &player.inventory, *player.depths);
            for (index, objective) in quest.objectives.iter().enumerate() {
                if objective.action_npc() != Some(npc) || current[index] >= objective.required() {
                    continue;
                }
                let ready = quest.objectives[..index]
                    .iter()
                    .zip(current)
                    .all(|(earlier, count)| count >= earlier.required());
                if ready {
                    open = Some((quest.id, index, *objective));
                    break 'quests;
                }
            }
        }
        let Some((quest, index, objective)) = open else {
            return Ok(Some((ErrorCode::InvalidQuest, target)));
        };
        match objective {
            QuestObjective::UseItem { item, .. } => {
                let player = self
                    .players
                    .get_mut(&player_id)
                    .ok_or_else(|| ZoneError::new("acting player is absent"))?;
                let Some(revision) = player.inventory_revision.checked_add(1) else {
                    return Ok(Some((ErrorCode::InvalidInventoryMove, target)));
                };
                if player.inventory.remove_item(item, 1).is_err() {
                    return Ok(Some((ErrorCode::MissingMaterials, target)));
                }
                player.inventory_revision = revision;
                self.complete_objective(player_id, quest, index);
                if let Some(confrontation) = content
                    .confrontation(npc)
                    .filter(|confrontation| confrontation.evidence == item)
                {
                    self.confront(*confrontation, player_id)?;
                }
            }
            QuestObjective::Interview { .. } => {
                if self.admission(player_id, npc)? == Admission::Refused {
                    return Ok(Some((ErrorCode::Distrusted, target)));
                }
                self.complete_objective(player_id, quest, index);
            }
            QuestObjective::Escort { .. } => {
                if let Some(escort) = self.escorts.get(&npc) {
                    let code = if escort.player == player_id {
                        ErrorCode::InvalidQuest
                    } else {
                        ErrorCode::NpcBusy
                    };
                    return Ok(Some((code, target)));
                }
                let placement = self
                    .npc_placement(npc)
                    .ok_or_else(|| ZoneError::new("escorted npc has no placement"))?;
                self.escorts.insert(
                    npc,
                    EscortState {
                        npc,
                        player: player_id,
                        position: placement.position,
                        facing: placement.facing,
                        velocity: [0, 0],
                        trail: Vec::new(),
                        behind: 0,
                        rests_until: 0,
                    },
                );
            }
            // A delivery takes the load of its source out of the player's arms (#1077).
            QuestObjective::DeliverLoad { source, .. } => {
                if !self.hand_over_load(player_id, source) {
                    return Ok(Some((ErrorCode::NotCarrying, target)));
                }
                self.complete_objective(player_id, quest, index);
            }
            // A defence (#1019) starts at its NPC; it completes when it holds.
            QuestObjective::Defend { .. } => {
                if !self.begin_defence()? {
                    return Ok(Some((ErrorCode::SiteNotReady, target)));
                }
            }
            _ => return Ok(Some((ErrorCode::InvalidQuest, target))),
        }
        Ok(None)
    }

    /// Completes objective `index` of `quest` in the player's log, once.
    fn complete_objective(&mut self, player_id: PlayerId, quest: QuestId, index: usize) -> bool {
        let Some(required) = self
            .content
            .quest(quest)
            .and_then(|definition| definition.objectives.get(index))
            .map(|objective| objective.required())
        else {
            return false;
        };
        let Some(player) = self.players.get_mut(&player_id) else {
            return false;
        };
        let Some(entry) = player
            .quests
            .entries
            .iter_mut()
            .find(|entry| entry.quest == quest)
        else {
            return false;
        };
        if entry.progress[index] >= required {
            return false;
        }
        entry.progress[index] += 1;
        let count = entry.progress[index];
        push_event(
            &mut player.events,
            ZoneEvent::QuestProgress {
                quest,
                objective: u8::try_from(index).unwrap_or(u8::MAX),
                count,
            },
        );
        true
    }

    /// Completes every open objective of the player's active quests that `matches` (#1019),
    /// once each.
    pub(crate) fn complete_objectives(
        &mut self,
        player_id: PlayerId,
        matches: impl Fn(QuestObjective) -> bool,
    ) {
        let content = Arc::clone(&self.content);
        let Some(player) = self.players.get(&player_id) else {
            return;
        };
        let open: Vec<_> = player
            .quests
            .entries
            .iter()
            .filter_map(|entry| Some((entry, content.quest(entry.quest)?)))
            .flat_map(|(entry, quest)| {
                quest
                    .objectives
                    .iter()
                    .enumerate()
                    .filter(|(index, objective)| {
                        matches(**objective) && entry.progress[*index] < objective.required()
                    })
                    .map(|(index, _)| (quest.id, index))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (quest, index) in open {
            self.complete_objective(player_id, quest, index);
        }
    }

    /// The NPC turns on the player who showed it the evidence: its foe stands and attacks.
    fn confront(
        &mut self,
        confrontation: Confrontation,
        player_id: PlayerId,
    ) -> Result<(), ZoneError> {
        let foe = confrontation.foe;
        if self
            .creatures
            .get(&foe)
            .is_some_and(|state| matches!(state.life, Life::Despawned { .. }))
        {
            self.spawn_creature(foe)?;
        }
        self.engage(foe, EntityRef::Player(player_id), false)
    }

    /// The open escort objective of `npc` in the player's log: its quest, index and area.
    fn open_escort(
        &self,
        player_id: PlayerId,
        npc: NpcId,
    ) -> Option<(QuestId, usize, crate::AreaId)> {
        let player = self.players.get(&player_id)?;
        player.quests.entries.iter().find_map(|entry| {
            let quest = self.content.quest(entry.quest)?;
            quest
                .objectives
                .iter()
                .enumerate()
                .find_map(|(index, objective)| match *objective {
                    QuestObjective::Escort { npc: wanted, area }
                        if wanted == npc && entry.progress[index] == 0 =>
                    {
                        Some((quest.id, index, area))
                    }
                    _ => None,
                })
        })
    }

    /// Ends `npc`'s escort; a failed one tells its player.
    fn end_escort(&mut self, npc: NpcId, failed: bool) {
        let Some(escort) = self.escorts.remove(&npc) else {
            return;
        };
        if failed && let Some(player) = self.players.get_mut(&escort.player) {
            push_event(
                &mut player.events,
                ZoneEvent::Error {
                    code: ErrorCode::EscortLost,
                    target: Some(EntityRef::Npc(npc)),
                },
            );
        }
    }

    /// Tick step: every escort follows its player's trail one step, arrives, fails or ends its
    /// rest (see the module documentation), and a confronted foe that gave up its fight leaves
    /// the world so its NPC is back.
    pub(crate) fn advance_npc_quests(&mut self, now: u64) -> Result<(), ZoneError> {
        let npcs: Vec<NpcId> = self.escorts.keys().copied().collect();
        for npc in npcs {
            let Some(mut escort) = self.escorts.get(&npc).cloned() else {
                continue;
            };
            self.tick_work.escorts_advanced += 1;
            if escort.rests_until > 0 {
                if now >= escort.rests_until {
                    self.end_escort(npc, false);
                }
                continue;
            }
            let leader = self
                .players
                .get(&escort.player)
                .filter(|player| player.is_alive())
                .map(|_| self.player_position(escort.player))
                .transpose()?;
            let (Some(leader), Some((quest, index, area))) =
                (leader, self.open_escort(escort.player, npc))
            else {
                self.end_escort(npc, true);
                continue;
            };
            let feet = [leader.x, leader.z];
            // The trail: where the player walked, so the NPC goes where a body could.
            let last = escort.trail.last().copied().unwrap_or(escort.position);
            if distance(last, feet) >= ESCORT_TRAIL_SPACING_UNITS {
                escort.trail.push(feet);
                if escort.trail.len() > MAX_ESCORT_TRAIL {
                    escort.trail.remove(0);
                }
            }
            let from = escort.position;
            let mut position = from;
            let mut budget = ESCORT_UNITS_PER_TICK;
            while budget > 0 && distance(position, feet) > ESCORT_GAP_UNITS {
                let Some(&next) = escort.trail.first() else {
                    break;
                };
                self.tick_work.escort_trail_points += 1;
                let step = distance(position, next);
                if step <= budget {
                    position = next;
                    budget -= step;
                    escort.trail.remove(0);
                } else {
                    let lerp = |a: i32, b: i32| {
                        i32::try_from(
                            i64::from(a) + (i64::from(b) - i64::from(a)) * budget / step.max(1),
                        )
                        .unwrap_or(a)
                    };
                    position = [lerp(position[0], next[0]), lerp(position[1], next[1])];
                    budget = 0;
                }
            }
            let moved = [position[0] - from[0], position[1] - from[1]];
            escort.velocity =
                moved.map(|axis| i8::try_from(axis.clamp(-127, 127)).unwrap_or_default());
            if let Some(facing) = crate::trig::yaw_from_vector(moved[0], moved[1]) {
                escort.facing = facing;
            }
            escort.position = position;
            if distance(position, feet) > ESCORT_LEASH_UNITS {
                escort.behind = escort.behind.saturating_add(1);
            } else {
                escort.behind = 0;
            }
            if escort.behind > ESCORT_LEASH_TICKS {
                self.end_escort(npc, true);
                continue;
            }
            let arrived = self
                .content
                .areas()
                .area_at(position[0], position[1])
                .is_some_and(|candidate| candidate.id() == area);
            if arrived {
                escort.velocity = [0, 0];
                escort.trail.clear();
                escort.rests_until = now.saturating_add(u64::from(ESCORT_REST_TICKS));
                let player = escort.player;
                self.escorts.insert(npc, escort);
                self.complete_objective(player, quest, index);
                // Party mates walking beside it share the rescue (Atlas 5.4.17).
                for mate in self.party_mates(player) {
                    if mate != player
                        && self
                            .players
                            .get(&mate)
                            .is_some_and(|state| state.is_alive())
                        && self.within_quest_reach(mate, npc)?
                    {
                        self.complete_objectives(mate, |objective| {
                            objective == QuestObjective::Escort { npc, area }
                        });
                    }
                }
                continue;
            }
            self.escorts.insert(npc, escort);
        }
        // A confronted foe out of its fight leaves; its NPC is back.
        for index in 0..self.content.confrontations().len() {
            let confrontation = self.content.confrontations()[index];
            self.tick_work.confrontations_checked += 1;
            if self.creatures.get(&confrontation.foe).is_some_and(|foe| {
                foe.is_alive()
                    && foe.ai != crate::snapshot::CreatureAi::Engaged
                    && foe.combat_timer == 0
            }) {
                self.retire_raider(confrontation.foe, now)?;
            }
        }
        Ok(())
    }

    /// Canonical escorts in NPC order.
    pub(crate) fn canonical_escorts(&self) -> Vec<EscortState> {
        self.escorts.values().cloned().collect()
    }

    /// Restores escorts from a snapshot taken at `tick`.
    pub(crate) fn restore_escorts(
        &mut self,
        escorts: &[EscortState],
        tick: u64,
    ) -> Result<(), ZoneError> {
        let escorted = |npc: NpcId| {
            self.content.quests().iter().any(|quest| {
                quest
                    .objectives
                    .iter()
                    .any(|objective| matches!(objective, QuestObjective::Escort { npc: wanted, .. } if *wanted == npc))
            })
        };
        if escorts.windows(2).any(|pair| pair[0].npc >= pair[1].npc)
            || escorts.iter().any(|escort| {
                !escorted(escort.npc)
                    || escort.trail.len() > MAX_ESCORT_TRAIL
                    || escort.behind > ESCORT_LEASH_TICKS
                    || escort.rests_until > tick.saturating_add(u64::from(ESCORT_REST_TICKS))
                    || escort
                        .position
                        .iter()
                        .chain(escort.trail.iter().flatten())
                        .any(|value| value.abs() > crate::MAX_CONTENT_COORDINATE_UNITS)
            })
        {
            return Err(ZoneError::new("snapshot escorts are invalid"));
        }
        self.escorts = escorts
            .iter()
            .map(|escort| (escort.npc, escort.clone()))
            .collect();
        Ok(())
    }
}
