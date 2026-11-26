//! Quests (#25): immutable quest definitions bound to zone content, each
//! player's bounded quest log, and the zone-owned accept, abandon, progress
//! and turn-in rules with their per-player NPC markers.
//!
//! Objectives are kills of a creature template, items collected from drops,
//! talking to an NPC and exploring a named area. Kill, talk and explore
//! progress is stored in the log; collect progress is the bag's count of the
//! item, so selling or moving it changes progress and the turn-in consumes
//! it. Quest items drop only while a quest still needs them. Retrieve objectives
//! (#274) grant their item when the player swims into their area without one;
//! like collect, their progress is the bag's count. A Thorn objective's progress is the
//! character's durable victory evidence (5.1.37), so a victory won before the quest was taken
//! counts without another fight.
//!
//! Inspect objectives (#1019) complete when the player inspects their authored evidence spot
//! ([`crate::EvidenceSpot`]) with the quest active; the log stores that progress.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use crate::events::push_event;
use crate::{
    AreaId, CreatureTemplateId, DepthsRecord, EntityRef, ErrorCode, INVENTORY_SLOTS, Inventory,
    InventoryError, ItemId, NpcId, NpcRole, PlayerId, ZoneContent, ZoneError, ZoneEvent,
    ZoneSimulation, item_template,
};

/// A player's quest log holds at most this many active quests.
pub const MAX_QUEST_LOG: usize = 10;
pub const MAX_QUEST_OBJECTIVES: usize = 3;
/// A turn-in offers at most this many items to choose one from.
pub const MAX_REWARD_CHOICES: usize = 4;
pub const MAX_QUEST_NAME_BYTES: usize = 64;
pub const MAX_QUEST_TEXT_BYTES: usize = 240;
/// Inclusive horizontal (XZ) distance from an NPC's feet within which a
/// player accepts, turns in or talks (5 m).
pub const QUEST_REACH_UNITS: i32 = 500;
/// The most quest experience one turn-in grants.
pub const MAX_QUEST_EXPERIENCE: u32 = 10_000;

/// Stable quest identity within one content revision; `0` names no quest.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QuestId(u32);

impl QuestId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// One thing a quest asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuestObjective {
    /// Kill `count` creatures of `template` as their tapper.
    Kill {
        template: CreatureTemplateId,
        count: u8,
    },
    /// Carry `count` units of `item`, which creatures of `source` drop
    /// while the quest needs more.
    Collect {
        item: ItemId,
        count: u8,
        source: CreatureTemplateId,
    },
    /// Stand within [`QUEST_REACH_UNITS`] of `npc`.
    Talk { npc: NpcId },
    /// Enter `area`.
    Explore { area: AreaId },
    /// Swim into `area`, which grants one `item` while the bag holds none (#274); progress
    /// is the bag's count, and the turn-in consumes it.
    Retrieve { item: ItemId, area: AreaId },
    /// Have beaten Captain Ulric Thorn in a Redbrand Depths run. Progress is the character's
    /// durable [`DepthsRecord::thorn_defeated`], never stored in the log, so a victory from
    /// before the quest was accepted satisfies it (5.1.37).
    DefeatThorn,
    /// Inspect authored evidence `spot` (#1019) with [`crate::ZoneCommand::Inspect`]; the log
    /// stores the progress.
    Inspect { spot: crate::EvidenceId },
    /// Use one `item` from the bag on `npc` (#1019) with [`crate::ZoneCommand::QuestAction`]:
    /// bandage a wounded guard, dose a farmhand or show a courier the evidence. The action
    /// consumes the item; the log stores the progress.
    UseItem { item: ItemId, npc: NpcId },
    /// Lead `npc` into `area` (#1019): [`crate::ZoneCommand::QuestAction`] at the NPC starts
    /// the escort once the quest's earlier objectives are done, and the NPC follows the player's
    /// trail ([`crate::EscortState`]). Straying too far or dying fails it; the log stores the
    /// arrival.
    Escort { npc: NpcId, area: AreaId },
    /// Be heard by `npc` (#1019) with [`crate::ZoneCommand::QuestAction`]; an NPC with an
    /// [`crate::NpcAttitude`] may refuse the player.
    Interview { npc: NpcId },
    /// Kill `count` creatures of `template` as their tapper while wielding the quest's loaned
    /// weapon (#1019, one of its [`Quest::supplies`] that is a main-hand weapon).
    DefeatWithLoan {
        template: CreatureTemplateId,
        count: u8,
    },
    /// Hit `count` creatures of `template` with a boulder the player set rolling (#1019): the
    /// environment's damage, each impact once (a roll hits a victim once), not a kill.
    Crush {
        template: CreatureTemplateId,
        count: u8,
    },
    /// Destroy structure piece `piece` (#1019) with the player's own blast or shot.
    Demolish { piece: crate::PieceId },
    /// Carry a load from source `source` to `npc` (#1077): lift it there with
    /// [`crate::ZoneCommand::Lift`], then [`crate::ZoneCommand::QuestAction`] at the NPC with it in
    /// both arms hands it over. The log stores the delivery; a lost load is fetched again.
    DeliverLoad {
        source: crate::LoadSpotId,
        npc: NpcId,
    },
    /// Raise construction site `piece` (#1019): carry its material loads to it and set them down
    /// there ([`crate::ConstructionSite`]); the objective is done for every player who brought
    /// one once the piece stands. The log stores it.
    Build { piece: crate::PieceId },
    /// Hold the raid site through a defence (#1019): [`crate::ZoneCommand::QuestAction`] at `npc`
    /// starts one when the site is ready, and the objective is done for each player who fought
    /// it and stands near when it ends with enough of the site's civilians standing
    /// ([`crate::RaidSite::hold`]). The raiders slain do not count. The log stores it.
    Defend { npc: NpcId },
    /// Have `count` aid actions of `chore` accepted at the raid site's aftermath (#1019): mend a
    /// breach or tend a wounded defender ([`crate::ZoneCommand::RaidAid`]). The log stores it.
    Aid {
        chore: crate::raid::AidChore,
        count: u8,
    },
    /// Take one `item` from authored spot `spot` (#1019) with [`crate::ZoneCommand::Inspect`] while
    /// the bag holds none: a rubbing of a drowned inscription. Progress is the bag's count, and the
    /// turn-in consumes it; a lost one is taken again.
    Transcribe {
        spot: crate::EvidenceId,
        item: ItemId,
    },
    /// Help open seal `piece` (#1019): work one of its mechanisms ([`crate::ZoneCommand::Operate`])
    /// toward the opening or while it stands open ([`crate::Seal`]). The log stores it.
    Unseal { piece: crate::PieceId },
    /// Decide the quest's [`Quest::decision`] at `npc` (#1019) with [`crate::ZoneCommand::Decide`].
    /// The log stores the chosen option (1-based) until the turn-in keeps it with the character
    /// ([`QuestLog::outcomes`]).
    Decide { npc: NpcId },
    /// Drive stray `npc` into its pen (#1019, [`crate::Stray`]): the stray shies from players, and
    /// the objective is done for the player who drove it in and party mates within quest reach.
    /// The log stores it.
    Herd { npc: NpcId },
    /// Keep the flock through a night watch (#1019, [`crate::HedgeWatch`]):
    /// [`crate::ZoneCommand::QuestAction`] at its shepherd `npc` starts one, and the objective is
    /// done for each player on the watch's ground when it ends with enough sheep standing. The
    /// prowlers slain do not count. The log stores it.
    Watch { npc: NpcId },
    /// Lay one `item` from the bag at authored spot `spot` (#1019) with
    /// [`crate::ZoneCommand::Inspect`]: bait or a snare at a gap in a hedge. The item is consumed;
    /// the log stores it.
    Lay {
        spot: crate::EvidenceId,
        item: ItemId,
    },
    /// Lead `npc` home to `to` (#1019): as [`QuestObjective::Escort`], but the escort arrives once
    /// the NPC stands within quest reach of `to` and its [`crate::EscortCompanion`]s, a family
    /// walking in a line behind it, all stand near it. The log stores the arrival.
    EscortTo { npc: NpcId, to: NpcId },
    /// Win `npc` over to a muster (#1019) with [`crate::ZoneCommand::QuestAction`]: an NPC with an
    /// [`crate::NpcAttitude`] may refuse the player. A recruit counts for the recruiter and for
    /// every party mate in the zone with the same objective open, so a party gathers its recruits
    /// in parallel while each character's log keeps its own. A quest's recruits come in any order.
    Recruit { npc: NpcId },
    /// Work the `count` mechanisms of [`crate::MechanismSequence`] `sequence` in its order (#1019)
    /// with [`crate::ZoneCommand::Operate`] once the quest's earlier objectives are done: the
    /// standing stones of a ring, in the order an inscription gives. Progress is the stones worked
    /// in order so far; a stone out of order starts it over ([`crate::ErrorCode::OutOfOrder`]).
    /// The log stores it.
    WorkInOrder {
        sequence: crate::SequenceId,
        count: u8,
    },
    /// Watch the supply patrol lookout `npc` calls out come up the road (#1019,
    /// [`crate::SupplyPatrol`]): [`crate::ZoneCommand::QuestAction`] at the lookout calls it out
    /// once the quest's earlier objectives are done, and the objective is done for each player
    /// watching with the lookout when a member comes within its sight. The log stores it.
    Sight { npc: NpcId },
    /// Stop the supply patrol lookout `npc` calls out before it reaches its camp (#1019):
    /// [`crate::ZoneCommand::QuestAction`] at the lookout calls it out, and the objective is done
    /// for each player near the carrier when it falls on the way. A patrol that arrives fails it
    /// ([`crate::ErrorCode::PatrolArrived`]) and is called again. The log stores it.
    Ambush { npc: NpcId },
    /// See the carter `npc`'s cart over the ford alive (#1019, [`crate::WagonCrossing`]):
    /// [`crate::ZoneCommand::QuestAction`] at the carter starts the crossing once the quest's
    /// earlier objectives are done, and the objective is done for each player near the cart when
    /// it reaches the far bank with health left. A cart that falls fails it
    /// ([`crate::ErrorCode::CartLost`]) and is tried again. The log stores it.
    Crossing { npc: NpcId },
    /// Kill `count` creatures of `template` as their tapper with the final blow landing outside
    /// the creature's front arc (#1019, [`crate::flanks`]): a crayfish turned aside by a lure or
    /// by the player's own footwork. A kill from its front does not count.
    Outflank {
        template: CreatureTemplateId,
        count: u8,
    },
}

