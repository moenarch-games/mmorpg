//! Quest evidence (#1019): authored things in the world a player inspects for a quest, such
//! as the cut sacks by the Wolfrun cart or a ledger on a tanner's strongbox.
//!
//! An [`EvidenceSpot`] is zone content with a stable ID; the world dressing draws its art at
//! the same spot, so both clients show it. A quest's [`crate::QuestObjective::Inspect`] names
//! one. The player inspects a spot with [`crate::ZoneCommand::Inspect`]: the zone checks in the
//! tick that the player lives, has the weapon sheathed, stands within the spot's reach and has
//! an active quest that still needs it. Then every such objective in the player's log is done,
//! once; the log stores that progress, so snapshots, replay, recovery and saved characters
//! carry it like a talk objective's. A spot no quest needs is refused, not credited, so an
//! inspection made before a quest was accepted never counts and can simply be repeated. A lure's
//! stake no objective names casts a quest's supplied bait there instead ([`crate::lure`]).

use crate::events::push_event;
use crate::{EntityRef, ErrorCode, PlayerId, QuestObjective, ZoneError, ZoneEvent, ZoneSimulation};

/// A player's feet inspect a spot only this close to its height: 2 m, so a prop on the
/// ground or on a low table is in reach and one on a ledge above is not.
pub const EVIDENCE_HEIGHT_UNITS: i32 = 200;

/// The widest reach content may give a spot: 5 m, as far as a quest giver's.
pub const MAX_EVIDENCE_REACH_UNITS: i32 = crate::QUEST_REACH_UNITS;

/// Stable evidence identity within one content revision; `0` names none.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EvidenceId(u32);

impl EvidenceId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// One authored piece of quest evidence (#1019).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSpot {
    pub id: EvidenceId,
    /// What clients call it, as in "Inspect the cut sacks".
    pub name: String,
    /// The spot: X, height and Z in units, where its art stands.
    pub at: [i32; 3],
    /// How near on XZ a player's feet must come, in units.
    pub reach: i32,
    /// A hidden spot's landmark directions (#1019), as in "at the old tower's foot, by its
    /// sunset corner": no client marks it on the map or dresses it, and the quest log reads the
    /// directions instead. `None` for a spot in plain sight. The zone treats both alike.
    pub directions: Option<String>,
}

/// A hidden spot's directions are at most this long.
pub const MAX_EVIDENCE_DIRECTIONS_BYTES: usize = 96;

impl EvidenceSpot {
    /// Whether it is hidden (#1019): found by its directions, not by a map hint.
    #[must_use]
    pub const fn hidden(&self) -> bool {
        self.directions.is_some()
    }

    /// Whether feet at `feet` stand within reach of the spot.
    #[must_use]
    pub fn within(&self, feet: [i32; 3]) -> bool {
        let [x, y, z] = self.at;
        let (dx, dz) = (i64::from(feet[0] - x), i64::from(feet[2] - z));
        dx * dx + dz * dz <= i64::from(self.reach).pow(2)
            && (feet[1] - y).abs() <= EVIDENCE_HEIGHT_UNITS
    }
}

impl ZoneSimulation {
    /// The `Inspect` intent (#1019): completes every open inspect objective of `spot` in the
    /// player's log. An unknown spot is an invalid target, one out of reach is out of range,
    /// and one no active quest still needs is an invalid quest; refusals change nothing.
    pub(crate) fn inspect(
        &mut self,
        player_id: PlayerId,
        spot: u32,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let content = std::sync::Arc::clone(&self.content);
        let spot = EvidenceId::new(spot);
        let Some(site) = content.evidence_spot(spot) else {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        };
        let centre = self.player_position(player_id)?;
        let feet = [
            centre.x,
            centre.y - crate::PLAYER_HALF_EXTENTS_UNITS[1],
            centre.z,
        ];
        if !site.within(feet) {
            return Ok(Some((ErrorCode::OutOfRange, None)));
        }
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("inspecting player is absent"))?;
        // A transcription (#1019) takes its item while the bag holds none; a full bag refuses
        // before anything else changes.
        let mut taken = Vec::new();
        for entry in &player.quests.entries {
            let Some(quest) = content.quest(entry.quest) else {
                continue;
            };
            for (index, objective) in quest.objectives.iter().enumerate() {
                if let QuestObjective::Transcribe { spot: wanted, item } = *objective
                    && wanted == spot
                    && player.inventory.count(item) == 0
                    && !taken.iter().any(|&(_, _, other)| other == item)
                {
                    taken.push((quest.id, index, item));
                }
            }
        }
        if !taken.is_empty() {
            let mut bag = player.inventory.clone();
            if taken
                .iter()
                .any(|&(_, _, item)| bag.insert(item, 1).is_err())
            {
                return Ok(Some((ErrorCode::InventoryFull, None)));
            }
            let Some(revision) = player.inventory_revision.checked_add(1) else {
                return Ok(Some((ErrorCode::InvalidInventoryMove, None)));
            };
            player.inventory = bag;
            player.inventory_revision = revision;
            for &(quest, index, _) in &taken {
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
        // Bait or a snare laid at the spot (#1019) takes its item from the bag; without one it is
        // refused before anything else changes.
        let mut laid = Vec::new();
        for (slot, entry) in player.quests.entries.iter().enumerate() {
            let Some(quest) = content.quest(entry.quest) else {
                continue;
            };
            for (index, objective) in quest.objectives.iter().enumerate() {
                if let QuestObjective::Lay { spot: wanted, item } = *objective
                    && wanted == spot
                    && entry.progress[index] == 0
                {
                    laid.push((slot, index, item));
                }
            }
        }
        if !laid.is_empty() {
            let mut bag = player.inventory.clone();
            if laid
                .iter()
                .any(|&(_, _, item)| bag.remove_item(item, 1).is_err())
            {
                return Ok(Some((ErrorCode::MissingMaterials, None)));
            }
            let Some(revision) = player.inventory_revision.checked_add(1) else {
                return Ok(Some((ErrorCode::InvalidInventoryMove, None)));
            };
            player.inventory = bag;
            player.inventory_revision = revision;
            for &(slot, index, _) in &laid {
                let entry = &mut player.quests.entries[slot];
                entry.progress[index] = 1;
                push_event(
                    &mut player.events,
                    ZoneEvent::QuestProgress {
                        quest: entry.quest,
                        objective: u8::try_from(index).unwrap_or(u8::MAX),
                        count: 1,
                    },
                );
            }
        }
        let mut credited = !taken.is_empty() || !laid.is_empty();
        for entry in &mut player.quests.entries {
            let Some(quest) = content.quest(entry.quest) else {
                continue;
            };
            for (index, objective) in quest.objectives.iter().enumerate() {
                if *objective == (QuestObjective::Inspect { spot }) && entry.progress[index] == 0 {
                    entry.progress[index] = 1;
                    credited = true;
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
        if credited {
            return Ok(None);
        }
        // A lure spot (#1019) no objective needs casts the supplied bait there.
        if let Some(lure) = content.lure(spot) {
            return Ok(self.cast_lure(player_id, lure));
        }
        Ok(Some((ErrorCode::InvalidQuest, None)))
    }
}
