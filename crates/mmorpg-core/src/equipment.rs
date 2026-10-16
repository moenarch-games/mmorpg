//! Equipment slots, item stats and the derived player stat rules. Equipping
//! moves a catalog item between a player's own bag and slots atomically;
//! the zone authority sequences and fences those moves.

use crate::inventory::{INVENTORY_SLOTS, ITEM_CATALOG, Inventory, InventoryError, ItemId};
use crate::{ItemStack, PlayerClass, item_template};

pub const EQUIPMENT_SLOTS: usize = 6;
/// Maximum health each point of equipped stamina adds.
pub const HEALTH_PER_STAMINA: u32 = 5;
/// Points of the class's primary stat per point of damage bonus.
pub const PRIMARY_STAT_PER_DAMAGE: u16 = 2;

/// An equipment slot; the wire and canonical index is its discriminant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EquipmentSlot {
    MainHand,
    OffHand,
    Head,
    Chest,
    Legs,
    Feet,
}

impl EquipmentSlot {
    pub const ALL: [Self; EQUIPMENT_SLOTS] = [
        Self::MainHand,
        Self::OffHand,
        Self::Head,
        Self::Chest,
        Self::Legs,
        Self::Feet,
    ];

    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::MainHand),
            1 => Some(Self::OffHand),
            2 => Some(Self::Head),
            3 => Some(Self::Chest),
            4 => Some(Self::Legs),
            5 => Some(Self::Feet),
            _ => None,
        }
    }

    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::MainHand => 0,
            Self::OffHand => 1,
            Self::Head => 2,
            Self::Chest => 3,
            Self::Legs => 4,
            Self::Feet => 5,
        }
    }

    /// Stable camel-case name for exports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MainHand => "mainHand",
            Self::OffHand => "offHand",
            Self::Head => "head",
            Self::Chest => "chest",
            Self::Legs => "legs",
            Self::Feet => "feet",
        }
    }
}

/// The attributes one item adds while equipped.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ItemStats {
    pub stamina: u8,
    pub strength: u8,
    pub agility: u8,
    pub intellect: u8,
}

impl ItemStats {
    pub const NONE: Self = Self {
        stamina: 0,
        strength: 0,
        agility: 0,
        intellect: 0,
    };
}

/// The sums over every equipped item; players have no base attributes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatTotals {
    pub stamina: u16,
    pub strength: u16,
    pub agility: u16,
    pub intellect: u16,
}

impl StatTotals {
    /// Maximum health the equipped stamina adds.
    #[must_use]
    pub const fn bonus_health(self) -> u32 {
        self.stamina as u32 * HEALTH_PER_STAMINA
    }

    /// Damage added to the player melee range and direct spell hits: half
    /// the class's primary stat, rounded down. Wardens and players without a
    /// class use strength, Rangers agility and Arcanists intellect.
    #[must_use]
    pub const fn damage_bonus(self, class: Option<PlayerClass>) -> u16 {
        let primary = match class {
            None | Some(PlayerClass::Warden) => self.strength,
            Some(PlayerClass::Ranger) => self.agility,
            Some(PlayerClass::Arcanist) => self.intellect,
        };
        primary / PRIMARY_STAT_PER_DAMAGE
    }
}

/// The largest stamina any combination of catalog items reaches: the
/// most-stamina item of every slot. Bounds recovered heal-over-time amounts.
pub const MAX_EQUIPPED_STAMINA: u16 = {
    let mut total = 0;
    let mut slot = 0;
    while slot < EQUIPMENT_SLOTS {
        let mut best = 0;
        let mut index = 0;
        while index < ITEM_CATALOG.len() {
            let item = &ITEM_CATALOG[index];
            if let Some(item_slot) = item.slot
                && item_slot.index() as usize == slot
                && item.stats.stamina > best
            {
                best = item.stats.stamina;
            }
            index += 1;
        }
        total += best as u16;
        slot += 1;
    }
    total
};

/// Six slots, each empty or holding one catalog item made for that slot.
/// Private fields keep imported equipment valid.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Equipment {
    slots: [Option<ItemId>; EQUIPMENT_SLOTS],
}

impl Equipment {
    /// Validates imported slots: every item exists and fits its slot.
    pub fn from_slots(slots: [Option<ItemId>; EQUIPMENT_SLOTS]) -> Result<Self, InventoryError> {
        for (slot, item) in EquipmentSlot::ALL.into_iter().zip(slots) {
            if let Some(item) = item {
                let template = item_template(item).ok_or(InventoryError::UnknownItem)?;
                if template.slot != Some(slot) {
                    return Err(InventoryError::NotEquippable);
                }
            }
        }
        Ok(Self { slots })
    }

    #[must_use]
    pub const fn slots(&self) -> &[Option<ItemId>; EQUIPMENT_SLOTS] {
        &self.slots
    }

    #[must_use]
    pub const fn item(&self, slot: EquipmentSlot) -> Option<ItemId> {
        self.slots[slot.index() as usize]
    }

    /// The summed stats of every equipped item.
    #[must_use]
    pub fn totals(&self) -> StatTotals {
        let mut totals = StatTotals::default();
        for stats in self
            .slots
            .iter()
            .flatten()
            .filter_map(|&item| item_template(item))
            .map(|template| template.stats)
        {
            totals.stamina += u16::from(stats.stamina);
            totals.strength += u16::from(stats.strength);
            totals.agility += u16::from(stats.agility);
            totals.intellect += u16::from(stats.intellect);
        }
        totals
    }