impl QuestObjective {
    /// The progress that completes the objective.
    #[must_use]
    pub const fn required(self) -> u8 {
        match self {
            Self::Kill { count, .. }
            | Self::Collect { count, .. }
            | Self::DefeatWithLoan { count, .. }
            | Self::Crush { count, .. }
            | Self::Aid { count, .. }
            | Self::WorkInOrder { count, .. }
            | Self::Outflank { count, .. } => count,
            Self::Talk { .. }
            | Self::Explore { .. }
            | Self::Retrieve { .. }
            | Self::DefeatThorn
            | Self::Inspect { .. }
            | Self::UseItem { .. }
            | Self::Escort { .. }
            | Self::Interview { .. }
            | Self::Demolish { .. }
            | Self::DeliverLoad { .. }
            | Self::Build { .. }
            | Self::Defend { .. }
            | Self::Transcribe { .. }
            | Self::Unseal { .. }
            | Self::Decide { .. }
            | Self::Herd { .. }
            | Self::Watch { .. }
            | Self::Lay { .. }
            | Self::EscortTo { .. }
            | Self::Recruit { .. }
            | Self::Sight { .. }
            | Self::Ambush { .. }
            | Self::Crossing { .. } => 1,
        }
    }

    /// Wire and fingerprint code: 1 kill, 2 collect, 3 talk, 4 explore, 5 retrieve, 6 Thorn,
    /// 7 inspect, 8 use item, 9 escort, 10 interview, 11 defeat with a loan, 12 crush,
    /// 13 demolish, 14 deliver a load, 15 build, 16 defend, 17 aid, 18 transcribe, 19 unseal,
    /// 20 decide, 21 herd, 22 watch, 23 lay, 24 escort home, 25 recruit, 26 work in order,
    /// 27 sight, 28 ambush, 29 crossing, 30 outflank.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Kill { .. } => 1,
            Self::Collect { .. } => 2,
            Self::Talk { .. } => 3,
            Self::Explore { .. } => 4,
            Self::Retrieve { .. } => 5,
            Self::DefeatThorn => 6,
            Self::Inspect { .. } => 7,
            Self::UseItem { .. } => 8,
            Self::Escort { .. } => 9,
            Self::Interview { .. } => 10,
            Self::DefeatWithLoan { .. } => 11,
            Self::Crush { .. } => 12,
            Self::Demolish { .. } => 13,
            Self::DeliverLoad { .. } => 14,
            Self::Build { .. } => 15,
            Self::Defend { .. } => 16,
            Self::Aid { .. } => 17,
            Self::Transcribe { .. } => 18,
            Self::Unseal { .. } => 19,
            Self::Decide { .. } => 20,
            Self::Herd { .. } => 21,
            Self::Watch { .. } => 22,
            Self::Lay { .. } => 23,
            Self::EscortTo { .. } => 24,
            Self::Recruit { .. } => 25,
            Self::WorkInOrder { .. } => 26,
            Self::Sight { .. } => 27,
            Self::Ambush { .. } => 28,
            Self::Crossing { .. } => 29,
            Self::Outflank { .. } => 30,
        }
    }

    /// The NPC a [`crate::ZoneCommand::QuestAction`] performs this objective at (#1019), if it
    /// is such an objective.
    #[must_use]
    pub const fn action_npc(self) -> Option<NpcId> {
        match self {
            Self::UseItem { npc, .. }
            | Self::Escort { npc, .. }
            | Self::Interview { npc }
            | Self::DeliverLoad { npc, .. }
            | Self::Defend { npc }
            | Self::Decide { npc }
            | Self::Watch { npc }
            | Self::EscortTo { npc, .. }
            | Self::Recruit { npc }
            | Self::Sight { npc }
            | Self::Ambush { npc }
            | Self::Crossing { npc } => Some(npc),
            _ => None,
        }
    }

    /// The bag item whose count is this objective's progress, if any.
    #[must_use]
    pub const fn carried_item(self) -> Option<ItemId> {
        match self {
            Self::Collect { item, .. }
            | Self::Retrieve { item, .. }
            | Self::Transcribe { item, .. } => Some(item),
            _ => None,
        }
    }

    /// Whether the log's stored count is this objective's progress; otherwise it is read from
    /// the bag or the character's durable evidence and the log stores zero.
    #[must_use]
    pub const fn stores_progress(self) -> bool {
        self.carried_item().is_none() && !matches!(self, Self::DefeatThorn)
    }
}

/// What a turn-in grants: experience, copper and one item of `choices`
/// (none when it is empty).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuestRewards {
    pub experience: u32,
    pub copper: u32,
    pub choices: Vec<ItemId>,
}

/// An authored quest: accepted at `giver`, turned in at `ender` once every
/// objective is complete, and available after `prerequisite` was turned in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quest {
    pub id: QuestId,
    pub name: String,
    /// What the giver says.
    pub text: String,
    pub giver: NpcId,
    pub ender: NpcId,
    pub prerequisite: Option<QuestId>,
    pub objectives: Vec<QuestObjective>,
    pub rewards: QuestRewards,
    /// What the giver hands over on accepting (#1019), at most [`MAX_QUEST_SUPPLIES`].
    pub supplies: Vec<QuestSupply>,
    /// Earlier quests that must be turned in too before it is offered, beside its prerequisite
    /// (#1019), at most [`MAX_QUEST_REQUIRES`].
    pub requires: Vec<QuestId>,
    /// The options of its [`QuestObjective::Decide`] (#1019), as clients offer them; empty
    /// without one, else `2..=MAX_DECISION_OPTIONS`.
    pub decision: Vec<DecisionOption>,
}

