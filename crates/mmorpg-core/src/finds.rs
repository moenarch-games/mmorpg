//! Hybrid loot (#918, owner choice C): handcrafted base items whose individual finds vary in
//! quality and a few tactical traits, and rare named items from authored encounters.
//!
//! A variant is its own immutable catalog item, so bags, stacking, the wire, saves and
//! recovery carry it as any other `ItemId`. A loot outcome names the base item; the roll that
//! settles a one-item stack then picks the base or one of its rolled variants from the same
//! per-creature loot stream (#879), so a corpse's rewards stay one deterministic draw. Named
//! items are never rolled from a base: their encounter's table names them directly.

use crate::{FamilyMelee, ItemId, StrikeProfile, StrikeVariant, WeaponFamily, item_template};

/// The base item's own share against its variants' weights.
pub const STANDARD_FIND_WEIGHT: u16 = 4;

/// How well a find was made or kept. Base catalog items are standard.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ItemQuality {
    Standard,
    /// Better kept or finished: higher attributes, the same handling.
    Fine,
    /// A one-of-a-kind item with its own name and look.
    Named,
    /// Made by a smith from gathered materials (#917): its own name, look and trait.
    Crafted,
}

impl ItemQuality {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Fine => "Fine",
            Self::Named => "Named",
            Self::Crafted => "Smith-made",
        }
    }
}

/// A tactical trait: how a weapon handles, never a spell. It reshapes the wielder's basic
/// strikes of the weapon's own family; class abilities are unchanged.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ItemTrait {
    /// A longer haft: every strike reaches this many units further.
    Reach { units: u16 },
    /// Better balance: every strike's recovery is this many percent shorter.
    Recovery { percent: u8 },
    /// A broad blade: every strike's arc widens by this many degrees either side.
    Sweep { degrees: u8 },
}

impl ItemTrait {
    /// The strike `profile` with the trait applied. The wind-up never changes, so contacts stay
    /// on their clips' authored frames.
    #[must_use]
    pub const fn shape(self, mut profile: StrikeProfile) -> StrikeProfile {
        match self {
            Self::Reach { units } => profile.reach_units += units as i32,
            Self::Recovery { percent } => {
                profile.interval_ticks =
                    (profile.interval_ticks as u32 * (100 - percent as u32) / 100) as u16;
            }
            Self::Sweep { degrees } => {
                profile.half_width += ((degrees as u32 * 65_536 + 180) / 360) as u16;
            }
        }
        profile
    }

    /// Stable code for content fingerprints.
    #[must_use]
    pub const fn code(self) -> [u8; 3] {
        match self {
            Self::Reach { units } => [1, (units >> 8) as u8, units as u8],
            Self::Recovery { percent } => [2, 0, percent],
            Self::Sweep { degrees } => [3, 0, degrees],
        }
    }

    /// What the trait does, as both clients' item lines show it.
    #[must_use]
    pub fn text(self) -> String {
        match self {
            Self::Reach { units } => {
                format!("Long haft: strikes reach {units} cm further")
            }
            Self::Recovery { percent } => {
                format!("Balanced: strikes recover {percent}% sooner")
            }
            Self::Sweep { degrees } => {
                format!("Broad blade: strikes sweep {degrees}° wider each side")
            }
        }
    }
}

/// A named item's own model (asset-tooling `humanoid-equipment`, #761), drawn in place of its
/// family's ordinary weapon.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NamedWeapon {
    /// `equipment.sword.garricks-cleaver`.
    GarricksCleaver,
    /// `equipment.spear.hollowpine-hunter` (#917), the smith's boar-spear.
    HollowpineHunter,
}

impl NamedWeapon {
    /// Stable wire code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::GarricksCleaver => 0,
            Self::HollowpineHunter => 1,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::GarricksCleaver),
            1 => Some(Self::HollowpineHunter),
            _ => None,
        }
    }
}

/// One find's identity beyond its catalog row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemFind {
    pub item: ItemId,
    /// The base item a loot roll turns into this find; `None` for named items, which only
    /// their encounter drops.
    pub base: Option<ItemId>,
    /// The find's share against [`STANDARD_FIND_WEIGHT`] when its base drops.
    pub weight: u16,
    pub quality: ItemQuality,
    pub trait_: Option<ItemTrait>,
    pub named: Option<NamedWeapon>,
    /// Where it comes from and why it is so: Redbrand, hunter, wildlife or artisan, never a
    /// magical reveal.
    pub source: &'static str,
}

const SHORTSWORD: ItemId = ItemId::new(3);
const SPEAR: ItemId = ItemId::new(23);

