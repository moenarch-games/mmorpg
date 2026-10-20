//! Canonical reward receipt encoding. These IDs never enter player projections.
use crate::{
    ProtocolError,
    wire::{read_bool, read_u8, take},
};
use mmorpg_core::{
    CreatureId, ItemId, MAX_PLAYERS_PER_ZONE, QuestId, RewardGrant, RewardHistory, RewardKey,
};
use std::collections::BTreeSet;

/// Version byte plus the largest tagged receipt (kill objective or quest item).
pub const MAX_REWARD_RECEIPT_BYTES: usize = 16;

fn error(cause: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(cause.to_string())
}
fn count(payload: &mut Vec<u8>, value: usize) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&u16::try_from(value).map_err(error)?.to_be_bytes());
    Ok(())
}
fn read_count(payload: &[u8], offset: &mut usize, maximum: usize) -> Result<usize, ProtocolError> {
    let value = usize::from(u16::from_be_bytes(take(payload, offset)?));
    if value > maximum {
        return Err(error("reward receipt count exceeds limit"));
    }
    Ok(value)
}
pub(crate) fn encode_key(payload: &mut Vec<u8>, key: RewardKey) -> Result<(), ProtocolError> {
    key.validate().map_err(error)?;
    let code = match key {
        RewardKey::KillExperience { .. } => 1,
        RewardKey::KillObjective { .. } => 2,
        RewardKey::LootMoney { .. } => 3,
        RewardKey::LootItem { .. } => 4,
        RewardKey::LootQuestItem { .. } => 5,
        RewardKey::QuestCompletion { .. } => 6,
    };
    payload.push(code);
    if let Some((creature, life)) = key.creature_life() {
        payload.extend_from_slice(&creature.get().to_be_bytes());
        payload.extend_from_slice(&life.to_be_bytes());
    }
    match key {
        RewardKey::KillObjective {
            quest, objective, ..
        } => payload.extend_from_slice(&[quest.get(), objective]),
        RewardKey::LootQuestItem { item, .. } => {
            payload.extend_from_slice(&item.get().to_be_bytes())
        }
        RewardKey::QuestCompletion { quest } => payload.push(quest.get()),
        _ => {}
    }
    Ok(())
}
pub(crate) fn decode_key(payload: &[u8], offset: &mut usize) -> Result<RewardKey, ProtocolError> {
    let code = read_u8(payload, offset)?;
    let key = if code == 6 {
        RewardKey::QuestCompletion {
            quest: QuestId::new(read_u8(payload, offset)?),
        }
    } else {
        if !(1..=5).contains(&code) {
            return Err(error("unknown reward key tag"));
        }
        let creature = CreatureId::new(u32::from_be_bytes(take(payload, offset)?));
        let life = u64::from_be_bytes(take(payload, offset)?);
        match code {
            1 => RewardKey::KillExperience { creature, life },
            2 => RewardKey::KillObjective {
                creature,
                life,
                quest: QuestId::new(read_u8(payload, offset)?),
                objective: read_u8(payload, offset)?,
            },
            3 => RewardKey::LootMoney { creature, life },
            4 => RewardKey::LootItem { creature, life },
            5 => RewardKey::LootQuestItem {
                creature,
                life,
                item: ItemId::new(u16::from_be_bytes(take(payload, offset)?)),
            },
            _ => return Err(error("unknown reward key tag")),
        }
    };
    key.validate().map_err(error)?;
    Ok(key)
}
fn encode_keys(payload: &mut Vec<u8>, keys: &BTreeSet<RewardKey>) -> Result<(), ProtocolError> {
    count(payload, keys.len())?;
    for key in keys {
        encode_key(payload, *key)?;
    }
    Ok(())
}
fn decode_keys(
    payload: &[u8],
    offset: &mut usize,
    maximum: usize,
) -> Result<BTreeSet<RewardKey>, ProtocolError> {
    let count = read_count(payload, offset, maximum)?;
    let mut keys = BTreeSet::new();
    for _ in 0..count {
        let key = decode_key(payload, offset)?;
        if keys.last().is_some_and(|previous| *previous >= key) {
            return Err(error("reward keys must be strictly ordered"));
        }
        keys.insert(key);
    }
    Ok(keys)
}
pub(crate) fn encode_history(
    payload: &mut Vec<u8>,
    history: Option<&RewardHistory>,
) -> Result<(), ProtocolError> {
    payload.push(u8::from(history.is_some()));
    let Some(history) = history else {
        return Ok(());
    };
    history.validate().map_err(error)?;
    payload.extend_from_slice(&history.max_known_ids.to_be_bytes());
    payload.extend_from_slice(&history.max_tick_grants.to_be_bytes());
    encode_keys(payload, &history.world)?;
    count(payload, history.players.len())?;
    for (player, keys) in &history.players {
        payload.extend_from_slice(&player.to_be_bytes());
        encode_keys(payload, keys)?;
    }
    count(payload, history.grants.len())?;
    for grant in &history.grants {
        payload.extend_from_slice(&grant.player.to_be_bytes());
        encode_key(payload, grant.key)?;
    }
    Ok(())
}
pub(crate) fn decode_history(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<RewardHistory>, ProtocolError> {
    if !read_bool(payload, offset)? {
        return Ok(None);
    }
    let mut history = RewardHistory::new(
        u16::from_be_bytes(take(payload, offset)?),
        u16::from_be_bytes(take(payload, offset)?),
    )
    .map_err(error)?;
    history.world = decode_keys(payload, offset, usize::from(history.max_known_ids))?;
    let players = read_count(payload, offset, MAX_PLAYERS_PER_ZONE)?;
    for _ in 0..players {
        let player = u32::from_be_bytes(take(payload, offset)?);
        if player == 0
            || history
                .players
                .last_key_value()
                .is_some_and(|(previous, _)| *previous >= player)
        {
            return Err(error("reward players must be strictly ordered"));
        }
        let keys = decode_keys(
            payload,
            offset,
            usize::from(history.max_known_ids).saturating_sub(history.known_count()),
        )?;
        history.players.insert(player, keys);
    }
    let grants = read_count(payload, offset, usize::from(history.max_tick_grants))?;
    for _ in 0..grants {
        let grant = RewardGrant {
            player: u32::from_be_bytes(take(payload, offset)?),
            key: decode_key(payload, offset)?,
        };
        if history
            .grants
            .last()
            .is_some_and(|previous| *previous >= grant)
        {
            return Err(error("tick rewards must be strictly ordered"));
        }
        history.grants.insert(grant);
    }
    history.validate().map_err(error)?;
    Ok(Some(history))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_reward_history_round_trips_every_entitlement_kind() {
        let creature = CreatureId::new(3);
        let life = 7;
        let quest = QuestId::new(1);
        let mut history = RewardHistory::new(32, 16).unwrap();
        for key in [
            RewardKey::KillExperience { creature, life },
            RewardKey::KillObjective {
                creature,
                life,
                quest,
                objective: 0,
            },
            RewardKey::LootMoney { creature, life },
            RewardKey::LootItem { creature, life },
            RewardKey::LootQuestItem {
                creature,
                life,
                item: ItemId::new(1),
            },
            RewardKey::QuestCompletion { quest },
        ] {
            history.record(1, key).unwrap();
        }
        let mut bytes = Vec::new();
        encode_history(&mut bytes, Some(&history)).unwrap();
        let mut offset = 0;
        assert_eq!(decode_history(&bytes, &mut offset).unwrap(), Some(history));
        assert_eq!(offset, bytes.len());
        assert!(decode_history(&bytes[..bytes.len() - 1], &mut 0).is_err());
    }
}

/// Versioned receipt metadata used by the PostgreSQL adapter and trusted inputs.
pub fn encode_reward_receipt(key: RewardKey) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = vec![1];
    encode_key(&mut bytes, key)?;
    Ok(bytes)
}
pub fn decode_reward_receipt(bytes: &[u8]) -> Result<RewardKey, ProtocolError> {
    if bytes.first() != Some(&1) {
        return Err(error("unsupported reward receipt version"));
    }
    let mut offset = 1;
    let key = decode_key(bytes, &mut offset)?;
    crate::wire::ensure_fully_consumed(bytes, offset)?;
    Ok(key)
}