/// One option of a quest's decision (#1019): what clients offer and what choosing it costs.
/// A paid option settles a matter with the player's own copper: deciding it takes `copper` from
/// the player at once and is refused ([`ErrorCode::NotEnoughMoney`]) without enough. Both the
/// payment and the choice stand; abandoning the quest later returns neither.
///
/// An option may also settle a matter in the world (#1019): one that `hands` an item has the NPC
/// hand it over at once (a full bag refuses the choice), and a `fight` option turns the NPC on
/// the player ([`crate::Confrontation`]). While a later objective of the quest is still open, an
/// action at the NPC carries the chosen option out again ([`Quest::unresolved_decision`]): a lost
/// item is handed over anew, and a foe that gave up its fight is faced again.
///
/// An option may also pay (#1019): its `reward` is copper the turn-in adds to the quest's own
/// reward when the quest is turned in with that option, so two outcomes can pay differently. It
/// is paid once, with the turn-in, never on deciding, so abandoning and deciding again mints
/// nothing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DecisionOption {
    pub label: String,
    /// Copper the option costs, at most [`MAX_DECISION_COPPER`]; zero for most.
    pub copper: u32,
    /// Copper the turn-in adds when the quest is turned in with this option, at most
    /// [`MAX_DECISION_COPPER`]; zero for most.
    pub reward: u32,
    /// A plain bag item the NPC hands over when it is chosen (#1019).
    pub hands: Option<ItemId>,
    /// Whether choosing it turns the NPC hostile (#1019).
    pub fight: bool,
}

impl DecisionOption {
    /// A free option labelled `label`.
    #[must_use]
    pub fn free(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    /// Whether choosing it does something in the world beyond the choice (#1019).
    #[must_use]
    pub const fn acts(&self) -> bool {
        self.hands.is_some() || self.fight
    }
}

impl Quest {
    /// The decide objective at `npc` whose chosen option acts in the world but has not yet settled
    /// the quest (#1019): with `current` progress (a decide objective's is the chosen option), an
    /// option that [`DecisionOption::acts`] while a later objective is still open. Its index and
    /// the option.
    #[must_use]
    pub fn unresolved_decision(
        &self,
        current: &[u8],
        npc: NpcId,
    ) -> Option<(usize, &DecisionOption)> {
        let index = self
            .objectives
            .iter()
            .position(|objective| *objective == QuestObjective::Decide { npc })?;
        let chosen = usize::from(*current.get(index)?).checked_sub(1)?;
        let option = self.decision.get(chosen).filter(|option| option.acts())?;
        let open = self.objectives[index + 1..]
            .iter()
            .zip(current.get(index + 1..)?)
            .any(|(objective, count)| *count < objective.required());
        open.then_some((index, option))
    }
}

/// A quest gives at most this many kinds of supplies.
pub const MAX_QUEST_SUPPLIES: usize = 4;
/// A quest requires at most this many quests beside its prerequisite.
pub const MAX_QUEST_REQUIRES: usize = 3;
/// A decision offers at most this many options.
pub const MAX_DECISION_OPTIONS: usize = 3;
/// An option's label is at most this long.
pub const MAX_DECISION_BYTES: usize = 48;
/// A paid decision option costs at most this much copper (#1019).
pub const MAX_DECISION_COPPER: u32 = 1_000;

/// An NPC that leaves a character's world once the character turned in `quest` with
/// decision `outcome` (#1019): the character no longer sees or reaches it; everyone else does.
/// Content lets only an NPC leave whose own quests all come before the decision, so leaving
/// strands none.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NpcDeparture {
    pub npc: NpcId,
    pub quest: QuestId,
    /// The 1-based option of the quest's decision.
    pub outcome: u8,
}

/// One item a quest's giver hands over on accepting (#1019), to a character of one of
/// `classes` (every class when empty): a bandage, a draught, a forged pass or a loaned weapon.
/// The bag gets one unless it already holds one, so a retaken quest never multiplies it.
/// Abandoning the quest takes the item back. A plain bag item left at the turn-in is taken
/// back too; equipment is a **loan**: the turn-in requires it, from the bag or the hands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuestSupply {
    pub item: ItemId,
    pub classes: Vec<crate::PlayerClass>,
}

impl QuestSupply {
    /// Whether a character of `class` receives it.
    #[must_use]
    pub fn for_class(&self, class: Option<crate::PlayerClass>) -> bool {
        self.classes.is_empty() || class.is_some_and(|class| self.classes.contains(&class))
    }

    /// Whether it is a loan, returned at the turn-in.
    #[must_use]
    pub fn loaned(&self) -> bool {
        item_template(self.item).is_some_and(|template| template.slot.is_some())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuestContentError {
    QuestCount,
    InvalidId,
    DuplicateId,
    InvalidText,
    InvalidNpc,
    InvalidPrerequisite,
    InvalidObjective,
    InvalidRewards,
}

impl fmt::Display for QuestContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::QuestCount => "zone content has no quests",
            Self::InvalidId => "a quest id must not be 0",
            Self::DuplicateId => "a quest id is used twice",
            Self::InvalidText => "a quest name or text is empty or too long",
            Self::InvalidNpc => "a quest giver or ender is no quest-giver npc",
            Self::InvalidPrerequisite => "a quest prerequisite must be an earlier quest",
            Self::InvalidObjective => "a quest objective names unknown content or no count",
            Self::InvalidRewards => "quest rewards are out of range",
        })
    }
}

impl Error for QuestContentError {}