/// Every variant and named item, in stable item order.
pub const ITEM_FINDS: [ItemFind; 5] = [
    ItemFind {
        item: ItemId::new(24),
        base: Some(SHORTSWORD),
        weight: 2,
        quality: ItemQuality::Fine,
        trait_: None,
        named: None,
        source: "Redbrand-kept militia steel, honed and oiled",
    },
    ItemFind {
        item: ItemId::new(25),
        base: Some(SHORTSWORD),
        weight: 2,
        quality: ItemQuality::Standard,
        trait_: Some(ItemTrait::Recovery { percent: 15 }),
        named: None,
        source: "Rebalanced by a Millbrook artisan, then taken by the Redbrands",
    },
    ItemFind {
        item: ItemId::new(26),
        base: Some(SPEAR),
        weight: 3,
        quality: ItemQuality::Standard,
        trait_: Some(ItemTrait::Reach { units: 40 }),
        named: None,
        source: "A Hollowpine hunter's boar-spear haft, looted from a hunting cache",
    },
    ItemFind {
        item: ItemId::new(27),
        base: None,
        weight: 0,
        quality: ItemQuality::Named,
        trait_: Some(ItemTrait::Sweep { degrees: 10 }),
        named: Some(NamedWeapon::GarricksCleaver),
        source: "Garrick Redbrand's own cleaver, notched on Millbrook's fences",
    },
    // The smith's recipe (#917): no drop rolls it. Quicker thrusts than the long-hafted find's
    // reach, for closing on a charging boar.
    ItemFind {
        item: ItemId::new(30),
        base: None,
        weight: 0,
        quality: ItemQuality::Crafted,
        trait_: Some(ItemTrait::Recovery { percent: 15 }),
        named: Some(NamedWeapon::HollowpineHunter),
        source: "Forged at the Outpost smithy from ridge iron, with a boar-hide grip and tusk lugs",
    },
];

#[must_use]
pub fn item_find(item: ItemId) -> Option<&'static ItemFind> {
    ITEM_FINDS.iter().find(|find| find.item == item)
}

/// The item a dropped `base` becomes: `roll` reduced over the base's own share and its
/// variants' weights, in catalog order. An item without variants is itself.
#[must_use]
pub fn roll_variant(base: ItemId, roll: u32) -> ItemId {
    let variants = || {
        ITEM_FINDS
            .iter()
            .filter(move |find| find.base == Some(base))
    };
    let total: u32 = u32::from(STANDARD_FIND_WEIGHT)
        + variants().map(|find| u32::from(find.weight)).sum::<u32>();
    if total == u32::from(STANDARD_FIND_WEIGHT) {
        return base;
    }
    let mut bucket = roll % total;
    if bucket < u32::from(STANDARD_FIND_WEIGHT) {
        return base;
    }
    bucket -= u32::from(STANDARD_FIND_WEIGHT);
    for find in variants() {
        if bucket < u32::from(find.weight) {
            return find.item;
        }
        bucket -= u32::from(find.weight);
    }
    base
}

/// Whether a dropped `item` may come from an outcome naming `base`: itself or a rolled variant.
#[must_use]
pub fn rolls_as(base: ItemId, item: ItemId) -> bool {
    item == base || item_find(item).is_some_and(|find| find.base == Some(base))
}

/// The basic strike `variant` of `family` as `main_hand` shapes it: its trait applies only
/// while the main-hand weapon is of the striking family.
#[must_use]
pub fn shaped_strike(
    family: WeaponFamily,
    variant: StrikeVariant,
    main_hand: Option<ItemId>,
) -> StrikeProfile {
    let profile = FamilyMelee::of(family).strike(variant);
    main_hand
        .filter(|&item| item_template(item).and_then(|template| template.weapon) == Some(family))
        .and_then(item_find)
        .and_then(|find| find.trait_)
        .map_or(profile, |trait_| trait_.shape(profile))
}

/// A find's line for inventory and character panes: quality, trait and source, or `None` for
/// a standard catalog item.
#[must_use]
pub fn find_text(item: ItemId) -> Option<String> {
    let find = item_find(item)?;
    let mut parts = Vec::new();
    if find.quality != ItemQuality::Standard {
        parts.push(find.quality.name().to_owned());
    }
    if let Some(trait_) = find.trait_ {
        parts.push(trait_.text());
    }
    parts.push(find.source.to_owned());
    Some(parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_find_is_a_catalog_item_of_its_base_slot_and_family() {
        for find in ITEM_FINDS {
            let template = item_template(find.item).expect("find is catalogued");
            if let Some(base) = find.base {
                let base = item_template(base).expect("base is catalogued");
                assert_eq!((template.slot, template.weapon), (base.slot, base.weapon));
                assert!(find.weight > 0 && find.named.is_none());
            } else {
                assert!(matches!(
                    find.quality,
                    ItemQuality::Named | ItemQuality::Crafted
                ));
                assert!(find.named.is_some());
            }
        }
    }

    #[test]
    fn rolled_variants_cover_their_buckets() {
        let picks: Vec<_> = (0..8)
            .map(|roll| roll_variant(SHORTSWORD, roll).get())
            .collect();
        assert_eq!(picks, [3, 3, 3, 3, 24, 24, 25, 25]);
        assert_eq!(roll_variant(ItemId::new(2), 7), ItemId::new(2));
        assert!(rolls_as(SHORTSWORD, ItemId::new(25)) && !rolls_as(SPEAR, ItemId::new(25)));
        assert!(!rolls_as(SHORTSWORD, ItemId::new(27)));
    }
}
