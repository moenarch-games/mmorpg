//! Fixed ordered self bag encoding. Empty slots are exactly (0, 0);
//! occupied slots must pass the core catalog's stack validation.

use mmorpg_core::{EQUIPMENT_SLOTS, Equipment, INVENTORY_SLOTS, Inventory, ItemId, ItemStack};

use crate::{ProtocolError, wire::take};

pub const INVENTORY_RECORD_BYTES: usize = INVENTORY_SLOTS * 4;

pub(crate) fn encode_inventory(payload: &mut Vec<u8>, inventory: &Inventory) {
    for slot in inventory.slots() {
        let (item, quantity) = slot.map_or((0, 0), |stack| (stack.item().get(), stack.quantity()));
        payload.extend_from_slice(&item.to_be_bytes());
        payload.extend_from_slice(&quantity.to_be_bytes());
    }
}

pub(crate) fn decode_inventory(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Inventory, ProtocolError> {
    let mut slots = [None; INVENTORY_SLOTS];
    for slot in &mut slots {
        let item = u16::from_be_bytes(take(payload, offset)?);
        let quantity = u16::from_be_bytes(take(payload, offset)?);
        if item == 0 && quantity == 0 {
            continue;
        }
        *slot = Some(
            ItemStack::new(ItemId::new(item), quantity)
                .map_err(|error| ProtocolError::new(error.to_string()))?,
        );
    }
    Ok(Inventory::from_slots(slots))
}

/// Six equipment item IDs (`u16`, 0 empty) in slot order.
pub const EQUIPMENT_RECORD_BYTES: usize = EQUIPMENT_SLOTS * 2;

pub(crate) fn encode_equipment(payload: &mut Vec<u8>, equipment: &Equipment) {
    for slot in equipment.slots() {
        payload.extend_from_slice(&slot.map_or(0, ItemId::get).to_be_bytes());
    }
}

/// Rejects unknown items and items in a slot they were not made for.
pub(crate) fn decode_equipment(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Equipment, ProtocolError> {
    let mut slots = [None; EQUIPMENT_SLOTS];
    for slot in &mut slots {
        let item = u16::from_be_bytes(take(payload, offset)?);
        *slot = (item != 0).then_some(ItemId::new(item));
    }
    Equipment::from_slots(slots).map_err(|error| ProtocolError::new(error.to_string()))
}