/// Validates and orders a content's quests by ID.
pub(crate) fn validate_quests(
    content: &ZoneContent,
    mut quests: Vec<Quest>,
) -> Result<Vec<Quest>, QuestContentError> {
    if quests.is_empty() {
        return Err(QuestContentError::QuestCount);
    }
    quests.sort_by_key(|quest| quest.id);
    if quests.iter().any(|quest| quest.id.get() == 0) {
        return Err(QuestContentError::InvalidId);
    }
    if quests.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(QuestContentError::DuplicateId);
    }
    let text = |value: &str, limit| !value.trim().is_empty() && value.len() <= limit;
    let quest_giver = |npc: NpcId| {
        content
            .npc(npc)
            .is_some_and(|npc| npc.role == NpcRole::QuestGiver)
    };
    for quest in &quests {
        if !text(&quest.name, MAX_QUEST_NAME_BYTES) || !text(&quest.text, MAX_QUEST_TEXT_BYTES) {
            return Err(QuestContentError::InvalidText);
        }
        if !quest_giver(quest.giver) || !quest_giver(quest.ender) {
            return Err(QuestContentError::InvalidNpc);
        }
        let earlier = |prerequisite: QuestId| {
            prerequisite < quest.id
                && quests
                    .binary_search_by_key(&prerequisite, |quest| quest.id)
                    .is_ok()
        };
        if quest
            .prerequisite
            .is_some_and(|prerequisite| !earlier(prerequisite))
            || quest.requires.len() > MAX_QUEST_REQUIRES
            || quest.requires.iter().enumerate().any(|(index, required)| {
                !earlier(*required)
                    || quest.prerequisite == Some(*required)
                    || quest.requires[..index].contains(required)
            })
        {
            return Err(QuestContentError::InvalidPrerequisite);
        }
        // A decision (#1019): its options with exactly one decide objective, and back.
        let decides = quest
            .objectives
            .iter()
            .filter(|objective| matches!(objective, QuestObjective::Decide { .. }))
            .count();
        // An option hands over a plain bag item; the NPC binding checks a fighting NPC has its
        // confrontation.
        let options_valid = quest.decision.iter().all(|option| {
            text(&option.label, MAX_DECISION_BYTES)
                && option.copper <= MAX_DECISION_COPPER
                && option.reward <= MAX_DECISION_COPPER
                && option.hands.is_none_or(|item| {
                    item_template(item).is_some_and(|template| template.slot.is_none())
                })
        });
        if !options_valid
            || match decides {
                0 => !quest.decision.is_empty(),
                1 => !(2..=MAX_DECISION_OPTIONS).contains(&quest.decision.len()),
                _ => true,
            }
        {
            return Err(QuestContentError::InvalidObjective);
        }
        if quest.objectives.is_empty() || quest.objectives.len() > MAX_QUEST_OBJECTIVES {
            return Err(QuestContentError::InvalidObjective);
        }
        for (index, objective) in quest.objectives.iter().enumerate() {
            let valid = match *objective {
                QuestObjective::Kill { template, count } => {
                    count > 0 && content.creature_template(template).is_some()
                }
                QuestObjective::Collect {
                    item,
                    count,
                    source,
                } => {
                    // Progress is the bag's count of the item, so one item backs one
                    // objective, and the count must fit in a full bag of its stacks.
                    let repeated = quest.objectives[..index]
                        .iter()
                        .any(|earlier| earlier.carried_item() == Some(item));
                    count > 0
                        && !repeated
                        && item_template(item).is_some_and(|template| {
                            template.slot.is_none()
                                && usize::from(count)
                                    <= usize::from(template.max_stack) * INVENTORY_SLOTS
                        })
                        && content.creature_template(source).is_some()
                }
                QuestObjective::Talk { npc } => content.npc(npc).is_some(),
                QuestObjective::Explore { area } => content
                    .areas()
                    .areas()
                    .iter()
                    .any(|candidate| candidate.id() == area),
                QuestObjective::Retrieve { item, area } => {
                    let repeated = quest.objectives[..index]
                        .iter()
                        .any(|earlier| earlier.carried_item() == Some(item));
                    !repeated
                        && item_template(item).is_some_and(|template| template.slot.is_none())
                        && content
                            .areas()
                            .areas()
                            .iter()
                            .any(|candidate| candidate.id() == area)
                }
                QuestObjective::DefeatThorn => true,
                // One spot backs one objective of a quest, like an item.
                QuestObjective::Inspect { spot } => {
                    content.evidence_spot(spot).is_some()
                        && !quest.objectives[..index].contains(objective)
                }
                // The NPC actions (#1019), each at most once per quest: actions at one NPC come in
                // the quest's order (bandage, then escort). A used item is a plain bag item.
                QuestObjective::UseItem { item, npc } => {
                    content.npc(npc).is_some()
                        && item_template(item).is_some_and(|template| template.slot.is_none())
                        && !quest.objectives[..index].contains(objective)
                }
                QuestObjective::Escort { npc, area } => {
                    content.npc(npc).is_some()
                        && content
                            .areas()
                            .areas()
                            .iter()
                            .any(|candidate| candidate.id() == area)
                        && !quest.objectives[..index].contains(objective)
                }
                QuestObjective::Interview { npc } | QuestObjective::Recruit { npc } => {
                    content.npc(npc).is_some() && !quest.objectives[..index].contains(objective)
                }
                // A homecoming (#1019) leads one NPC to another.
                QuestObjective::EscortTo { npc, to } => {
                    npc != to
                        && content.npc(npc).is_some()
                        && content.npc(to).is_some()
                        && !quest.objectives[..index].contains(objective)
                }
                // The trial needs a weapon to lend.
                QuestObjective::DefeatWithLoan { template, count } => {
                    count > 0
                        && content.creature_template(template).is_some()
                        && quest.supplies.iter().any(|supply| {
                            item_template(supply.item).is_some_and(|template| {
                                template.slot == Some(crate::EquipmentSlot::MainHand)
                            })
                        })
                }
                QuestObjective::Crush { template, count }
                | QuestObjective::Outflank { template, count } => {
                    count > 0 && content.creature_template(template).is_some()
                }
                QuestObjective::Demolish { piece } => {
                    content
                        .pieces()
                        .iter()
                        .any(|candidate| candidate.id == piece)
                        && !quest.objectives[..index].contains(objective)
                }
                // One delivery of a source per quest (#1077), to an NPC the content has.
                QuestObjective::DeliverLoad { source, npc } => {
                    content.npc(npc).is_some()
                        && content.load_spot(source).is_some()
                        && !quest.objectives[..index].contains(objective)
                }
                // Construction, defence and aid (#1019), each once per quest: a build names a
                // construction site, a defence is started at an NPC the content has.
                QuestObjective::Build { piece } => {
                    content.construction(piece).is_some()
                        && !quest.objectives[..index].contains(objective)
                }
                QuestObjective::Defend { npc } => {
                    content.npc(npc).is_some() && !quest.objectives[..index].contains(objective)
                }
                QuestObjective::Aid { chore, count } => {
                    count > 0
                        && !quest.objectives[..index].iter().any(|earlier| {
                            matches!(*earlier, QuestObjective::Aid { chore: other, .. } if other == chore)
                        })
                }
                // A transcription (#1019) takes a plain item from a known spot, one per item.
                QuestObjective::Transcribe { spot, item } => {
                    let repeated = quest.objectives[..index]
                        .iter()
                        .any(|earlier| earlier.carried_item() == Some(item));
                    !repeated
                        && content.evidence_spot(spot).is_some()
                        && item_template(item).is_some_and(|template| template.slot.is_none())
                }
                QuestObjective::Unseal { piece } => {
                    content.seal(piece).is_some() && !quest.objectives[..index].contains(objective)
                }
                QuestObjective::Decide { npc } => content.npc(npc).is_some(),
                // The flock (#1019): a herd and a watch each once per quest at a known NPC (the
                // livestock binding checks they are a stray and the shepherd), a lay once per spot
                // with a plain bag item.
                QuestObjective::Herd { npc } | QuestObjective::Watch { npc } => {
                    content.npc(npc).is_some() && !quest.objectives[..index].contains(objective)
                }
                // The road's supply patrol (#1019): each once per quest at a known NPC (the patrol
                // binding checks it is a patrol's lookout).
                // The ford's crossing (#1019) likewise: the crossing binding checks it is a carter.
                QuestObjective::Sight { npc }
                | QuestObjective::Ambush { npc }
                | QuestObjective::Crossing { npc } => {
                    content.npc(npc).is_some() && !quest.objectives[..index].contains(objective)
                }
                QuestObjective::Lay { spot, item } => {
                    content.evidence_spot(spot).is_some()
                        && item_template(item).is_some_and(|template| template.slot.is_none())
                        && !quest.objectives[..index].iter().any(|earlier| {
                            matches!(*earlier, QuestObjective::Lay { spot: other, .. } if other == spot)
                        })
                }
                // An ordered sequence (#1019): a known one, its whole length, once per quest.
                QuestObjective::WorkInOrder { sequence, count } => {
                    content.sequence(sequence).is_some_and(|known| {
                        usize::from(count) == known.mechanisms.len()
                    }) && !quest.objectives[..index].contains(objective)
                }
            };
            if !valid {
                return Err(QuestContentError::InvalidObjective);
            }
        }
        let choices = &quest.rewards.choices;
        let distinct = choices
            .iter()
            .enumerate()
            .all(|(index, item)| !choices[..index].contains(item));
        if choices.len() > MAX_REWARD_CHOICES
            || !distinct
            || choices.iter().any(|item| item_template(*item).is_none())
            || quest.rewards.experience > MAX_QUEST_EXPERIENCE
        {
            return Err(QuestContentError::InvalidRewards);
        }
        // Supplies (#1019): known distinct items, each weapon wielded by every class it goes to.
        let supplies = &quest.supplies;
        let valid_supply = |supply: &QuestSupply| {
            item_template(supply.item).is_some_and(|template| {
                template.weapon.is_none_or(|family| {
                    !supply.classes.is_empty()
                        && supply
                            .classes
                            .iter()
                            .all(|class| family.wielded_by(Some(*class)))
                })
            })
        };
        if supplies.len() > MAX_QUEST_SUPPLIES
            || supplies
                .iter()
                .enumerate()
                .any(|(index, supply)| supplies[..index].iter().any(|s| s.item == supply.item))
            || !supplies.iter().all(valid_supply)
        {
            return Err(QuestContentError::InvalidRewards);
        }
    }
    Ok(quests)
}

