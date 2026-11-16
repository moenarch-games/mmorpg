//! Quest records (#25): the canonical log and the self sheet's quest part.
//! The turned-in quests are a count and their IDs in ascending order, as many as content
//! defines (#862); entries are a quest ID and one progress byte per objective slot; the
//! sheet adds the viewer's NPC markers, as many as the packet budget holds (see
//! [`crate::sheet::MAX_QUEST_SHEET_BYTES`]). Whether content defines a quest is for core (recovery)
//! and clients (catalog) to decide.

use std::collections::BTreeSet;

use mmorpg_core::{
    MAX_QUEST_LOG, MAX_QUEST_OBJECTIVES, NpcId, NpcMarker, QuestEntry, QuestId, QuestLog,
    QuestMarker, QuestSheet,
};

use crate::ProtocolError;
use crate::wire::{decode_quest, decode_u8_count, encode_u8_count, read_u8, take};

/// Quest ID and one progress byte per objective slot.
pub(crate) const QUEST_ENTRY_BYTES: usize = 1 + MAX_QUEST_OBJECTIVES;

/// Turned-in quests: `count u8 · count × quest u8`, ascending. Quest IDs are one byte and
/// never `0`, so every set fits.
fn encode_completed(
    payload: &mut Vec<u8>,
    completed: &BTreeSet<QuestId>,
) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        completed.len(),
        usize::from(u8::MAX),
        "too many quests are turned in",
    )?);
    for quest in completed {
        if quest.get() == 0 {
            return Err(ProtocolError::new("quest id is out of range"));
        }
        payload.push(quest.get());
    }
    Ok(())
}

/// Distinct in-range IDs in strictly ascending order.
fn decode_completed(
    payload: &[u8],
    offset: &mut usize,
) -> Result<BTreeSet<QuestId>, ProtocolError> {
    let count = decode_u8_count(
        payload,
        offset,
        usize::from(u8::MAX),
        "too many quests are turned in",
    )?;
    let mut completed = BTreeSet::new();
    for _ in 0..count {
        let quest = decode_quest(read_u8(payload, offset)?)?;
        if completed.last().is_some_and(|last| *last >= quest) {
            return Err(ProtocolError::new("turned-in quests are not ordered"));
        }
        completed.insert(quest);
    }
    Ok(completed)
}

/// The `u32` mask character records stored before #862 (bit `id - 1`).
pub(crate) fn decode_completed_mask(
    payload: &[u8],
    offset: &mut usize,
) -> Result<BTreeSet<QuestId>, ProtocolError> {
    let mask = u32::from_be_bytes(take(payload, offset)?);
    Ok((1..=32_u8)
        .filter(|id| mask & (1 << (id - 1)) != 0)
        .map(QuestId::new)
        .collect())
}

fn encode_entries(payload: &mut Vec<u8>, entries: &[QuestEntry]) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        entries.len(),
        MAX_QUEST_LOG,
        "quest log exceeds its capacity",
    )?);
    for entry in entries {
        payload.push(entry.quest.get());
        payload.extend_from_slice(&entry.progress);
    }
    Ok(())
}

