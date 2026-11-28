//! Equipment slots, item stats and the derived player stat rules. Equipping
//! moves a catalog item between a player's own bag and slots atomically;
//! the zone authority sequences and fences those moves.

use crate::inventory::{INVENTORY_SLOTS, ITEM_CATALOG, Inventory, InventoryError, ItemId};
use crate::{ItemStack, PlayerClass, item_template};
use strum::VariantArray;

pub const EQUIPMENT_SLOTS: usize = <EquipmentSlot as strum::VariantArray>::VARIANTS.len();
/// Maximum health each point of equipped stamina adds.
pub const HEALTH_PER_STAMINA: u32 = 5;
/// Points of the class's primary stat per point of damage bonus.
pub const PRIMARY_STAT_PER_DAMAGE: u16 = 2;

/// An equipment slot; the wire and canonical index is its discriminant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum EquipmentSlot {
    MainHand,
    OffHand,
    Head,
    Chest,
    Legs,
    Feet,
}

impl EquipmentSlot {
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
    /// class use strength, Rangers and Rogues agility, Arcanists, Necromancers, Clerics and Druids
    /// intellect.
    #[must_use]
    pub const fn damage_bonus(self, class: Option<PlayerClass>) -> u16 {
        let primary = match class {
            None | Some(PlayerClass::Warden) => self.strength,
            Some(PlayerClass::Ranger | PlayerClass::Rogue) => self.agility,
            Some(
                PlayerClass::Arcanist
                | PlayerClass::Necromancer
                | PlayerClass::Cleric
                | PlayerClass::Druid,
            ) => self.intellect,
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
    /// Validates imported slots: every item exists and fits its slot, and a
    /// two-handed weapon leaves the off hand empty (#1073).
    pub fn from_slots(slots: [Option<ItemId>; EQUIPMENT_SLOTS]) -> Result<Self, InventoryError> {
        for (slot, item) in EquipmentSlot::VARIANTS.iter().copied().zip(slots) {
            if let Some(item) = item {
                let template = item_template(item).ok_or(InventoryError::UnknownItem)?;
                if template.slot != Some(slot) {
                    return Err(InventoryError::NotEquippable);
                }
            }
        }
        let equipment = Self { slots };
        if equipment.two_handed() && equipment.item(EquipmentSlot::OffHand).is_some() {
            return Err(InventoryError::NotEquippable);
        }
        Ok(equipment)
    }

    #[must_use]
    pub const fn slots(&self) -> &[Option<ItemId>; EQUIPMENT_SLOTS] {
        &self.slots
    }

    #[must_use]
    pub const fn item(&self, slot: EquipmentSlot) -> Option<ItemId> {
        self.slots[slot.index() as usize]
    }

    /// Whether the main hand holds a two-handed weapon (#1073), which leaves the off hand empty.
    #[must_use]
    pub fn two_handed(&self) -> bool {
        self.item(EquipmentSlot::MainHand)
            .and_then(item_template)
            .is_some_and(|template| template.weapon == Some(crate::WeaponFamily::TwoHanded))
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
    /// never blocks equipping. A weapon whose family `class` may not wield
    /// (#914) is not equippable. A two-handed weapon and an off-hand item exclude each other
    /// (#1073): equipping one moves the other to the lowest empty bag slot. Refusals change
    /// neither value.
    pub fn equip(
        &mut self,
        bag: &mut Inventory,
        bag_slot: usize,
        class: Option<PlayerClass>,
    ) -> Result<(), InventoryError> {
        let stack = bag
            .slots()
            .get(bag_slot)
            .ok_or(InventoryError::InvalidSlot)?
            .ok_or(InventoryError::EmptySlot)?;
        let template = item_template(stack.item()).ok_or(InventoryError::UnknownItem)?;
        let slot = template.slot.ok_or(InventoryError::NotEquippable)?;
        if template
            .weapon
            .is_some_and(|family| !family.wielded_by(class))
        {
            return Err(InventoryError::NotEquippable);
        }
        let index = usize::from(slot.index());
        let previous = match self.slots[index] {
            Some(item) => Some(ItemStack::new(item, 1)?),
            None => None,
        };
        // Both hands hold a two-handed weapon (#1073): equipping one sends the off-hand item to
        // the bag, and equipping an off-hand item sends the two-handed weapon there.
        let other = match slot {
            EquipmentSlot::MainHand if template.weapon == Some(crate::WeaponFamily::TwoHanded) => {
                Some(EquipmentSlot::OffHand)
            }
            EquipmentSlot::OffHand if self.two_handed() => Some(EquipmentSlot::MainHand),
            _ => None,
        }
        .and_then(|other| Some((usize::from(other.index()), self.item(other)?)));
        // Equippable catalog items stack to one, so the whole stack moves.
        let mut slots = *bag.slots();
        slots[bag_slot] = previous;
        if let Some((other, item)) = other {
            let free = (0..INVENTORY_SLOTS)
                .find(|&free| slots[free].is_none())
                .ok_or(InventoryError::NoCapacity)?;
            slots[free] = Some(ItemStack::new(item, 1)?);
            self.slots[other] = None;
        }
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
        let (mut equipment, mut bag) = (*player.equipment, (*player.inventory).clone());
        let refusal = match change(&mut equipment, &mut bag) {
            Err(InventoryError::NoCapacity) => Some(crate::ErrorCode::InventoryFull),
            Err(InventoryError::NotEquippable) => Some(crate::ErrorCode::NotEquippable),
            Err(_) => Some(crate::ErrorCode::InvalidInventoryMove),
            Ok(()) => {
                let old_max = player.max_health();
                match player.commit_bag(bag.into(), Some(equipment)) {
                    Err(code) => Some(code),
                    Ok(()) => {
                        player.health =
                            follow_max_health(player.health, old_max, player.max_health());
                        None
                    }
                }
            }
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
    use strum::VariantArray;

    #[test]
    fn slot_indices_round_trip() {
        for &slot in EquipmentSlot::VARIANTS {
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
        assert_eq!(MAX_EQUIPPED_STAMINA, 15);
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

#[cfg(test)]
mod two_handed {
    use super::*;
    use crate::GreatWeapon;

    const BUCKLER: ItemId = ItemId::new(5);
    const SHORTSWORD: ItemId = ItemId::new(3);
    const WARDEN: Option<PlayerClass> = Some(PlayerClass::Warden);

    fn bag(items: &[ItemId]) -> Inventory {
        let mut slots = [None; INVENTORY_SLOTS];
        for (slot, &item) in slots.iter_mut().zip(items) {
            *slot = Some(ItemStack::new(item, 1).unwrap());
        }
        Inventory::from_slots(slots)
    }

    fn held(equipment: &Equipment) -> [Option<ItemId>; 2] {
        [EquipmentSlot::MainHand, EquipmentSlot::OffHand].map(|slot| equipment.item(slot))
    }

    #[test]
    fn a_great_weapon_and_an_off_hand_item_exclude_each_other() {
        let maul = GreatWeapon::Maul.item();
        let mut equipment = Equipment::default();
        let mut inventory = bag(&[SHORTSWORD, BUCKLER, maul]);
        equipment.equip(&mut inventory, 0, WARDEN).unwrap();
        equipment.equip(&mut inventory, 1, WARDEN).unwrap();
        assert_eq!(held(&equipment), [Some(SHORTSWORD), Some(BUCKLER)]);
        // The maul takes both hands: the shortsword takes its bag slot, the buckler the
        // lowest empty one.
        equipment.equip(&mut inventory, 2, WARDEN).unwrap();
        assert_eq!(held(&equipment), [Some(maul), None]);
        assert!(equipment.two_handed());
        assert_eq!(inventory.count(SHORTSWORD) + inventory.count(BUCKLER), 2);
        assert_eq!(inventory.slots()[2].map(ItemStack::item), Some(SHORTSWORD));
        // Taking up the buckler again puts the maul away.
        let buckler = (0..INVENTORY_SLOTS)
            .find(|&slot| inventory.slots()[slot].map(ItemStack::item) == Some(BUCKLER))
            .unwrap();
        equipment.equip(&mut inventory, buckler, WARDEN).unwrap();
        assert_eq!(held(&equipment), [None, Some(BUCKLER)]);
        assert_eq!(inventory.count(maul), 1);
        // A restored record never holds both.
        let mut slots = [None; EQUIPMENT_SLOTS];
        slots[0] = Some(maul);
        slots[1] = Some(BUCKLER);
        assert_eq!(
            Equipment::from_slots(slots),
            Err(InventoryError::NotEquippable)
        );
    }

    #[test]
    fn a_full_bag_refuses_the_great_weapon_without_a_change() {
        let maul = GreatWeapon::Greatsword.item();
        let mut equipment = Equipment::default();
        let mut inventory = bag(&[SHORTSWORD, BUCKLER]);
        equipment.equip(&mut inventory, 0, WARDEN).unwrap();
        equipment.equip(&mut inventory, 1, WARDEN).unwrap();
        let mut slots = *inventory.slots();
        slots[0] = Some(ItemStack::new(maul, 1).unwrap());
        for slot in &mut slots[1..] {
            *slot = Some(ItemStack::new(ItemId::new(1), 1).unwrap());
        }
        let mut inventory = Inventory::from_slots(slots);
        let before = (equipment, inventory.clone());
        assert_eq!(
            equipment.equip(&mut inventory, 0, WARDEN),
            Err(InventoryError::NoCapacity)
        );
        assert_eq!((equipment, inventory), before);
        // Only the Warden wields one.
        assert_eq!(
            Equipment::default().equip(&mut bag(&[maul]), 0, Some(PlayerClass::Ranger)),
            Err(InventoryError::NotEquippable)
        );
    }

    #[test]
    fn every_great_weapon_is_a_two_handed_catalog_item() {
        for &look in GreatWeapon::VARIANTS {
            let item = item_template(look.item()).unwrap();
            assert_eq!(
                item.weapon,
                Some(crate::WeaponFamily::TwoHanded),
                "{look:?}"
            );
            assert_eq!(GreatWeapon::of_item(look.item()), Some(look));
            assert_eq!(GreatWeapon::from_code(look.code()), Some(look));
        }
        assert_eq!(GreatWeapon::of_item(ItemId::new(31)), None);
        assert_eq!(GreatWeapon::of_item(ItemId::new(36)), None);
    }
}
