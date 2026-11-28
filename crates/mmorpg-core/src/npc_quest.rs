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
//!   [`crate::ErrorCode::Distrusted`]. Sponsoring completes nothing for the sponsor. An NPC that
//!   distrusts obvious arcane equipment ([`NpcAttitude::arcane`]) also distrusts anyone with a
//!   staff in the main hand, read from what is equipped; when only the staff stands between them it
//!   refuses with [`crate::ErrorCode::StaffShown`], and putting the staff away is enough.
//! - **Escort**: starts an [`EscortState`]: the NPC follows the player's trail at a wounded
//!   walk, keeping [`ESCORT_GAP_UNITS`] behind. Inside the objective's area it has arrived: the
//!   objective is done for the escorting player and for each party mate with the same objective
//!   open within quest reach, and the NPC rests there for [`ESCORT_REST_TICKS`] before it is back
//!   at its post. A player more than [`ESCORT_LEASH_UNITS`] ahead for [`ESCORT_LEASH_TICKS`], a
//!   dead, absent or quitting player fails it ([`crate::ErrorCode::EscortLost`]): the NPC is back
//!   at its post at once and the player asks again. An escorted NPC wades but never swims: one led
//!   where the water is deeper than [`crate::water::SWIM_DEPTH_UNITS`] balks and fails it with
//!   [`crate::ErrorCode::TooDeep`] (#1019), so a marsh's wading line is the safe way through. One
//!   player escorts an NPC at a time.
//! - **Homecoming** (#1019, [`crate::QuestObjective::EscortTo`]): an escort that ends within quest
//!   reach of another NPC instead of an area. The escorted NPC's [`EscortCompanion`]s, a family,
//!   start with it and walk in a line, each following the trail of the one ahead of it as the
//!   lead follows the player's. The line arrives once the lead reached its destination with every
//!   companion within [`ESCORT_GATHER_UNITS`] of it, and rests together; any of it left too far
//!   behind fails the whole line.
//! - **Recruit** (#1019): an NPC with an [`NpcAttitude`] may refuse as for an interview (its
//!   recommendation or a sponsor still admits). The recruit completes the objective for the
//!   recruiter and for every party mate in the zone with the same recruit open, so a party splits
//!   up to gather a muster in parallel; each character's own log stores what it was credited.
//!   A quest's recruits are open in any order.
//! - **Delivery** (#1077): takes the load of the objective's source out of the player's arms
//!   ([`crate::LoadState`]); empty arms are refused with [`crate::ErrorCode::NotCarrying`].
//! - **Defence**: starts the defence of the raid site its NPC keeps ([`crate::raid`],
//!   [`crate::RaidSite::steward`]) at once when that site is ready; otherwise
//!   [`crate::ErrorCode::SiteNotReady`]. It completes nothing itself.
//! - **Watch**: starts the watch over the flock ([`crate::flock`]) at once when the flock stands
//!   and no watch runs; otherwise [`crate::ErrorCode::FlockUnsettled`]. It completes nothing itself.
//! - **Sight and ambush**: calls the road's supply patrol out at its lookout ([`crate::supply`]),
//!   or joins the run under way; while the last run rests, [`crate::ErrorCode::PatrolResting`]. It
//!   completes nothing itself.
//! - **Crossing**: starts the carter's crossing of the ford ([`crate::crossing`]), or joins the one
//!   under way; while the last one settles, [`crate::ErrorCode::CartResting`]. It completes
//!   nothing itself.
//! - **Decision** ([`crate::ZoneCommand::Decide`]): stores the chosen option. A paid option
//!   ([`crate::DecisionOption::copper`]) takes its copper at once, or is refused with
//!   [`crate::ErrorCode::NotEnoughMoney`]. An option that hands over an item puts it in the bag
//!   (a full bag refuses the choice), and a fight option turns the NPC hostile through its
//!   [`Confrontation`]. While the quest still has an open objective after the decision, a quest
//!   action at the NPC carries the option out again ([`crate::Quest::unresolved_decision`]).
//!
//! The escorts are canonical zone state, so snapshots, replay and recovery continue them; the
//! log stores the completed objectives like a talk objective's.

use std::sync::Arc;

