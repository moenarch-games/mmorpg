//! Quests (#25): immutable quest definitions bound to zone content, each
//! player's bounded quest log, and the zone-owned accept, abandon, progress
//! and turn-in rules with their per-player NPC markers.
//!
//! Objectives are kills of a creature template, items collected from drops,
//! talking to an NPC and exploring a named area. Kill, talk and explore
//! progress is stored in the log; collect progress is the bag's count of the
//! item, so selling or moving it changes progress and the turn-in consumes
//! it. Quest items drop only while a quest still needs them.

use std::{error::Error, fmt};

use crate::events::push_event;
use crate::{
    AreaId, CreatureTemplateId, EntityRef, ErrorCode, INVENTORY_SLOTS, Inventory, InventoryError,
    ItemId, NpcId, NpcRole, PlayerId, ZoneContent, ZoneError, ZoneEvent, ZoneSimulation,
    item_template,
};

/// Revision of the quest rules and the hosted Greyhaven chain.
pub const QUEST_CATALOG_REVISION: u64 = 1;
/// Quest IDs are `1..=MAX_QUESTS`, so completed quests fit a `u32` mask.
pub const MAX_QUESTS: usize = 32;
/// A player's quest log holds at most this many active quests.
pub const MAX_QUEST_LOG: usize = 10;
pub const MAX_QUEST_OBJECTIVES: usize = 3;
/// A turn-in offers at most this many items to choose one from.
pub const MAX_REWARD_CHOICES: usize = 4;
/// Distinct givers and enders of a content's quests.
pub const MAX_QUEST_NPCS: usize = 8;
pub const MAX_QUEST_NAME_BYTES: usize = 64;
pub const MAX_QUEST_TEXT_BYTES: usize = 240;
/// Inclusive horizontal (XZ) distance from an NPC's feet within which a
/// player accepts, turns in or talks (5 m).
pub const QUEST_REACH_UNITS: i32 = 500;
/// The most quest experience one turn-in grants.
pub const MAX_QUEST_EXPERIENCE: u32 = 10_000;

/// Stable quest identity within one content revision, `1..=MAX_QUESTS`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QuestId(u8);

impl QuestId {
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// The quest's bit in a completed mask; `None` outside `1..=MAX_QUESTS`.
    #[must_use]
    pub const fn bit(self) -> Option<u32> {
        if self.0 == 0 || self.0 as usize > MAX_QUESTS {
            None
        } else {
            Some(1 << (self.0 - 1))
        }
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
}

impl QuestObjective {
    /// The progress that completes the objective.
    #[must_use]
    pub const fn required(self) -> u8 {
        match self {
            Self::Kill { count, .. } | Self::Collect { count, .. } => count,
            Self::Talk { .. } | Self::Explore { .. } => 1,
        }
    }

    /// Wire and fingerprint code: 1 kill, 2 collect, 3 talk, 4 explore.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Kill { .. } => 1,
            Self::Collect { .. } => 2,
            Self::Talk { .. } => 3,
            Self::Explore { .. } => 4,
        }
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
    TooManyQuestNpcs,
}

impl fmt::Display for QuestContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::QuestCount => "zone content has one to 32 quests",
            Self::InvalidId => "a quest id must be 1..=32",
            Self::DuplicateId => "a quest id is used twice",
            Self::InvalidText => "a quest name or text is empty or too long",
            Self::InvalidNpc => "a quest giver or ender is no quest-giver npc",
            Self::InvalidPrerequisite => "a quest prerequisite must be an earlier quest",
            Self::InvalidObjective => "a quest objective names unknown content or no count",
            Self::InvalidRewards => "quest rewards are out of range",
            Self::TooManyQuestNpcs => "quests name more than eight npcs",
        })
    }
}

impl Error for QuestContentError {}

/// Validates and orders a content's quests by ID.
pub(crate) fn validate_quests(
    content: &ZoneContent,
    mut quests: Vec<Quest>,
) -> Result<Vec<Quest>, QuestContentError> {
    if quests.is_empty() || quests.len() > MAX_QUESTS {
        return Err(QuestContentError::QuestCount);
    }
    quests.sort_by_key(|quest| quest.id);
    if quests.iter().any(|quest| quest.id.bit().is_none()) {
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
    let mut quest_npcs = Vec::new();
    for quest in &quests {
        if !text(&quest.name, MAX_QUEST_NAME_BYTES) || !text(&quest.text, MAX_QUEST_TEXT_BYTES) {
            return Err(QuestContentError::InvalidText);
        }
        if !quest_giver(quest.giver) || !quest_giver(quest.ender) {
            return Err(QuestContentError::InvalidNpc);
        }
        quest_npcs.extend([quest.giver, quest.ender]);
        if let Some(prerequisite) = quest.prerequisite
            && (prerequisite >= quest.id
                || quests
                    .binary_search_by_key(&prerequisite, |quest| quest.id)
                    .is_err())
        {
            return Err(QuestContentError::InvalidPrerequisite);
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
                    let repeated = quest.objectives[..index].iter().any(|earlier| {
                        matches!(*earlier, QuestObjective::Collect { item: other, .. } if other == item)
                    });
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
    }
    quest_npcs.sort_unstable();
    quest_npcs.dedup();
    if quest_npcs.len() > MAX_QUEST_NPCS {
        return Err(QuestContentError::TooManyQuestNpcs);
    }
    Ok(quests)
}

/// One active quest and its stored progress per objective. Collect
/// objectives store zero: their progress is the bag's count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuestEntry {
    pub quest: QuestId,
    pub progress: [u8; MAX_QUEST_OBJECTIVES],
}

/// A player's quests: at most [`MAX_QUEST_LOG`] active entries in quest-ID
/// order and the mask of turned-in quests (bit `id - 1`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuestLog {
    pub entries: Vec<QuestEntry>,
    pub completed: u32,
}