    /// Equips the item in `bag_slot` into its catalog slot. A previously
    /// equipped item takes its place in the same bag slot, so a full bag
    /// never blocks equipping. Refusals change neither value.
    pub fn equip(&mut self, bag: &mut Inventory, bag_slot: usize) -> Result<(), InventoryError> {
        let stack = bag
            .slots()
            .get(bag_slot)
            .ok_or(InventoryError::InvalidSlot)?
            .ok_or(InventoryError::EmptySlot)?;
        let slot = item_template(stack.item())
            .ok_or(InventoryError::UnknownItem)?
            .slot
            .ok_or(InventoryError::NotEquippable)?;
        let index = usize::from(slot.index());
        let previous = match self.slots[index] {
            Some(item) => Some(ItemStack::new(item, 1)?),
            None => None,
        };
        // Equippable catalog items stack to one, so the whole stack moves.
        let mut slots = *bag.slots();
        slots[bag_slot] = previous;
        *bag = Inventory::from_slots(slots);
        self.slots[index] = Some(stack.item());
        Ok(())
    }

    /// Moves the item in `slot` to the lowest empty bag slot. Refusals
    /// change neither value.
    pub fn unequip(&mut self, bag: &mut Inventory, slot: u8) -> Result<(), InventoryError> {
        let index = EquipmentSlot::from_index(slot).ok_or(InventoryError::InvalidSlot)?;
        let index = usize::from(index.index());
        let item = self.slots[index].ok_or(InventoryError::EmptySlot)?;
        let free = (0..INVENTORY_SLOTS)
            .find(|&bag_slot| bag.slots()[bag_slot].is_none())
            .ok_or(InventoryError::NoCapacity)?;
        let mut slots = *bag.slots();
        slots[free] = Some(ItemStack::new(item, 1)?);
        *bag = Inventory::from_slots(slots);
        self.slots[index] = None;
        Ok(())
    }
}

impl crate::ZoneSimulation {
    /// Tick step 1 of `EquipItem` and `UnequipItem`: stages the bag and
    /// equipment, preflights the shared revision, then commits both together
    /// with the health change of the new maximum. Refusals change nothing.
    pub(crate) fn change_equipment(
        &mut self,
        player_id: crate::PlayerId,
        change: impl FnOnce(&mut Equipment, &mut Inventory) -> Result<(), InventoryError>,
    ) -> Result<Option<(crate::ErrorCode, Option<crate::EntityRef>)>, crate::ZoneError> {
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| crate::ZoneError::new("equipment owner is absent"))?;
        let (mut equipment, mut bag) = (player.equipment, player.inventory.clone());
        let refusal = match change(&mut equipment, &mut bag) {
            Err(InventoryError::NoCapacity) => Some(crate::ErrorCode::InventoryFull),
            Err(InventoryError::NotEquippable) => Some(crate::ErrorCode::NotEquippable),
            Err(_) => Some(crate::ErrorCode::InvalidInventoryMove),
            Ok(()) => match player.inventory_revision.checked_add(1) {
                None => Some(crate::ErrorCode::InvalidInventoryMove),
                Some(revision) => {
                    let old_max = player.max_health();
                    player.equipment = equipment;
                    player.inventory = bag;
                    player.inventory_revision = revision;
                    player.inventory_changed_at = self.tick + 1;
                    player.health = follow_max_health(player.health, old_max, player.max_health());
                    None
                }
            },
        };
        Ok(refusal.map(|code| (code, None)))
    }
}

/// A living player's health after its maximum changes: it rises or falls
/// by the same amount, never below 1.
#[must_use]
pub(crate) const fn follow_max_health(health: u32, old_max: u32, new_max: u32) -> u32 {
    if new_max >= old_max {
        health + (new_max - old_max)
    } else {
        let lowered = health.saturating_sub(old_max - new_max);
        if lowered == 0 { 1 } else { lowered }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_indices_round_trip() {
        for slot in EquipmentSlot::ALL {
            assert_eq!(EquipmentSlot::from_index(slot.index()), Some(slot));
        }
        assert_eq!(EquipmentSlot::from_index(6), None);
    }

    #[test]
    fn equippable_items_stack_to_one() {
        for item in ITEM_CATALOG {
            if item.slot.is_some() {
                assert_eq!(item.max_stack, 1, "{}", item.name);
            } else {
                assert_eq!(item.stats, ItemStats::NONE, "{}", item.name);
            }
        }
        assert_eq!(MAX_EQUIPPED_STAMINA, 8);
    }

    #[test]
    fn the_primary_stat_follows_the_class() {
        let totals = StatTotals {
            stamina: 1,
            strength: 5,
            agility: 3,
            intellect: 4,
        };
        assert_eq!(totals.bonus_health(), 5);
        assert_eq!(totals.damage_bonus(None), 2);
        assert_eq!(totals.damage_bonus(Some(PlayerClass::Warden)), 2);
        assert_eq!(totals.damage_bonus(Some(PlayerClass::Ranger)), 1);
        assert_eq!(totals.damage_bonus(Some(PlayerClass::Arcanist)), 2);
    }
}