use crate::creature::Life;
use crate::events::push_event;
use crate::reach::xz_distance;
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
/// A content's escort companions number at most this many (#1019).
pub const MAX_ESCORT_COMPANIONS: usize = 8;
/// An escort's companions have gathered once each stands within 6 m of its lead (#1019).
pub const ESCORT_GATHER_UNITS: i64 = 600;

/// An NPC who walks in an escorted NPC's line (#1019): when an escort of the line's lead starts,
/// it follows the NPC ahead of it, `follows`, the way an escorted NPC follows its player. The
/// line arrives together, and the line's escort fails if any of it is lost.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EscortCompanion {
    pub npc: NpcId,
    pub follows: NpcId,
}

/// Where an escort ends (#1019).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Destination {
    /// Inside a named area.
    Area(crate::AreaId),
    /// Within quest reach of an NPC.
    Npc(NpcId),
}

/// How an NPC regards those who would interview it (#1019).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NpcAttitude {
    pub npc: NpcId,
    /// The classes whose visible practice it distrusts.
    pub distrusts: Vec<PlayerClass>,
    /// It distrusts obvious arcane equipment (#1019): a staff in the main hand, whatever the class.
    /// Putting the staff away conceals it.
    pub arcane: bool,
    /// A trusted party mate within reach may speak for a player it distrusts.
    pub sponsor: bool,
    /// Having turned in this quest is a recommendation it accepts.
    pub recommendation: Option<QuestId>,
}

/// Why an NPC hears a player, or why it does not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Trusted,
    Recommended,
    Sponsored(PlayerId),
    /// It distrusts the player's class.
    Refused,
    /// It distrusts only the staff the player holds (#1019): put away, it would hear them.
    StaffShown,
}

impl Admission {
    /// The refusal a quest action answers with, if it is one (#1019).
    #[must_use]
    pub const fn refusal(self) -> Option<ErrorCode> {
        match self {
            Self::Refused => Some(ErrorCode::Distrusted),
            Self::StaffShown => Some(ErrorCode::StaffShown),
            Self::Trusted | Self::Recommended | Self::Sponsored(_) => None,
        }
    }
}