/// One active quest and its stored progress per objective. Collect, retrieve
/// and Thorn objectives store zero: their progress is the bag's count or the
/// character's durable victory evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuestEntry {
    pub quest: QuestId,
    pub progress: [u8; MAX_QUEST_OBJECTIVES],
}

/// A player's quests: at most [`MAX_QUEST_LOG`] active entries in quest-ID
/// order and the turned-in quests, as many as content defines.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuestLog {
    pub entries: Vec<QuestEntry>,
    pub completed: BTreeSet<QuestId>,
    /// The option each turned-in quest with a decision was turned in with (#1019), 1-based:
    /// the character's durable story outcome.
    pub outcomes: BTreeMap<QuestId, u8>,
}

impl QuestLog {
    /// The option `quest`'s decision was turned in with (#1019), if it was.
    #[must_use]
    pub fn outcome(&self, quest: QuestId) -> Option<u8> {
        self.outcomes.get(&quest).copied()
    }

    #[must_use]
    pub fn entry(&self, quest: QuestId) -> Option<&QuestEntry> {
        self.entries.iter().find(|entry| entry.quest == quest)
    }

    #[must_use]
    pub fn is_completed(&self, quest: QuestId) -> bool {
        self.completed.contains(&quest)
    }
}

/// What an NPC's marker shows one player.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QuestMarker {
    /// Grey `?`: a quest that ends here is still in progress.
    InProgress,
    /// `!`: a quest starts here.
    Available,
    /// `?`: a quest is ready to turn in here.
    Complete,
}

impl QuestMarker {
    /// Wire code: 1 available, 2 in progress, 3 complete.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Available => 1,
            Self::InProgress => 2,
            Self::Complete => 3,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Available),
            2 => Some(Self::InProgress),
            3 => Some(Self::Complete),
            _ => None,
        }
    }
}

/// One NPC's marker for the viewer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NpcMarker {
    pub npc: NpcId,
    pub marker: QuestMarker,
}

/// The viewer's quests as the sheet projects them: entries carry the
/// current progress of every objective, collect counts included, and the
/// markers of every quest NPC that shows one, in NPC-ID order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuestSheet {
    pub completed: BTreeSet<QuestId>,
    pub entries: Vec<QuestEntry>,
    pub markers: Vec<NpcMarker>,
}

/// Current progress of every objective of `quest` for `entry`.
pub(crate) fn progress(
    quest: &Quest,
    entry: &QuestEntry,
    bag: &Inventory,
    depths: DepthsRecord,
) -> [u8; MAX_QUEST_OBJECTIVES] {
    let mut current = [0; MAX_QUEST_OBJECTIVES];
    for (index, objective) in quest.objectives.iter().enumerate() {
        current[index] = match (*objective, objective.carried_item()) {
            (QuestObjective::DefeatThorn, _) => u8::from(depths.thorn_defeated),
            // A decision's progress is its chosen option (#1019), so clients see which.
            (QuestObjective::Decide { .. }, _) => {
                entry.progress[index].min(stored_limit(quest, Some(objective)))
            }
            (_, Some(item)) => {
                let count = objective.required();
                u8::try_from(bag.count(item).min(u32::from(count))).unwrap_or(count)
            }
            (_, None) => entry.progress[index].min(objective.required()),
        };
    }
    current
}

pub(crate) fn objectives_done(
    quest: &Quest,
    entry: &QuestEntry,
    bag: &Inventory,
    depths: DepthsRecord,
) -> bool {
    let current = progress(quest, entry, bag, depths);
    quest
        .objectives
        .iter()
        .zip(current)
        .all(|(objective, count)| count >= objective.required())
}

/// The log, mask and markers one player sees.
pub(crate) fn quest_sheet(
    content: &ZoneContent,
    log: &QuestLog,
    bag: &Inventory,
    depths: DepthsRecord,
    night: bool,
) -> QuestSheet {
    let entries = log
        .entries
        .iter()
        .map(|entry| QuestEntry {
            quest: entry.quest,
            // Carried quests included, so a log read in the depths shows the same progress.
            progress: content
                .log_quest(entry.quest)
                .map_or(entry.progress, |quest| progress(quest, entry, bag, depths)),
        })
        .collect();
    QuestSheet {
        completed: log.completed.clone(),
        entries,
        markers: markers(content, log, bag, depths, night),
    }
}

/// Markers of every quest giver and ender, in NPC-ID order: `?` for a
/// quest ready to turn in, else `!` for an available quest, else grey `?`
/// for one still in progress.
/// At `night` a private-home resident shows no `!` (#158).
pub(crate) fn markers(
    content: &ZoneContent,
    log: &QuestLog,
    bag: &Inventory,
    depths: DepthsRecord,
    night: bool,
) -> Vec<NpcMarker> {
    let mut npcs: Vec<NpcId> = content
        .quests()
        .iter()
        .flat_map(|quest| [quest.giver, quest.ender])
        .collect();
    npcs.sort_unstable();
    npcs.dedup();
    npcs.into_iter()
        .filter_map(|npc| {
            let mut best = None;
            for quest in content.quests() {
                let marker = match log.entry(quest.id) {
                    Some(entry) if quest.ender == npc => {
                        if objectives_done(quest, entry, bag, depths) {
                            Some(QuestMarker::Complete)
                        } else {
                            Some(QuestMarker::InProgress)
                        }
                    }
                    None if quest.giver == npc
                        && available(log, quest)
                        && !(night && content.private_home(npc).is_some()) =>
                    {
                        Some(QuestMarker::Available)
                    }
                    _ => None,
                };
                best = best.max(marker);
            }
            best.map(|marker| NpcMarker { npc, marker })
        })
        .collect()
}

/// Neither active nor turned in, and its prerequisite and required quests were turned in.
fn available(log: &QuestLog, quest: &Quest) -> bool {
    log.entry(quest.id).is_none() && !log.is_completed(quest.id) && unlocked(log, quest)
}

/// Whether `quest`'s prerequisite and required quests (#1019) were turned in.
pub(crate) fn unlocked(log: &QuestLog, quest: &Quest) -> bool {
    quest
        .prerequisite
        .iter()
        .chain(&quest.requires)
        .all(|before| log.is_completed(*before))
}

/// The most progress the log may store for `objective` of `quest`: its count where it stores
/// progress, a decision's chosen option (#1019), zero otherwise.
fn stored_limit(quest: &Quest, objective: Option<&QuestObjective>) -> u8 {
    match objective {
        Some(QuestObjective::Decide { .. }) => u8::try_from(quest.decision.len()).unwrap_or(0),
        Some(objective) if objective.stores_progress() => objective.required(),
        _ => 0,
    }
}

