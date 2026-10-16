//! Immutable item content and bounded bag rules. Player ownership and queued
//! intents are integrated separately; this value never grants network authority.

use std::{error::Error, fmt};

use crate::equipment::{EquipmentSlot, ItemStats};

pub const INVENTORY_SLOTS: usize = 16;
/// Revision 3 adds the Wolf Pelt quest item (#25).
pub const ITEM_CATALOG_REVISION: u64 = 3;
/// Re-send unchanged bags every ten ticks so a lost change self-heals.
pub const INVENTORY_RESEND_TICKS: u64 = 10;

/// Stable catalog identity. Zero and unknown values fail closed on lookup.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ItemId(u16);

impl ItemId {
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemTemplate {
    pub id: ItemId,
    pub name: &'static str,
    pub max_stack: u16,
    /// The equipment slot the item fits; `None` for plain bag items.
    pub slot: Option<EquipmentSlot>,
    /// Attributes added while equipped; zero for plain bag items.
    pub stats: ItemStats,
}

/// Starter catalog, in stable ID order. Equippable items stack to one;
/// consumable effects, prices and loot tables are separate rules. Quest
/// items are plain bag items that quest content names as collect objectives.
pub const ITEM_CATALOG: [ItemTemplate; 10] = [
    ItemTemplate {
        id: ItemId::new(1),
        name: "Torn Fur",
        max_stack: 20,
        slot: None,
        stats: ItemStats::NONE,
    },
    ItemTemplate {
        id: ItemId::new(2),
        name: "Worn Dagger",
        max_stack: 1,
        slot: Some(EquipmentSlot::MainHand),
        stats: ItemStats {
            stamina: 0,
            strength: 2,
            agility: 2,
            intellect: 0,
        },
    },
    ItemTemplate {
        id: ItemId::new(3),
        name: "Militia Shortsword",
        max_stack: 1,
        slot: Some(EquipmentSlot::MainHand),
        stats: ItemStats {
            stamina: 1,
            strength: 3,
            agility: 0,
            intellect: 0,
        },
    },
    ItemTemplate {
        id: ItemId::new(4),
        name: "Apprentice Wand",
        max_stack: 1,
        slot: Some(EquipmentSlot::MainHand),
        stats: ItemStats {
            stamina: 0,
            strength: 0,
            agility: 0,
            intellect: 4,
        },
    },
    ItemTemplate {
        id: ItemId::new(5),
        name: "Pine Buckler",
        max_stack: 1,
        slot: Some(EquipmentSlot::OffHand),
        stats: ItemStats {
            stamina: 2,
            strength: 0,
            agility: 0,
            intellect: 0,
        },
    },
    ItemTemplate {
        id: ItemId::new(6),
        name: "Cloth Hood",
        max_stack: 1,
        slot: Some(EquipmentSlot::Head),
        stats: ItemStats {
            stamina: 1,
            strength: 0,
            agility: 0,
            intellect: 2,
        },
    },
    ItemTemplate {
        id: ItemId::new(7),
        name: "Padded Tunic",
        max_stack: 1,
        slot: Some(EquipmentSlot::Chest),
        stats: ItemStats {
            stamina: 2,
            strength: 0,
            agility: 0,
            intellect: 0,
        },
    },
    ItemTemplate {
        id: ItemId::new(8),
        name: "Padded Trousers",
        max_stack: 1,
        slot: Some(EquipmentSlot::Legs),
        stats: ItemStats {
            stamina: 1,
            strength: 0,
            agility: 0,
            intellect: 0,
        },
    },
    ItemTemplate {
        id: ItemId::new(9),
        name: "Worn Boots",
        max_stack: 1,
        slot: Some(EquipmentSlot::Feet),
        stats: ItemStats {
            stamina: 1,
            strength: 0,
            agility: 2,
            intellect: 0,
        },
    },
    ItemTemplate {
        id: ItemId::new(10),
        name: "Wolf Pelt",
        max_stack: 10,
        slot: None,
        stats: ItemStats::NONE,
    },
];

#[must_use]
pub fn item_template(id: ItemId) -> Option<&'static ItemTemplate> {
    ITEM_CATALOG.iter().find(|item| item.id == id)
}

/// A nonempty, catalog-valid stack. Private fields prevent invalid imported
/// state; decoders must use `new` rather than repairing an invalid quantity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemStack {
    item: ItemId,
    quantity: u16,
}

impl ItemStack {
    pub fn new(item: ItemId, quantity: u16) -> Result<Self, InventoryError> {
        let template = item_template(item).ok_or(InventoryError::UnknownItem)?;
        if quantity == 0 || quantity > template.max_stack {
            return Err(InventoryError::InvalidQuantity);
        }
        Ok(Self { item, quantity })
    }

    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    #[must_use]
    pub const fn quantity(self) -> u16 {
        self.quantity
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InventoryError {
    UnknownItem,
    InvalidQuantity,
    InvalidSlot,
    EmptySlot,
    InsufficientItems,
    NoCapacity,
    IncompatibleStacks,
    /// The item has no equipment slot, or does not fit the slot it is in.
    NotEquippable,
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownItem => "unknown item",
            Self::InvalidQuantity => "invalid item quantity",
            Self::InvalidSlot => "invalid inventory slot",
            Self::EmptySlot => "empty inventory slot",
            Self::InsufficientItems => "insufficient items",
            Self::NoCapacity => "insufficient inventory capacity",
            Self::IncompatibleStacks => "cannot split into a different item stack",
            Self::NotEquippable => "item cannot be equipped there",
        })
    }
}

impl Error for InventoryError {}

/// Exactly 16 ordered slots. Mutations are atomic and allocation-free.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inventory {
    slots: [Option<ItemStack>; INVENTORY_SLOTS],
}

