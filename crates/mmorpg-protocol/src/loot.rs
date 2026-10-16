//! Bounded copper/item rewards shared by canonical corpse state and self loot sheets.

use mmorpg_core::{CreatureId, ItemId, ItemStack, LootClaim, LootRewards};

use crate::{
    ProtocolError,
    wire::{read_bool, take},
};

/// Claim, copper, then the item and the quest item, each a presence byte
/// and an optional stack.
pub(crate) const MAX_LOOT_SHEET_BYTES: usize = 12 + 4 + 2 * (1 + 4);

pub(crate) fn encode_claim(payload: &mut Vec<u8>, claim: LootClaim) {
    payload.extend_from_slice(&claim.creature.get().to_be_bytes());
    payload.extend_from_slice(&claim.died_at.to_be_bytes());
}

pub(crate) fn decode_claim(payload: &[u8], offset: &mut usize) -> Result<LootClaim, ProtocolError> {
    Ok(LootClaim {
        creature: CreatureId::new(u32::from_be_bytes(take(payload, offset)?)),
        died_at: u64::from_be_bytes(take(payload, offset)?),
    })
}

pub(crate) fn encode_rewards(payload: &mut Vec<u8>, rewards: LootRewards) {
    payload.extend_from_slice(&rewards.money.to_be_bytes());
    for stack in [rewards.item, rewards.quest_item] {
        payload.push(u8::from(stack.is_some()));
        if let Some(stack) = stack {
            payload.extend_from_slice(&stack.item().get().to_be_bytes());
            payload.extend_from_slice(&stack.quantity().to_be_bytes());
        }
    }
}

fn decode_stack(payload: &[u8], offset: &mut usize) -> Result<Option<ItemStack>, ProtocolError> {
    if !read_bool(payload, offset)? {
        return Ok(None);
    }
    let id = ItemId::new(u16::from_be_bytes(take(payload, offset)?));
    let quantity = u16::from_be_bytes(take(payload, offset)?);
    ItemStack::new(id, quantity)
        .map(Some)
        .map_err(|error| ProtocolError::new(error.to_string()))
}

pub(crate) fn decode_rewards(
    payload: &[u8],
    offset: &mut usize,
) -> Result<LootRewards, ProtocolError> {
    let money = u32::from_be_bytes(take(payload, offset)?);
    let item = decode_stack(payload, offset)?;
    let quest_item = decode_stack(payload, offset)?;
    Ok(LootRewards {
        money,
        item,
        quest_item,
    })
}