/// Recovery checks of a log against its content: ordered unique active
/// quests of this content that are available apart from being active,
/// progress within each objective's count (zero for collect objectives),
/// and turned-in quests whose prerequisites were turned in.
///
/// A quest log belongs to the character, not the zone (#145): the quests
/// content carries for other zones ([`ZoneContent::with_carried_quests`])
/// count as this content's here, so the log crosses unchanged.
pub(crate) fn validate_log(content: &ZoneContent, log: &QuestLog) -> Result<(), ZoneError> {
    let invalid = || Err(ZoneError::new("player quest log is out of range"));
    if log.entries.len() > MAX_QUEST_LOG
        || log
            .entries
            .windows(2)
            .any(|pair| pair[0].quest >= pair[1].quest)
    {
        return invalid();
    }
    for quest in content.log_quests() {
        if log.is_completed(quest.id) && !unlocked(log, quest) {
            return invalid();
        }
    }
    // A decision's outcome (#1019) belongs to a turned-in quest with one of its options.
    if log.outcomes.iter().any(|(quest, outcome)| {
        !log.is_completed(*quest)
            || content
                .log_quest(*quest)
                .is_none_or(|quest| !(1..=quest.decision.len()).contains(&usize::from(*outcome)))
    }) {
        return invalid();
    }
    if log
        .completed
        .iter()
        .any(|quest| content.log_quest(*quest).is_none())
    {
        return invalid();
    }
    for entry in &log.entries {
        let Some(quest) = content.log_quest(entry.quest) else {
            return invalid();
        };
        if log.is_completed(quest.id) || !unlocked(log, quest) {
            return invalid();
        }
        for (index, &stored) in entry.progress.iter().enumerate() {
            if stored > stored_limit(quest, quest.objectives.get(index)) {
                return invalid();
            }
        }
    }
    Ok(())
}

/// A saved log fitted to the content admitting it (#984): content may revise an active
/// quest's objectives, so progress a character stored past an objective's count is that
/// objective done, and progress where the objective stores none is dropped. Everything else
/// still answers to [`validate_log`].
pub(crate) fn fit_log(content: &ZoneContent, log: &QuestLog) -> QuestLog {
    let mut log = log.clone();
    for entry in &mut log.entries {
        let Some(quest) = content.log_quest(entry.quest) else {
            continue;
        };
        for (index, stored) in entry.progress.iter_mut().enumerate() {
            *stored = (*stored).min(stored_limit(quest, quest.objectives.get(index)));
        }
    }
    log
}

/// The quest a creature of `template` drops for a log, if one still needs it.
pub(crate) fn quest_drop(
    content: &ZoneContent,
    log: &QuestLog,
    bag: &Inventory,
    template: CreatureTemplateId,
) -> Option<ItemId> {
    log.entries.iter().find_map(|entry| {
        content
            .quest(entry.quest)?
            .objectives
            .iter()
            .find_map(|objective| match *objective {
                QuestObjective::Collect {
                    item,
                    count,
                    source,
                } if source == template && bag.count(item) < u32::from(count) => Some(item),
                _ => None,
            })
    })
}

/// Whether some quest of `content` drops `item` from `template`.
pub(crate) fn drops_quest_item(
    content: &ZoneContent,
    template: CreatureTemplateId,
    item: ItemId,
) -> bool {
    content.quests().iter().any(|quest| {
        quest.objectives.iter().any(|objective| {
            matches!(*objective, QuestObjective::Collect { item: candidate, source, .. }
                if candidate == item && source == template)
        })
    })
}

/// Takes `quest`'s supplies (#1019) out of `bag` and `hands`: every one still held, from the
/// bag first. With `loans_due` (a turn-in, by a character of that class) each loan meant for the class
/// must come back, so a missing one leaves both untouched and answers `false`.
fn reclaim_supplies(
    quest: &Quest,
    bag: &mut Inventory,
    hands: &mut crate::Equipment,
    loans_due: Option<Option<crate::PlayerClass>>,
) -> bool {
    let (mut staged_bag, mut slots) = (bag.clone(), *hands.slots());
    for supply in &quest.supplies {
        let carried = staged_bag.count(supply.item);
        let wielded = slots.iter().position(|slot| *slot == Some(supply.item));
        if carried == 0 && wielded.is_none() {
            // Only a loan whose borrower could have had it is due: a class-bound loan given to
            // another class never existed.
            if loans_due.is_some_and(|class| supply.loaned() && supply.for_class(class)) {
                return false;
            }
            continue;
        }
        if carried > 0 {
            let _ = staged_bag.remove_item(supply.item, u16::try_from(carried).unwrap_or(u16::MAX));
        }
        if let Some(index) = wielded {
            slots[index] = None;
        }
    }
    let Ok(staged_hands) = crate::Equipment::from_slots(slots) else {
        return false;
    };
    *bag = staged_bag;
    *hands = staged_hands;
    true
}

/// A quest request at an NPC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuestRequest {
    Accept { npc: NpcId, quest: u32 },
    Complete { npc: NpcId, quest: u32, choice: u8 },
    Abandon { quest: u32 },
}

type Refusal = Option<(ErrorCode, Option<EntityRef>)>;

impl ZoneSimulation {
    /// Tick step 1 of the quest intents. Accepting and turning in need a
    /// living player within reach of the quest's giver or ender; refusals
    /// change nothing. A turn-in settles bag, copper and log together.
    pub(crate) fn quest_request(
        &mut self,
        player_id: PlayerId,
        request: QuestRequest,
    ) -> Result<Refusal, ZoneError> {
        match request {
            QuestRequest::Abandon { quest } => {
                let player = self
                    .players
                    .get_mut(&player_id)
                    .ok_or_else(|| ZoneError::new("quest player is absent"))?;
                let before = player.quests.entries.len();
                player
                    .quests
                    .entries
                    .retain(|entry| entry.quest != QuestId::new(quest));
                if player.quests.entries.len() == before {
                    return Ok(Some((ErrorCode::InvalidQuest, None)));
                }
                // The giver takes its supplies back (#1019); an escort the quest ran ends.
                if let Some(definition) = self.content.quest(QuestId::new(quest)) {
                    let mut bag = player.inventory.clone();
                    let mut hands = *player.equipment;
                    reclaim_supplies(definition, &mut bag, &mut hands, None);
                    if (bag != player.inventory || hands != *player.equipment)
                        && let Some(revision) = player.inventory_revision.checked_add(1)
                    {
                        player.inventory = bag;
                        *player.equipment = hands;
                        player.inventory_revision = revision;
                    }
                }
                // A load no active quest delivers any more is gone (#1077).
                self.forget_loads(player_id);
                Ok(None)
            }
            QuestRequest::Accept { npc, quest } => self.accept_quest(player_id, npc, quest),
            QuestRequest::Complete { npc, quest, choice } => {
                self.complete_quest(player_id, npc, quest, choice)
            }
        }
    }

    /// Whether the player's feet are within quest reach of `npc`'s.
    pub(crate) fn within_quest_reach(
        &self,
        player_id: PlayerId,
        npc: NpcId,
    ) -> Result<bool, ZoneError> {
        // A confronted NPC (#1019) is out of the world while its foe stands, and a departed one
        // out of this character's.
        let Some(feet) = self
            .npc_feet(npc)
            .filter(|_| !self.npc_hidden(npc) && !self.npc_departed(player_id, npc))
        else {
            return Ok(false);
        };
        let from = self.player_position(player_id)?;
        // i128 keeps the squared distance exact for any recovered i32 position.
        let [dx, dz] = [
            i128::from(from.x) - i128::from(feet[0]),
            i128::from(from.z) - i128::from(feet[1]),
        ];
        Ok(dx * dx + dz * dz <= i128::from(QUEST_REACH_UNITS).pow(2)
            && self.npc_in_sight(player_id, npc)?)
    }

