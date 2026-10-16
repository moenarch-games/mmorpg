//! Quest records (#25): the canonical log and the self sheet's quest part.
//! Entries are a quest ID and one progress byte per objective slot; the
//! sheet adds the viewer's NPC markers. Whether content defines a quest is
//! for core (recovery) and clients (catalog) to decide.

use mmorpg_core::{
    MAX_QUEST_LOG, MAX_QUEST_NPCS, MAX_QUEST_OBJECTIVES, NpcId, NpcMarker, QuestEntry, QuestLog,
    QuestMarker, QuestSheet,
};

use crate::ProtocolError;
use crate::wire::{decode_quest, decode_u8_count, encode_u8_count, read_u8, take};

/// Quest ID and one progress byte per objective slot.
pub(crate) const QUEST_ENTRY_BYTES: usize = 1 + MAX_QUEST_OBJECTIVES;
/// NPC ID (`u16`) and marker code.
pub(crate) const MARKER_BYTES: usize = 2 + 1;
/// Completed mask, entry count, entries, marker count and markers.
pub(crate) const MAX_QUEST_SHEET_BYTES: usize =
    4 + 1 + MAX_QUEST_LOG * QUEST_ENTRY_BYTES + 1 + MAX_QUEST_NPCS * MARKER_BYTES;

fn encode_entries(
    payload: &mut Vec<u8>,
    completed: u32,
    entries: &[QuestEntry],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&completed.to_be_bytes());
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
fn decode_entries(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(u32, Vec<QuestEntry>), ProtocolError> {
    let completed = u32::from_be_bytes(take(payload, offset)?);
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
    Ok((completed, entries))
}

/// Canonical: change tick, completed mask and entries.
pub(crate) fn encode_log(
    payload: &mut Vec<u8>,
    log: &QuestLog,
    changed_at: u64,
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&changed_at.to_be_bytes());
    encode_entries(payload, log.completed, &log.entries)
}

pub(crate) fn decode_log(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(QuestLog, u64), ProtocolError> {
    let changed_at = u64::from_be_bytes(take(payload, offset)?);
    let (completed, entries) = decode_entries(payload, offset)?;
    Ok((QuestLog { entries, completed }, changed_at))
}

pub(crate) fn encode_sheet(payload: &mut Vec<u8>, sheet: &QuestSheet) -> Result<(), ProtocolError> {
    encode_entries(payload, sheet.completed, &sheet.entries)?;
    payload.push(encode_u8_count(
        sheet.markers.len(),
        MAX_QUEST_NPCS,
        "quest sheet has too many markers",
    )?);
    for marker in &sheet.markers {
        let npc = u16::try_from(marker.npc.get())
            .map_err(|_| ProtocolError::new("a marked npc id must fit u16"))?;
        payload.extend_from_slice(&npc.to_be_bytes());
        payload.push(marker.marker.code());
    }
    Ok(())
}

/// A sheet: no active quest is also turned in, and markers name distinct
/// NPCs in ascending order with known codes.
pub(crate) fn decode_sheet(
    payload: &[u8],
    offset: &mut usize,
) -> Result<QuestSheet, ProtocolError> {
    let (completed, entries) = decode_entries(payload, offset)?;
    if entries
        .iter()
        .any(|entry| entry.quest.bit().is_some_and(|bit| completed & bit != 0))
    {
        return Err(ProtocolError::new("an active quest is also turned in"));
    }
    let count = decode_u8_count(
        payload,
        offset,
        MAX_QUEST_NPCS,
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