impl Inventory {
    /// A deterministic admission grant, not a client operation.
    pub(crate) const fn starter() -> Self {
        let mut slots = [None; INVENTORY_SLOTS];
        slots[0] = Some(ItemStack {
            item: ItemId::new(1),
            quantity: 3,
        });
        slots[1] = Some(ItemStack {
            item: ItemId::new(2),
            quantity: 1,
        });
        Self { slots }
    }

    /// Imported stacks have already passed `ItemStack::new`; array length
    /// enforces capacity without accepting or truncating excess slots.
    #[must_use]
    pub const fn from_slots(slots: [Option<ItemStack>; INVENTORY_SLOTS]) -> Self {
        Self { slots }
    }

    #[must_use]
    pub const fn slots(&self) -> &[Option<ItemStack>; INVENTORY_SLOTS] {
        &self.slots
    }

    /// Fill existing matching stacks in slot order, then empty slots in slot
    /// order. Either the entire quantity fits or the original bag is retained.
    pub fn insert(&mut self, item: ItemId, quantity: u16) -> Result<(), InventoryError> {
        let template = item_template(item).ok_or(InventoryError::UnknownItem)?;
        if quantity == 0 {
            return Err(InventoryError::InvalidQuantity);
        }
        let mut slots = self.slots;
        let mut remaining = quantity;
        for stack in slots
            .iter_mut()
            .flatten()
            .filter(|stack| stack.item == item)
        {
            let added = remaining.min(template.max_stack - stack.quantity);
            stack.quantity += added;
            remaining -= added;
        }
        for slot in &mut slots {
            if slot.is_none() && remaining > 0 {
                let added = remaining.min(template.max_stack);
                *slot = Some(ItemStack {
                    item,
                    quantity: added,
                });
                remaining -= added;
            }
        }
        if remaining > 0 {
            return Err(InventoryError::NoCapacity);
        }
        self.slots = slots;
        Ok(())
    }

    /// Units of `item` across every slot.
    #[must_use]
    pub fn count(&self, item: ItemId) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|stack| stack.item == item)
            .map(|stack| u32::from(stack.quantity))
            .sum()
    }

    /// Takes `quantity` units of `item` from its stacks in slot order. Without
    /// enough units the bag stays unchanged.
    pub fn remove_item(&mut self, item: ItemId, quantity: u16) -> Result<(), InventoryError> {
        if quantity == 0 {
            return Err(InventoryError::InvalidQuantity);
        }
        if self.count(item) < u32::from(quantity) {
            return Err(InventoryError::InsufficientItems);
        }
        let mut remaining = quantity;
        for slot in &mut self.slots {
            let Some(stack) = slot else { continue };
            if stack.item != item || remaining == 0 {
                continue;
            }
            let taken = remaining.min(stack.quantity);
            remaining -= taken;
            *slot = (taken < stack.quantity).then_some(ItemStack {
                item,
                quantity: stack.quantity - taken,
            });
        }
        Ok(())
    }

    /// Takes `quantity` items from one occupied slot, emptying it when the
    /// whole stack goes. Invalid slots, empty slots, zero or excess
    /// quantities leave the bag unchanged.
    pub fn remove(&mut self, slot: usize, quantity: u16) -> Result<ItemStack, InventoryError> {
        let stack = self
            .slots
            .get(slot)
            .ok_or(InventoryError::InvalidSlot)?
            .ok_or(InventoryError::EmptySlot)?;
        if quantity == 0 {
            return Err(InventoryError::InvalidQuantity);
        }
        if quantity > stack.quantity {
            return Err(InventoryError::InsufficientItems);
        }
        self.slots[slot] = (quantity < stack.quantity).then_some(ItemStack {
            item: stack.item,
            quantity: stack.quantity - quantity,
        });
        Ok(ItemStack {
            item: stack.item,
            quantity,
        })
    }

    /// Move/split into an empty slot or merge with the same item. A complete
    /// stack moved onto a different item swaps slots; a partial swap is refused.
    /// A validated move to the same slot is a no-op. No partial merges occur.
    pub fn move_stack(
        &mut self,
        source: usize,
        destination: usize,
        quantity: u16,
    ) -> Result<(), InventoryError> {
        let source_stack = self.slots.get(source).ok_or(InventoryError::InvalidSlot)?;
        let destination_stack = self
            .slots
            .get(destination)
            .ok_or(InventoryError::InvalidSlot)?;
        let stack = source_stack.ok_or(InventoryError::EmptySlot)?;
        if quantity == 0 {
            return Err(InventoryError::InvalidQuantity);
        }
        if quantity > stack.quantity {
            return Err(InventoryError::InsufficientItems);
        }
        if source == destination {
            return Ok(());
        }
        let target = match *destination_stack {
            Some(target) if target.item != stack.item => {
                if quantity != stack.quantity {
                    return Err(InventoryError::IncompatibleStacks);
                }
                self.slots.swap(source, destination);
                return Ok(());
            }
            Some(target) => {
                let max_stack = item_template(stack.item)
                    .ok_or(InventoryError::UnknownItem)?
                    .max_stack;
                if quantity > max_stack - target.quantity {
                    return Err(InventoryError::NoCapacity);
                }
                ItemStack {
                    item: stack.item,
                    quantity: target.quantity + quantity,
                }
            }
            None => ItemStack {
                item: stack.item,
                quantity,
            },
        };
        self.slots[destination] = Some(target);
        self.slots[source] = if quantity == stack.quantity {
            None
        } else {
            Some(ItemStack {
                item: stack.item,
                quantity: stack.quantity - quantity,
            })
        };
        Ok(())
    }
}