    fn accept_quest(
        &mut self,
        player_id: PlayerId,
        npc: NpcId,
        quest: u32,
    ) -> Result<Refusal, ZoneError> {
        let target = Some(EntityRef::Npc(npc));
        self.keep_awake(npc);
        let Some(definition) = self
            .content
            .quest(QuestId::new(quest))
            .filter(|definition| definition.giver == npc)
        else {
            return Ok(Some((ErrorCode::InvalidQuest, target)));
        };
        // A private-home resident only takes turn-ins at night (#158).
        if self.resting(npc) {
            return Ok(Some((ErrorCode::NotAtNight, target)));
        }
        let id = definition.id;
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("quest player is absent"))?;
        if !available(&player.quests, definition) {
            return Ok(Some((ErrorCode::InvalidQuest, target)));
        }
        if !self.within_quest_reach(player_id, npc)? {
            return Ok(Some((ErrorCode::OutOfRange, target)));
        }
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("quest player is absent"))?;
        if player.quests.entries.len() >= MAX_QUEST_LOG {
            return Ok(Some((ErrorCode::QuestLogFull, target)));
        }
        // The giver's supplies (#1019): one of each meant for the class, unless already held.
        let class = player.class.map(|choice| choice.class);
        let mut bag = player.inventory.clone();
        for supply in &definition.supplies {
            let held =
                bag.count(supply.item) > 0 || player.equipment.slots().contains(&Some(supply.item));
            if supply.for_class(class) && !held && bag.insert(supply.item, 1).is_err() {
                return Ok(Some((ErrorCode::InventoryFull, target)));
            }
        }
        if bag != player.inventory {
            let Some(revision) = player.inventory_revision.checked_add(1) else {
                return Ok(Some((ErrorCode::InvalidInventoryMove, target)));
            };
            player.inventory = bag;
            player.inventory_revision = revision;
        }
        let at = player
            .quests
            .entries
            .partition_point(|entry| entry.quest < id);
        player.quests.entries.insert(
            at,
            QuestEntry {
                quest: id,
                progress: [0; MAX_QUEST_OBJECTIVES],
            },
        );
        Ok(None)
    }

    fn complete_quest(
        &mut self,
        player_id: PlayerId,
        npc: NpcId,
        quest: u32,
        choice: u8,
    ) -> Result<Refusal, ZoneError> {
        let target = Some(EntityRef::Npc(npc));
        let content = std::sync::Arc::clone(&self.content);
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("quest player is absent"))?;
        let Some((definition, entry)) = content
            .quest(QuestId::new(quest))
            .filter(|definition| definition.ender == npc)
            .and_then(|definition| Some((definition, *player.quests.entry(definition.id)?)))
        else {
            return Ok(Some((ErrorCode::InvalidQuest, target)));
        };
        self.keep_awake(npc);
        let reward = match definition.rewards.choices.as_slice() {
            [] if choice == 0 => None,
            [] => return Ok(Some((ErrorCode::InvalidQuest, target))),
            choices => match choices.get(usize::from(choice)) {
                Some(&item) => Some(item),
                None => return Ok(Some((ErrorCode::InvalidQuest, target))),
            },
        };
        if !self.within_quest_reach(player_id, npc)? {
            return Ok(Some((ErrorCode::OutOfRange, target)));
        }
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("quest player is absent"))?;
        if !objectives_done(definition, &entry, &player.inventory, *player.depths) {
            return Ok(Some((ErrorCode::QuestIncomplete, target)));
        }
        let grant = crate::RewardKey::QuestCompletion {
            quest: definition.id,
        };
        if !crate::rewards::allowed(&self.rewards, player_id, grant) {
            return Ok(Some((ErrorCode::InvalidQuest, target)));
        }
        // A turn-in the tick cannot record waits for the player's retry (#1112).
        if !crate::rewards::fits(&self.rewards, 1) {
            return Ok(Some((ErrorCode::RewardsBusy, target)));
        }
        // Stage bag and copper; any refusal leaves both and the log intact.
        let mut bag = player.inventory.clone();
        for objective in &definition.objectives {
            if let Some(item) = objective.carried_item()
                && bag
                    .remove_item(item, u16::from(objective.required()))
                    .is_err()
            {
                return Ok(Some((ErrorCode::QuestIncomplete, target)));
            }
        }
        // Loans come back and leftover supplies go (#1019), before the reward takes their room.
        let mut hands = *player.equipment;
        let class = player.class.map(|choice| choice.class);
        if !reclaim_supplies(definition, &mut bag, &mut hands, Some(class)) {
            return Ok(Some((ErrorCode::QuestIncomplete, target)));
        }
        if let Some(item) = reward {
            match bag.insert(item, 1) {
                Ok(()) => {}
                Err(InventoryError::NoCapacity) => {
                    return Ok(Some((ErrorCode::InventoryFull, target)));
                }
                Err(_) => return Ok(Some((ErrorCode::InvalidInventoryMove, target))),
            }
        }
        // The decision (#1019): its index, and the reward of the option it was decided with.
        let decided = definition
            .objectives
            .iter()
            .position(|objective| matches!(objective, QuestObjective::Decide { .. }));
        let bonus = decided
            .and_then(|index| usize::from(entry.progress[index]).checked_sub(1))
            .and_then(|option| definition.decision.get(option))
            .map_or(0, |option| option.reward);
        let Some(copper) = player
            .copper
            .checked_add(definition.rewards.copper)
            .and_then(|copper| copper.checked_add(bonus))
        else {
            return Ok(Some((ErrorCode::MoneyOverflow, target)));
        };
        let revision = if bag == player.inventory && hands == *player.equipment {
            None
        } else {
            match player.inventory_revision.checked_add(1) {
                Some(revision) => Some(revision),
                None => return Ok(Some((ErrorCode::InvalidInventoryMove, target))),
            }
        };
        player.inventory = bag;
        if hands != *player.equipment {
            *player.equipment = hands;
        }
        player.copper = copper;
        if let Some(revision) = revision {
            player.inventory_revision = revision;
        }
        player
            .quests
            .entries
            .retain(|candidate| candidate.quest != definition.id);
        player.quests.completed.insert(definition.id);
        // The decision (#1019) stays with the character.
        if let Some(index) = decided {
            player
                .quests
                .outcomes
                .insert(definition.id, entry.progress[index]);
        }
        push_event(
            &mut player.events,
            ZoneEvent::QuestCompleted {
                quest: definition.id,
            },
        );
        self.grant_experience(player_id, definition.rewards.experience)?;
        crate::rewards::record(&mut self.rewards, player_id, grant)?;
        Ok(None)
    }

    /// The tapper's kill credit: every active kill objective of the
    /// template that is not yet complete advances by one, and an outflank objective (#1019) when
    /// the final blow was `flanked`.
    pub(crate) fn credit_kill(
        &mut self,
        player_id: PlayerId,
        template: CreatureTemplateId,
        creature: crate::CreatureId,
        life: u64,
        flanked: bool,
    ) -> Result<(), ZoneError> {
        let content = std::sync::Arc::clone(&self.content);
        let Some(player) = self.players.get(&player_id) else {
            return Ok(());
        };
        // A trial's kill counts only with its loaned weapon in hand (#1019).
        let wielded = player.equipment.item(crate::EquipmentSlot::MainHand);
        let mut due = Vec::new();
        for entry in &player.quests.entries {
            let Some(quest) = content.quest(entry.quest) else {
                continue;
            };
            let loaned = wielded.is_some_and(|item| {
                quest
                    .supplies
                    .iter()
                    .any(|supply| supply.item == item && supply.loaned())
            });
            for (index, objective) in quest.objectives.iter().enumerate() {
                let wanted = match *objective {
                    QuestObjective::Kill { template, count } => Some((template, count)),
                    QuestObjective::DefeatWithLoan { template, count } if loaned => {
                        Some((template, count))
                    }
                    QuestObjective::Outflank { template, count } if flanked => {
                        Some((template, count))
                    }
                    _ => None,
                };
                if let Some((wanted, count)) = wanted
                    && wanted == template
                    && entry.progress[index] < count
                {
                    let key = crate::RewardKey::KillObjective {
                        creature,
                        life,
                        quest: quest.id,
                        objective: u8::try_from(index)
                            .map_err(|_| ZoneError::new("quest objective index overflow"))?,
                    };
                    if crate::rewards::allowed(&self.rewards, player_id, key) {
                        due.push(key);
                    }
                }
            }
        }
        for key in due {
            self.pay_zone_reward(player_id, key, crate::rewards::RewardPayout::default())?;
        }
        Ok(())
    }

    /// A boulder `player_id` set rolling hit living `creature` (#1019): every open crush
    /// objective of its template advances by one. One roll hits a victim once (the impact's
    /// forced-move immunity), so each impact counts once, on every host alike.
    pub(crate) fn credit_crush(
        &mut self,
        player_id: PlayerId,
        creature: crate::CreatureId,
    ) -> Result<(), ZoneError> {
        let content = std::sync::Arc::clone(&self.content);
        if !self
            .creatures
            .get(&creature)
            .is_some_and(crate::creature::CreatureState::is_alive)
        {
            return Ok(());
        }
        let template = Self::creature_content(&content, creature)?.1.id;
        self.complete_objectives(player_id, |objective| {
            matches!(objective, QuestObjective::Crush { template: wanted, .. } if wanted == template)
        });
        Ok(())
    }

    /// Advances one open kill objective by one (#1112: also a deferred kill credit's
    /// payout); `false` when the quest left the log or the objective is already complete.
    pub(crate) fn advance_kill_objective(
        &mut self,
        player_id: PlayerId,
        quest: QuestId,
        objective: u8,
    ) -> bool {
        let content = std::sync::Arc::clone(&self.content);
        let index = usize::from(objective);
        // Kill credits pay kills, trial kills and flank kills (#1019) alike.
        let Some(
            QuestObjective::Kill { count, .. }
            | QuestObjective::DefeatWithLoan { count, .. }
            | QuestObjective::Outflank { count, .. },
        ) = content
            .quest(quest)
            .and_then(|definition| definition.objectives.get(index).copied())
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
        if entry.progress[index] >= count {
            return false;
        }
        entry.progress[index] += 1;
        push_event(
            &mut player.events,
            ZoneEvent::QuestProgress {
                quest,
                objective,
                count: entry.progress[index],
            },
        );
        true
    }

    /// Progress events for collect objectives of `item` after the bag
    /// gained it.
    pub(crate) fn report_collected(&mut self, player_id: PlayerId, item: ItemId) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(player) = self.players.get_mut(&player_id) else {
            return;
        };
        let held = player.inventory.count(item);
        for entry in &player.quests.entries {
            let Some(quest) = content.quest(entry.quest) else {
                continue;
            };
            for (index, objective) in quest.objectives.iter().enumerate() {
                if let QuestObjective::Collect {
                    item: wanted,
                    count,
                    ..
                } = *objective
                    && wanted == item
                {
                    push_event(
                        &mut player.events,
                        ZoneEvent::QuestProgress {
                            quest: quest.id,
                            objective: u8::try_from(index).unwrap_or(u8::MAX),
                            count: u8::try_from(held.min(u32::from(count))).unwrap_or(count),
                        },
                    );
                }
            }
        }
    }

    /// Progress events for Thorn objectives after the character's first victory evidence.
    pub(crate) fn report_thorn_defeated(&mut self, player_id: PlayerId) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(player) = self.players.get_mut(&player_id) else {
            return;
        };
        for entry in &player.quests.entries {
            let Some(quest) = content.log_quest(entry.quest) else {
                continue;
            };
            for (index, objective) in quest.objectives.iter().enumerate() {
                if *objective == QuestObjective::DefeatThorn {
                    push_event(
                        &mut player.events,
                        ZoneEvent::QuestProgress {
                            quest: quest.id,
                            objective: u8::try_from(index).unwrap_or(u8::MAX),
                            count: 1,
                        },
                    );
                }
            }
        }
    }

    /// Tick step 8: talk and explore objectives of living players, in
    /// player-ID order, against the positions after this tick's movement.
    pub(crate) fn advance_quest_objectives(&mut self) -> Result<(), ZoneError> {
        let content = std::sync::Arc::clone(&self.content);
        if content.quests().is_empty() {
            return Ok(());
        }
        let ids: Vec<_> = self.players.keys().copied().collect();
        for player_id in ids {
            let Some(player) = self.players.get(&player_id) else {
                continue;
            };
            if !player.is_alive() {
                continue;
            }
            // Pending talk and explore objectives as (entry, objective), and retrieved items.
            let mut reached = Vec::new();
            let mut retrieved = Vec::new();
            for (slot, entry) in player.quests.entries.iter().enumerate() {
                let Some(quest) = content.quest(entry.quest) else {
                    continue;
                };
                for (index, objective) in quest.objectives.iter().enumerate() {
                    if let QuestObjective::Retrieve { item, area } = *objective {
                        let at = self.player_position(player_id)?;
                        let inside = content
                            .areas()
                            .area_at(at.x, at.z)
                            .is_some_and(|candidate| candidate.id() == area);
                        if inside
                            && player.inventory.count(item) == 0
                            && !retrieved.iter().any(|&(_, _, other)| other == item)
                            && self.player_water_mode(at).swims()
                        {
                            retrieved.push((quest.id, index, item));
                        }
                        continue;
                    }
                    if entry.progress[index] > 0 {
                        continue;
                    }
                    let done = match *objective {
                        QuestObjective::Talk { npc } => self.within_quest_reach(player_id, npc)?,
                        QuestObjective::Explore { area } => {
                            let at = self.player_position(player_id)?;
                            content
                                .areas()
                                .area_at(at.x, at.z)
                                .is_some_and(|candidate| candidate.id() == area)
                        }
                        QuestObjective::Kill { .. }
                        | QuestObjective::Collect { .. }
                        | QuestObjective::Retrieve { .. }
                        | QuestObjective::DefeatThorn
                        | QuestObjective::Inspect { .. }
                        | QuestObjective::UseItem { .. }
                        | QuestObjective::Escort { .. }
                        | QuestObjective::Interview { .. }
                        | QuestObjective::DefeatWithLoan { .. }
                        | QuestObjective::Crush { .. }
                        | QuestObjective::Demolish { .. }
                        | QuestObjective::DeliverLoad { .. }
                        | QuestObjective::Build { .. }
                        | QuestObjective::Defend { .. }
                        | QuestObjective::Aid { .. }
                        | QuestObjective::Transcribe { .. }
                        | QuestObjective::Unseal { .. }
                        | QuestObjective::Decide { .. }
                        | QuestObjective::Herd { .. }
                        | QuestObjective::Watch { .. }
                        | QuestObjective::Lay { .. }
                        | QuestObjective::EscortTo { .. }
                        | QuestObjective::Recruit { .. }
                        | QuestObjective::WorkInOrder { .. }
                        | QuestObjective::Sight { .. }
                        | QuestObjective::Ambush { .. }
                        | QuestObjective::Crossing { .. }
                        | QuestObjective::Outflank { .. } => false,
                    };
                    if done {
                        reached.push((slot, quest.id, index));
                    }
                }
            }
            if reached.is_empty() && retrieved.is_empty() {
                continue;
            }
            let Some(player) = self.players.get_mut(&player_id) else {
                continue;
            };
            // A full bag leaves the item at the altar until there is room.
            for (quest, index, item) in retrieved {
                let Some(revision) = player.inventory_revision.checked_add(1) else {
                    continue;
                };
                if player.inventory.insert(item, 1).is_err() {
                    continue;
                }
                player.inventory_revision = revision;
                push_event(
                    &mut player.events,
                    ZoneEvent::QuestProgress {
                        quest,
                        objective: u8::try_from(index).unwrap_or(u8::MAX),
                        count: 1,
                    },
                );
            }
            for (slot, quest, index) in reached {
                player.quests.entries[slot].progress[index] = 1;
                push_event(
                    &mut player.events,
                    ZoneEvent::QuestProgress {
                        quest,
                        objective: u8::try_from(index).unwrap_or(u8::MAX),
                        count: 1,
                    },
                );
            }
        }
        Ok(())
    }
}
