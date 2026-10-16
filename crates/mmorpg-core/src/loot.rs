//! Validated immutable loot rules. Rolling receives explicit random values;
//! corpse ownership, grants, RNG streams and persistence belong to the zone.

use std::{error::Error, fmt, sync::LazyLock};

use crate::{CreatureTemplateId, Inventory, InventoryError, ItemId, ItemStack, item_template};

pub const LOOT_CATALOG_REVISION: u64 = 2;
pub const MAX_LOOT_OUTCOMES: usize = 4;

/// One weighted outcome: no ordinary item, or one bounded stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LootOutcome {
    Nothing {
        weight: u16,
    },
    Item {
        weight: u16,
        item: ItemId,
        /// Inclusive quantity bounds.
        quantity: [u16; 2],
    },
}

impl LootOutcome {
    #[must_use]
    pub const fn weight(self) -> u16 {
        match self {
            Self::Nothing { weight } | Self::Item { weight, .. } => weight,
        }
    }
}

/// Independent supplied rolls, reduced modulo each declared range/bucket.
/// The rule does not draw from or advance any RNG itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LootRolls {
    pub money: u32,
    pub outcome: u32,
    pub quantity: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LootRewards {
    /// Copper, matching the starter economy's `u32` balance contract.
    pub money: u32,
    pub item: Option<ItemStack>,
    /// One unit of a quest item, rolled outside the table while the
    /// tapper's quest still needs it (#25).
    pub quest_item: Option<ItemStack>,
}

/// Credit one validated reward atomically. The caller owns claim eligibility
/// and consumes the reward only after success; this rule provides no authority
/// or duplicate-claim protection. Copper overflow takes precedence over bag errors.
pub fn settle_loot(
    inventory: &mut Inventory,
    copper: &mut u32,
    rewards: LootRewards,
) -> Result<(), LootSettlementError> {
    let balance = copper
        .checked_add(rewards.money)
        .ok_or(LootSettlementError::MoneyOverflow)?;
    // Both stacks fit or neither is granted.
    let mut bag = inventory.clone();
    for stack in [rewards.item, rewards.quest_item].into_iter().flatten() {
        bag.insert(stack.item(), stack.quantity())
            .map_err(LootSettlementError::Inventory)?;
    }
    *inventory = bag;
    *copper = balance;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LootSettlementError {
    MoneyOverflow,
    Inventory(InventoryError),
}

impl fmt::Display for LootSettlementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MoneyOverflow => formatter.write_str("loot would overflow copper balance"),
            Self::Inventory(error) => error.fmt(formatter),
        }
    }
}

impl Error for LootSettlementError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MoneyOverflow => None,
            Self::Inventory(error) => Some(error),
        }
    }
}

/// Validates authored content once; immutable queries and rolls cannot alter it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LootTable {
    money: [u32; 2],
    outcomes: [LootOutcome; MAX_LOOT_OUTCOMES],
    outcome_count: usize,
    total_weight: u32,
}

impl LootTable {
    pub fn new(money: [u32; 2], outcomes: &[LootOutcome]) -> Result<Self, LootTableError> {
        if money[0] > money[1] {
            return Err(LootTableError::InvalidMoneyRange);
        }
        if outcomes.is_empty() || outcomes.len() > MAX_LOOT_OUTCOMES {
            return Err(LootTableError::InvalidOutcomes);
        }
        let mut total_weight = 0;
        for outcome in outcomes {
            if outcome.weight() == 0 {
                return Err(LootTableError::InvalidWeight);
            }
            total_weight += u32::from(outcome.weight());
            if let LootOutcome::Item { item, quantity, .. } = outcome {
                let template = item_template(*item).ok_or(LootTableError::UnknownItem)?;
                if quantity[0] == 0 || quantity[0] > quantity[1] || quantity[1] > template.max_stack
                {
                    return Err(LootTableError::InvalidQuantity);
                }
            }
        }
        let mut stored = [LootOutcome::Nothing { weight: 0 }; MAX_LOOT_OUTCOMES];
        stored[..outcomes.len()].copy_from_slice(outcomes);
        Ok(Self {
            money,
            outcomes: stored,
            outcome_count: outcomes.len(),
            total_weight,
        })
    }

    #[must_use]
    pub const fn money_range(&self) -> [u32; 2] {
        self.money
    }