/// An NPC who turns hostile (#1019): when shown `evidence`, or when a quest decision's fight
/// option is chosen at it ([`crate::DecisionOption::fight`]). Its reserved `foe` stands and fights.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Confrontation {
    pub npc: NpcId,
    pub evidence: Option<ItemId>,
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

    /// Whether `npc` left `player_id`'s world (#1019): a [`crate::NpcDeparture`] whose quest the
    /// character turned in with its outcome. Everyone else still sees and reaches it.
    #[must_use]
    pub fn npc_departed(&self, player_id: PlayerId, npc: NpcId) -> bool {
        let Some(player) = self.players.get(&player_id) else {
            return false;
        };
        self.content.departures().iter().any(|departure| {
            departure.npc == npc
                && player.quests.outcome(departure.quest) == Some(departure.outcome)
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
        let class_distrusted = |player: PlayerId| {
            self.players
                .get(&player)
                .and_then(|state| state.class)
                .is_some_and(|choice| attitude.distrusts.contains(&choice.class))
        };
        // Obvious arcane equipment (#1019): a staff in the main hand, read from what is equipped.
        let staff_shown = |player: PlayerId| {
            attitude.arcane
                && self.players.get(&player).is_some_and(|state| {
                    state
                        .equipment
                        .item(crate::EquipmentSlot::MainHand)
                        .and_then(crate::item_template)
                        .is_some_and(|template| template.weapon == Some(crate::WeaponFamily::Staff))
                })
        };
        let distrusted = |player: PlayerId| class_distrusted(player) || staff_shown(player);
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
        if attitude.sponsor
            && let Some(mate) = self
                .party_mates_near(player_id, npc)?
                .into_iter()
                .find(|&mate| !distrusted(mate))
        {
            return Ok(Admission::Sponsored(mate));
        }
        // Put away, the staff would not stand between them (#1019).
        Ok(if class_distrusted(player_id) {
            Admission::Refused
        } else {
            Admission::StaffShown
        })
    }

    /// The `QuestAction` intent (#1019) at `npc`, or with `decision` the `Decide` intent: see the
    /// module documentation.
    pub(crate) fn quest_action(
        &mut self,
        player_id: PlayerId,
        npc: NpcId,
        decision: Option<u8>,
    ) -> Result<Refusal, ZoneError> {
        let target = Some(EntityRef::Npc(npc));
        let content = Arc::clone(&self.content);
        if content.npc(npc).is_none() || self.npc_hidden(npc) || self.npc_departed(player_id, npc) {
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
                // A decision is only ever decided, never acted on (#1019).
                let deciding = matches!(objective, QuestObjective::Decide { .. });
                if objective.action_npc() != Some(npc)
                    || current[index] >= objective.required()
                    || deciding != decision.is_some()
                {
                    continue;
                }
                // A muster's recruits (#1019) come in any order.
                let recruiting = matches!(objective, QuestObjective::Recruit { .. });
                let ready =
                    quest.objectives[..index]
                        .iter()
                        .zip(current)
                        .all(|(earlier, count)| {
                            count >= earlier.required()
                                || (recruiting && matches!(earlier, QuestObjective::Recruit { .. }))
                        });
                if ready {
                    open = Some((quest.id, index, *objective));
                    break 'quests;
                }
            }
        }
        let Some((quest, index, objective)) = open else {
            // A decided option that still acts in the world (#1019) is carried out again: the
            // item handed over anew, the fight faced again.
            if decision.is_none() {
                let unresolved = player.quests.entries.iter().find_map(|entry| {
                    let quest = content.quest(entry.quest)?;
                    let current =
                        crate::quest::progress(quest, entry, &player.inventory, *player.depths);
                    quest
                        .unresolved_decision(&current, npc)
                        .map(|(_, option)| option.clone())
                });
                if let Some(option) = unresolved {
                    return self.carry_out(player_id, npc, &option);
                }
            }
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
                self.advance_objective(player_id, quest, index);
                if let Some(confrontation) = content
                    .confrontation(npc)
                    .filter(|confrontation| confrontation.evidence == Some(item))
                {
                    self.confront(*confrontation, player_id)?;
                }
            }
            QuestObjective::Interview { .. } => {
                if let Some(code) = self.admission(player_id, npc)?.refusal() {
                    return Ok(Some((code, target)));
                }
                self.advance_objective(player_id, quest, index);
            }
            // An escort starts its NPC and, for a homecoming, the companions in its line (#1019).
            QuestObjective::Escort { .. } | QuestObjective::EscortTo { .. } => {
                let line = content.escort_line(npc);
                if let Some(escort) = std::iter::once(&npc)
                    .chain(&line)
                    .find_map(|member| self.escorts.get(member))
                {
                    let code = if escort.player == player_id {
                        ErrorCode::InvalidQuest
                    } else {
                        ErrorCode::NpcBusy
                    };
                    return Ok(Some((code, target)));
                }
                for member in std::iter::once(npc).chain(line) {
                    let placement = self
                        .npc_placement(member)
                        .ok_or_else(|| ZoneError::new("escorted npc has no placement"))?;
                    self.escorts.insert(
                        member,
                        EscortState {
                            npc: member,
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
            }
            // A recruit (#1019) counts for the recruiter and each party mate with it open.
            QuestObjective::Recruit { .. } => {
                if let Some(code) = self.admission(player_id, npc)?.refusal() {
                    return Ok(Some((code, target)));
                }
                self.advance_objective(player_id, quest, index);
                for mate in self.party_mates(player_id) {
                    if mate != player_id {
                        self.complete_objectives(mate, |objective| {
                            objective == QuestObjective::Recruit { npc }
                        });
                    }
                }
            }
            // A delivery takes the load of its source out of the player's arms (#1077).
            QuestObjective::DeliverLoad { source, .. } => {
                if !self.hand_over_load(player_id, source) {
                    return Ok(Some((ErrorCode::NotCarrying, target)));
                }
                self.advance_objective(player_id, quest, index);
            }
            // A decision (#1019) stores the chosen option until the turn-in keeps it; a paid
            // option takes its copper now or is refused, an option that hands over an item needs
            // room for it, and a fight option turns the NPC on the player.
            QuestObjective::Decide { .. } => {
                let Some((option, chosen)) = decision.and_then(|option| {
                    let chosen = content
                        .quest(quest)?
                        .decision
                        .get(usize::from(option).checked_sub(1)?)?;
                    Some((option, chosen.clone()))
                }) else {
                    return Ok(Some((ErrorCode::InvalidQuest, target)));
                };
                let player = self
                    .players
                    .get_mut(&player_id)
                    .ok_or_else(|| ZoneError::new("deciding player is absent"))?;
                let mut bag = player.inventory.clone();
                if let Some(item) = chosen.hands
                    && bag.insert(item, 1).is_err()
                {
                    return Ok(Some((ErrorCode::InventoryFull, target)));
                }
                if chosen.copper > 0 || chosen.hands.is_some() {
                    let Some(copper) = player.copper.checked_sub(chosen.copper) else {
                        return Ok(Some((ErrorCode::NotEnoughMoney, target)));
                    };
                    let Some(revision) = player.inventory_revision.checked_add(1) else {
                        return Ok(Some((ErrorCode::InvalidInventoryMove, target)));
                    };
                    player.copper = copper;
                    player.inventory = bag;
                    player.inventory_revision = revision;
                }
                if let Some(entry) = player
                    .quests
                    .entries
                    .iter_mut()
                    .find(|entry| entry.quest == quest)
                {
                    entry.progress[index] = option;
                    push_event(
                        &mut player.events,
                        ZoneEvent::QuestProgress {
                            quest,
                            objective: u8::try_from(index).unwrap_or(u8::MAX),
                            count: 1,
                        },
                    );
                }
                if chosen.fight
                    && let Some(confrontation) = content.confrontation(npc)
                {
                    self.confront(*confrontation, player_id)?;
                }
            }
            // A defence (#1019) starts at its NPC; it completes when it holds.
            QuestObjective::Defend { npc } => {
                if !self.begin_defence(npc)? {
                    return Ok(Some((ErrorCode::SiteNotReady, target)));
                }
            }
            // A watch over the flock (#1019) starts at its shepherd; it completes when it holds.
            QuestObjective::Watch { .. } => {
                if !self.begin_watch() {
                    return Ok(Some((ErrorCode::FlockUnsettled, target)));
                }
            }
            // The road's supply patrol (#1019) is called out at its lookout, or joined while it
            // marches; it completes when it is sighted or stopped.
            QuestObjective::Sight { .. } | QuestObjective::Ambush { .. } => {
                if !self.call_supply_patrol(npc)? {
                    return Ok(Some((ErrorCode::PatrolResting, target)));
                }
            }
            // The carter's crossing (#1019) starts at the carter, or is joined while it runs; it
            // completes when the cart reaches the far bank alive.
            QuestObjective::Crossing { .. } => {
                if !self.begin_crossing(npc) {
                    return Ok(Some((ErrorCode::CartResting, target)));
                }
            }
            _ => return Ok(Some((ErrorCode::InvalidQuest, target))),
        }
        Ok(None)
    }

    /// Carries a decided option out again at `npc` (#1019, [`crate::Quest::unresolved_decision`]):
    /// hands its item over once more while the bag holds none, or turns the NPC on the player
    /// again. Nothing to do is an invalid quest.
    fn carry_out(
        &mut self,
        player_id: PlayerId,
        npc: NpcId,
        option: &crate::DecisionOption,
    ) -> Result<Refusal, ZoneError> {
        let target = Some(EntityRef::Npc(npc));
        if option.fight {
            let Some(confrontation) = self.content.confrontation(npc).copied() else {
                return Ok(Some((ErrorCode::InvalidQuest, target)));
            };
            self.confront(confrontation, player_id)?;
            return Ok(None);
        }
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("acting player is absent"))?;
        let Some(item) = option
            .hands
            .filter(|item| player.inventory.count(*item) == 0)
        else {
            return Ok(Some((ErrorCode::InvalidQuest, target)));
        };
        let Some(revision) = player.inventory_revision.checked_add(1) else {
            return Ok(Some((ErrorCode::InvalidInventoryMove, target)));
        };
        if player.inventory.insert(item, 1).is_err() {
            return Ok(Some((ErrorCode::InventoryFull, target)));
        }
        player.inventory_revision = revision;
        Ok(None)
    }

    /// Completes every open objective of the player's active quests that `matches` (#1019),
    /// once each.
    pub(crate) fn complete_objectives(
        &mut self,
        player_id: PlayerId,
        matches: impl Fn(QuestObjective) -> bool,
    ) {
        self.complete_quest_objectives(player_id, |_, objective| matches(objective));
    }

    /// Completes every open objective of the player's active quests that `matches` with its
    /// quest (#1019), once each.
    pub(crate) fn complete_quest_objectives(
        &mut self,
        player_id: PlayerId,
        matches: impl Fn(&crate::Quest, QuestObjective) -> bool,
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
                        matches(quest, **objective) && entry.progress[*index] < objective.required()
                    })
                    .map(|(index, _)| (quest.id, index))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (quest, index) in open {
            self.advance_objective(player_id, quest, index);
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

    /// The open escort objective of `npc` in the player's log: its quest, index and destination.
    fn open_escort(
        &self,
        player_id: PlayerId,
        npc: NpcId,
    ) -> Option<(QuestId, usize, Destination)> {
        let player = self.players.get(&player_id)?;
        player.quests.entries.iter().find_map(|entry| {
            let quest = self.content.quest(entry.quest)?;
            quest
                .objectives
                .iter()
                .enumerate()
                .find_map(|(index, objective)| {
                    let destination = match *objective {
                        QuestObjective::Escort { npc: wanted, area } if wanted == npc => {
                            Destination::Area(area)
                        }
                        QuestObjective::EscortTo { npc: wanted, to } if wanted == npc => {
                            Destination::Npc(to)
                        }
                        _ => return None,
                    };
                    (entry.progress[index] == 0).then_some((quest.id, index, destination))
                })
        })
    }

    /// The escorted NPC leading `npc`'s line (#1019): itself unless it is a companion.
    fn line_lead(&self, npc: NpcId) -> NpcId {
        let mut lead = npc;
        while let Some(ahead) = self.content.companion_of(lead) {
            lead = ahead;
        }
        lead
    }

    /// Ends the escort of `lead` and its line's companions (#1019); a failed one tells its player
    /// why (`failure`).
    fn end_escort(&mut self, lead: NpcId, failure: Option<ErrorCode>) {
        for companion in self.content.escort_line(lead) {
            self.escorts.remove(&companion);
        }
        let Some(escort) = self.escorts.remove(&lead) else {
            return;
        };
        if let Some(code) = failure
            && let Some(player) = self.players.get_mut(&escort.player)
        {
            push_event(
                &mut player.events,
                ZoneEvent::Error {
                    code,
                    target: Some(EntityRef::Npc(lead)),
                },
            );
        }
    }

    /// Tick step: every escorted NPC follows a step's worth of the trail of the one it follows,
    /// its player or the companion ahead of it in its line (#1019); the line arrives together,
    /// fails or ends its rest (see the module documentation). A confronted foe that gave up its
    /// fight leaves the world so its NPC is back.
    pub(crate) fn advance_npc_quests(&mut self, now: u64) -> Result<(), ZoneError> {
        let npcs: Vec<NpcId> = self.escorts.keys().copied().collect();
        for npc in npcs {
            let Some(mut escort) = self.escorts.get(&npc).cloned() else {
                continue;
            };
            self.tick_work.escorts_advanced += 1;
            let lead = self.line_lead(npc);
            if escort.rests_until > 0 {
                if now >= escort.rests_until {
                    self.end_escort(lead, None);
                }
                continue;
            }
            // The one it follows: its player, or the NPC ahead of it in its line, which the
            // NPC order has already moved this tick.
            let ahead = match self.content.companion_of(npc) {
                Some(ahead) => self.escorts.get(&ahead).map(|ahead| ahead.position),
                None => self
                    .players
                    .get(&escort.player)
                    .filter(|player| player.is_alive())
                    .map(|_| self.player_position(escort.player))
                    .transpose()?
                    .map(|leader| [leader.x, leader.z]),
            };
            let (Some(feet), Some((quest, index, destination))) =
                (ahead, self.open_escort(escort.player, lead))
            else {
                self.end_escort(lead, Some(ErrorCode::EscortLost));
                continue;
            };
            // The trail: where the one ahead walked, so the NPC goes where a body could.
            let last = escort.trail.last().copied().unwrap_or(escort.position);
            if xz_distance(last, feet) >= ESCORT_TRAIL_SPACING_UNITS {
                escort.trail.push(feet);
                if escort.trail.len() > MAX_ESCORT_TRAIL {
                    escort.trail.remove(0);
                }
            }
            let from = escort.position;
            let mut position = from;
            let mut budget = ESCORT_UNITS_PER_TICK;
            while budget > 0 && xz_distance(position, feet) > ESCORT_GAP_UNITS {
                let Some(&next) = escort.trail.first() else {
                    break;
                };
                self.tick_work.escort_trail_points += 1;
                let step = xz_distance(position, next);
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
            if xz_distance(position, feet) > ESCORT_LEASH_UNITS {
                escort.behind = escort.behind.saturating_add(1);
            } else {
                escort.behind = 0;
            }
            if escort.behind > ESCORT_LEASH_TICKS {
                self.end_escort(lead, Some(ErrorCode::EscortLost));
                continue;
            }
            // An escorted NPC wades but never swims (#1019): led into deeper water, it balks.
            if self
                .water_depth_at(position[0], position[1])
                .is_some_and(|depth| depth > crate::water::SWIM_DEPTH_UNITS)
            {
                self.end_escort(lead, Some(ErrorCode::TooDeep));
                continue;
            }
            // A companion only follows; its lead decides the arrival.
            if npc != lead {
                self.escorts.insert(npc, escort);
                continue;
            }
            let reached = match destination {
                Destination::Area(area) => self
                    .content
                    .areas()
                    .area_at(position[0], position[1])
                    .is_some_and(|candidate| candidate.id() == area),
                Destination::Npc(to) => self.npc_placement(to).is_some_and(|place| {
                    xz_distance(place.position, position)
                        <= i64::from(crate::quest::QUEST_REACH_UNITS)
                }),
            };
            let line = self.content.escort_line(npc);
            let gathered = line.iter().all(|companion| {
                self.escorts.get(companion).is_some_and(|companion| {
                    xz_distance(companion.position, position) <= ESCORT_GATHER_UNITS
                })
            });
            if reached && gathered {
                let until = now.saturating_add(u64::from(ESCORT_REST_TICKS));
                escort.velocity = [0, 0];
                escort.trail.clear();
                escort.rests_until = until;
                let player = escort.player;
                self.escorts.insert(npc, escort);
                for companion in &line {
                    if let Some(companion) = self.escorts.get_mut(companion) {
                        companion.velocity = [0, 0];
                        companion.trail.clear();
                        companion.rests_until = until;
                    }
                }
                self.advance_objective(player, quest, index);
                let Some(objective) = self
                    .content
                    .quest(quest)
                    .and_then(|definition| definition.objectives.get(index))
                    .copied()
                else {
                    continue;
                };
                // Party mates walking beside it share the rescue (Atlas 5.4.17).
                for mate in self.party_mates_near(player, npc)? {
                    self.complete_objectives(mate, |candidate| candidate == objective);
                }
                continue;
            }
            self.escorts.insert(npc, escort);
        }
        // A confronted foe out of its fight leaves; its NPC is back.
        for index in 0..self.content.confrontations().len() {
            let confrontation = self.content.confrontations()[index];
            self.tick_work.confrontations_checked += 1;
            if self
                .creatures
                .get(&confrontation.foe)
                .is_some_and(|foe| foe.is_alive() && foe.out_of_fight())
            {
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
        // An escorted NPC or a companion in an escorted NPC's line (#1019).
        let escorted = |npc: NpcId| {
            self.content.companion_of(npc).is_some()
                || self.content.quests().iter().any(|quest| {
                    quest.objectives.iter().any(|objective| match *objective {
                        QuestObjective::Escort { npc: wanted, .. }
                        | QuestObjective::EscortTo { npc: wanted, .. } => wanted == npc,
                        _ => false,
                    })
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