/// Entries with in-range IDs in strictly ascending order.
pub(crate) fn decode_entries(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<QuestEntry>, ProtocolError> {
    let count = decode_u8_count(
        payload,
        offset,
        MAX_QUEST_LOG,
        "quest log exceeds its capacity",
    )?;
    let mut entries: Vec<QuestEntry> = Vec::with_capacity(count);
    for _ in 0..count {
        let quest = decode_quest(read_u8(payload, offset)?)?;
        let progress = take(payload, offset)?;
        if entries.last().is_some_and(|last| last.quest >= quest) {
            return Err(ProtocolError::new("quest log entries are not ordered"));
        }
        entries.push(QuestEntry { quest, progress });
    }
    Ok(entries)
}

/// Canonical and durable: turned-in quests, then entries.
pub(crate) fn encode_log(payload: &mut Vec<u8>, log: &QuestLog) -> Result<(), ProtocolError> {
    encode_completed(payload, &log.completed)?;
    encode_entries(payload, &log.entries)
}

pub(crate) fn decode_log(payload: &[u8], offset: &mut usize) -> Result<QuestLog, ProtocolError> {
    let completed = decode_completed(payload, offset)?;
    let entries = decode_entries(payload, offset)?;
    Ok(QuestLog { entries, completed })
}

/// Fails when the sheet outgrows [`crate::sheet::MAX_QUEST_SHEET_BYTES`], what the packet
/// budget leaves a quiet tick.
pub(crate) fn encode_sheet(payload: &mut Vec<u8>, sheet: &QuestSheet) -> Result<(), ProtocolError> {
    let start = payload.len();
    encode_completed(payload, &sheet.completed)?;
    encode_entries(payload, &sheet.entries)?;
    payload.push(encode_u8_count(
        sheet.markers.len(),
        usize::from(u8::MAX),
        "quest sheet has too many markers",
    )?);
    for marker in &sheet.markers {
        let npc = u16::try_from(marker.npc.get())
            .map_err(|_| ProtocolError::new("a marked npc id must fit u16"))?;
        payload.extend_from_slice(&npc.to_be_bytes());
        payload.push(marker.marker.code());
    }
    if payload.len() - start > crate::sheet::MAX_QUEST_SHEET_BYTES {
        return Err(ProtocolError::new("quest sheet exceeds its packet budget"));
    }
    Ok(())
}

/// A sheet: no active quest is also turned in, and markers name distinct
/// NPCs in ascending order with known codes.
pub(crate) fn decode_sheet(
    payload: &[u8],
    offset: &mut usize,
) -> Result<QuestSheet, ProtocolError> {
    let completed = decode_completed(payload, offset)?;
    let entries = decode_entries(payload, offset)?;
    if entries.iter().any(|entry| completed.contains(&entry.quest)) {
        return Err(ProtocolError::new("an active quest is also turned in"));
    }
    let count = decode_u8_count(
        payload,
        offset,
        usize::from(u8::MAX),
        "quest sheet has too many markers",
    )?;
    let mut markers: Vec<NpcMarker> = Vec::with_capacity(count);
    for _ in 0..count {
        let npc = NpcId::new(u32::from(u16::from_be_bytes(take(payload, offset)?)));
        let marker = QuestMarker::from_code(read_u8(payload, offset)?)
            .ok_or_else(|| ProtocolError::new("unknown quest marker"))?;
        if markers.last().is_some_and(|last| last.npc >= npc) {
            return Err(ProtocolError::new("quest markers are not ordered"));
        }
        markers.push(NpcMarker { npc, marker });
    }
    Ok(QuestSheet {
        completed,
        entries,
        markers,
    })
}

#[cfg(test)]
mod tests {
    use mmorpg_core::{NpcMarker, QuestMarker, greyhaven_vale};

    use super::*;

    /// Every Greyhaven quest turned in, a full log and every quest NPC marked: the sheet still
    /// fits the packet budget, and a turned-in set reads back as written.
    #[test]
    fn greyhavens_fullest_quest_sheet_fits_its_budget() {
        let quests = greyhaven_vale::quests::quests();
        let mut npcs: Vec<NpcId> = quests
            .iter()
            .flat_map(|quest| [quest.giver, quest.ender])
            .collect();
        npcs.sort_unstable();
        npcs.dedup();
        let sheet = QuestSheet {
            completed: quests.iter().map(|quest| quest.id).collect(),
            entries: (1..=MAX_QUEST_LOG)
                .map(|quest| QuestEntry {
                    quest: QuestId::new(u8::try_from(quest).unwrap()),
                    progress: [u8::MAX; MAX_QUEST_OBJECTIVES],
                })
                .collect(),
            markers: npcs
                .into_iter()
                .map(|npc| NpcMarker {
                    npc,
                    marker: QuestMarker::Complete,
                })
                .collect(),
        };
        let mut payload = Vec::new();
        encode_sheet(&mut payload, &sheet).unwrap();
        assert!(payload.len() <= crate::sheet::MAX_QUEST_SHEET_BYTES);

        let log = QuestLog {
            entries: Vec::new(),
            completed: sheet.completed,
        };
        let mut payload = Vec::new();
        encode_log(&mut payload, &log).unwrap();
        assert_eq!(decode_log(&payload, &mut 0).unwrap(), log);
    }
}