    #[must_use]
    pub fn outcomes(&self) -> &[LootOutcome] {
        &self.outcomes[..self.outcome_count]
    }

    #[must_use]
    pub fn roll(&self, rolls: LootRolls) -> LootRewards {
        // Widen before computing the inclusive width: 0..=u32::MAX is valid.
        let width = u64::from(self.money[1]) - u64::from(self.money[0]) + 1;
        let money = u32::try_from(u64::from(self.money[0]) + u64::from(rolls.money) % width)
            .expect("validated money range fits u32");
        let mut bucket = rolls.outcome % self.total_weight;
        let outcome = self
            .outcomes()
            .iter()
            .find(|outcome| {
                let weight = u32::from(outcome.weight());
                if bucket < weight {
                    true
                } else {
                    bucket -= weight;
                    false
                }
            })
            .expect("positive validated weights cover every bucket");
        let item = match *outcome {
            LootOutcome::Nothing { .. } => None,
            LootOutcome::Item { item, quantity, .. } => {
                let width = u32::from(quantity[1]) - u32::from(quantity[0]) + 1;
                let amount = u16::try_from(u32::from(quantity[0]) + rolls.quantity % width)
                    .expect("validated quantity fits u16");
                Some(
                    ItemStack::new(item, amount)
                        .expect("validated loot quantity fits its catalog stack"),
                )
            }
        };
        LootRewards {
            money,
            item,
            quest_item: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LootTableError {
    InvalidMoneyRange,
    InvalidOutcomes,
    InvalidWeight,
    UnknownItem,
    InvalidQuantity,
}

impl fmt::Display for LootTableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidMoneyRange => "loot money range is reversed",
            Self::InvalidOutcomes => "loot table must have one to four outcomes",
            Self::InvalidWeight => "loot outcome weight must be positive",
            Self::UnknownItem => "loot outcome names an unknown item",
            Self::InvalidQuantity => "loot quantity range must fit its catalog stack",
        })
    }
}
impl Error for LootTableError {}

/// Authored starter rewards; revision 2 adds the equipment drops of humanoids.
static CATALOG: LazyLock<[(CreatureTemplateId, LootTable); 7]> = LazyLock::new(|| {
    use crate::greyhaven_vale::units::templates::*;
    let fur = |weight, quantity| LootOutcome::Item {
        weight,
        item: ItemId::new(1),
        quantity,
    };
    // Equippable items stack to one.
    let gear = |id, weight| LootOutcome::Item {
        weight,
        item: ItemId::new(id),
        quantity: [1, 1],
    };
    let (dagger, shortsword, wand, buckler) = (2, 3, 4, 5);
    let (hood, tunic, trousers, boots) = (6, 7, 8, 9);
    let nothing = |weight| LootOutcome::Nothing { weight };
    let table =
        |money, outcomes| LootTable::new(money, outcomes).expect("starter loot content is valid");
    [
        (TIMBER_WOLF, table([0, 2], &[fur(3, [1, 2]), nothing(1)])),
        (YOUNG_BOAR, table([0, 3], &[fur(1, [1, 1]), nothing(1)])),
        (GRAIN_RAT, table([0, 1], &[fur(1, [1, 1]), nothing(3)])),
        (
            FIELD_MARAUDER,
            table(
                [2, 6],
                &[
                    gear(dagger, 1),
                    gear(trousers, 1),
                    gear(boots, 1),
                    nothing(6),
                ],
            ),
        ),
        (MIREFIN_LURKER, table([1, 4], &[nothing(1)])),
        (
            REDBRAND_BANDIT,
            table(
                [4, 9],
                &[
                    gear(dagger, 1),
                    gear(shortsword, 1),
                    gear(tunic, 1),
                    nothing(3),
                ],
            ),
        ),
        (
            GARRICK_REDBRAND,
            table(
                [25, 35],
                &[
                    gear(wand, 1),
                    gear(buckler, 1),
                    gear(hood, 1),
                    gear(shortsword, 1),
                ],
            ),
        ),
    ]
});

/// Unknown templates have no table; callers must not substitute another reward.
#[must_use]
pub fn loot_table(template: CreatureTemplateId) -> Option<&'static LootTable> {
    CATALOG
        .iter()
        .find(|(id, _)| *id == template)
        .map(|(_, table)| table)
}