impl QuestLog {
    #[must_use]
    pub fn entry(&self, quest: QuestId) -> Option<&QuestEntry> {
        self.entries.iter().find(|entry| entry.quest == quest)
    }

    #[must_use]
    pub fn is_completed(&self, quest: QuestId) -> bool {
        quest.bit().is_some_and(|bit| self.completed & bit != 0)
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
    pub completed: u32,
    pub entries: Vec<QuestEntry>,
    pub markers: Vec<NpcMarker>,
}

/// Current progress of every objective of `quest` for `entry`.
fn progress(quest: &Quest, entry: &QuestEntry, bag: &Inventory) -> [u8; MAX_QUEST_OBJECTIVES] {
    let mut current = [0; MAX_QUEST_OBJECTIVES];
    for (index, objective) in quest.objectives.iter().enumerate() {
        current[index] = match *objective {
            QuestObjective::Collect { item, count, .. } => {
                u8::try_from(bag.count(item).min(u32::from(count))).unwrap_or(count)
            }
            _ => entry.progress[index].min(objective.required()),
        };
    }
    current
}

fn objectives_done(quest: &Quest, entry: &QuestEntry, bag: &Inventory) -> bool {
    let current = progress(quest, entry, bag);
    quest
        .objectives
        .iter()
        .zip(current)
        .all(|(objective, count)| count >= objective.required())
}

/// The log, mask and markers one player sees.
pub(crate) fn quest_sheet(content: &ZoneContent, log: &QuestLog, bag: &Inventory) -> QuestSheet {
    let entries = log
        .entries
        .iter()
        .map(|entry| QuestEntry {
            quest: entry.quest,
            progress: content
                .quest(entry.quest)
                .map_or(entry.progress, |quest| progress(quest, entry, bag)),
        })
        .collect();
    QuestSheet {
        completed: log.completed,
        entries,
        markers: markers(content, log, bag),
    }
}

/// Markers of every quest giver and ender, in NPC-ID order: `?` for a
/// quest ready to turn in, else `!` for an available quest, else grey `?`
/// for one still in progress.
pub(crate) fn markers(content: &ZoneContent, log: &QuestLog, bag: &Inventory) -> Vec<NpcMarker> {
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
                        if objectives_done(quest, entry, bag) {
                            Some(QuestMarker::Complete)
                        } else {
                            Some(QuestMarker::InProgress)
                        }
                    }
                    None if quest.giver == npc && available(log, quest) => {
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

/// Neither active nor turned in, and its prerequisite was turned in.
fn available(log: &QuestLog, quest: &Quest) -> bool {
    log.entry(quest.id).is_none()
        && !log.is_completed(quest.id)
        && quest
            .prerequisite
            .is_none_or(|prerequisite| log.is_completed(prerequisite))
}

/// Recovery checks of a log against its content: ordered unique active
/// quests of this content that are available apart from being active,
/// progress within each objective's count (zero for collect objectives),
/// and turned-in quests whose prerequisites were turned in.
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
    let mut known = 0_u32;
    for quest in content.quests() {
        known |= quest.id.bit().unwrap_or(0);
        if log.is_completed(quest.id)
            && quest
                .prerequisite
                .is_some_and(|prerequisite| !log.is_completed(prerequisite))
        {
            return invalid();
        }
    }
    if log.completed & !known != 0 {
        return invalid();
    }
    for entry in &log.entries {
        let Some(quest) = content.quest(entry.quest) else {
            return invalid();
        };
        if log.is_completed(quest.id)
            || quest
                .prerequisite
                .is_some_and(|prerequisite| !log.is_completed(prerequisite))
        {
            return invalid();
        }
        for (index, &stored) in entry.progress.iter().enumerate() {
            let limit = match quest.objectives.get(index) {
                Some(QuestObjective::Collect { .. }) | None => 0,
                Some(objective) => objective.required(),
            };
            if stored > limit {
                return invalid();
            }
        }
    }
    Ok(())
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

/// A quest request at an NPC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuestRequest {
    Accept { npc: NpcId, quest: u8 },
    Complete { npc: NpcId, quest: u8, choice: u8 },
    Abandon { quest: u8 },
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
        now: u64,
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
                player.quests_changed_at = now;
                Ok(None)
            }
            QuestRequest::Accept { npc, quest } => self.accept_quest(player_id, npc, quest, now),
            QuestRequest::Complete { npc, quest, choice } => {
                self.complete_quest(player_id, npc, quest, choice, now)
            }
        }
    }

    /// Whether the player's feet are within quest reach of `npc`'s.
    fn within_quest_reach(&self, player_id: PlayerId, npc: NpcId) -> Result<bool, ZoneError> {
        let Some(feet) = self.content.npc(npc).map(|npc| npc.position) else {
            return Ok(false);
        };
        let from = self.player_position(player_id)?;
        // i128 keeps the squared distance exact for any recovered i32 position.
        let [dx, dz] = [
            i128::from(from.x) - i128::from(feet[0]),
            i128::from(from.z) - i128::from(feet[1]),
        ];
        Ok(dx * dx + dz * dz <= i128::from(QUEST_REACH_UNITS).pow(2))
    }

    fn accept_quest(
        &mut self,
        player_id: PlayerId,
        npc: NpcId,
        quest: u8,
        now: u64,
    ) -> Result<Refusal, ZoneError> {
        let target = Some(EntityRef::Npc(npc));
        let Some(definition) = self
            .content
            .quest(QuestId::new(quest))
            .filter(|definition| definition.giver == npc)
        else {
            return Ok(Some((ErrorCode::InvalidQuest, target)));
        };
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
        player.quests_changed_at = now;
        Ok(None)
    }

    fn complete_quest(
        &mut self,
        player_id: PlayerId,
        npc: NpcId,
        quest: u8,
        choice: u8,
        now: u64,
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
        if !objectives_done(definition, &entry, &player.inventory) {
            return Ok(Some((ErrorCode::QuestIncomplete, target)));
        }
        // Stage bag and copper; any refusal leaves both and the log intact.
        let mut bag = player.inventory.clone();
        for objective in &definition.objectives {
            if let QuestObjective::Collect { item, count, .. } = *objective
                && bag.remove_item(item, u16::from(count)).is_err()
            {
                return Ok(Some((ErrorCode::QuestIncomplete, target)));
            }
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
        let Some(copper) = player.copper.checked_add(definition.rewards.copper) else {
            return Ok(Some((ErrorCode::MoneyOverflow, target)));
        };
        let revision = if bag == player.inventory {
            None
        } else {
            match player.inventory_revision.checked_add(1) {
                Some(revision) => Some(revision),
                None => return Ok(Some((ErrorCode::InvalidInventoryMove, target))),
            }
        };
        let Some(bit) = definition.id.bit() else {
            return Ok(Some((ErrorCode::InvalidQuest, target)));
        };
        player.inventory = bag;
        player.copper = copper;
        if let Some(revision) = revision {
            player.inventory_revision = revision;
            player.inventory_changed_at = now;
        }
        player
            .quests
            .entries
            .retain(|candidate| candidate.quest != definition.id);
        player.quests.completed |= bit;
        player.quests_changed_at = now;
        push_event(
            &mut player.events,
            ZoneEvent::QuestCompleted {
                quest: definition.id,
            },
        );
        self.grant_experience(player_id, definition.rewards.experience)?;
        Ok(None)
    }

    /// The tapper's kill credit: every active kill objective of the
    /// template that is not yet complete advances by one.
    pub(crate) fn credit_kill(
        &mut self,
        player_id: PlayerId,
        template: CreatureTemplateId,
        now: u64,
    ) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(player) = self.players.get_mut(&player_id) else {
            return;
        };
        let mut changed = false;
        for entry in &mut player.quests.entries {
            let Some(quest) = content.quest(entry.quest) else {
                continue;
            };
            for (index, objective) in quest.objectives.iter().enumerate() {
                if let QuestObjective::Kill {
                    template: wanted,
                    count,
                } = *objective
                    && wanted == template
                    && entry.progress[index] < count
                {
                    entry.progress[index] += 1;
                    changed = true;
                    push_event(
                        &mut player.events,
                        ZoneEvent::QuestProgress {
                            quest: quest.id,
                            objective: u8::try_from(index).unwrap_or(u8::MAX),
                            count: entry.progress[index],
                        },
                    );
                }
            }
        }
        if changed {
            player.quests_changed_at = now;
        }
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

    /// Tick step 8: talk and explore objectives of living players, in
    /// player-ID order, against the positions after this tick's movement.
    pub(crate) fn advance_quest_objectives(&mut self, now: u64) -> Result<(), ZoneError> {
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
            // Pending talk and explore objectives as (entry, objective).
            let mut reached = Vec::new();
            for (slot, entry) in player.quests.entries.iter().enumerate() {
                let Some(quest) = content.quest(entry.quest) else {
                    continue;
                };
                for (index, objective) in quest.objectives.iter().enumerate() {
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
                        QuestObjective::Kill { .. } | QuestObjective::Collect { .. } => false,
                    };
                    if done {
                        reached.push((slot, quest.id, index));
                    }
                }
            }
            if reached.is_empty() {
                continue;
            }
            let Some(player) = self.players.get_mut(&player_id) else {
                continue;
            };
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
            player.quests_changed_at = now;
        }
        Ok(())
    }
}
