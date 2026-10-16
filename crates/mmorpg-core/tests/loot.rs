use mmorpg_core::{
    CreatureTemplateId, ItemId, LOOT_CATALOG_REVISION, LootOutcome, LootRolls, LootTable,
    LootTableError, MAX_LOOT_OUTCOMES, ZoneId, ZoneSimulation, greyhaven_vale, loot_table,
};

const FUR: ItemId = ItemId::new(1);
const DAGGER: ItemId = ItemId::new(2);

fn item(weight: u16, id: ItemId, quantity: [u16; 2]) -> LootOutcome {
    LootOutcome::Item {
        weight,
        item: id,
        quantity,
    }
}

#[test]
fn invalid_authored_tables_fail_before_rolling() {
    let none = LootOutcome::Nothing { weight: 1 };
    assert_eq!(MAX_LOOT_OUTCOMES, 4);
    for (money, outcomes, expected) in [
        ([2, 1], vec![none], LootTableError::InvalidMoneyRange),
        ([0, 1], vec![], LootTableError::InvalidOutcomes),
        ([0, 1], vec![none; 5], LootTableError::InvalidOutcomes),
        (
            [0, 1],
            vec![LootOutcome::Nothing { weight: 0 }],
            LootTableError::InvalidWeight,
        ),
        (
            [0, 1],
            vec![item(0, FUR, [1, 1])],
            LootTableError::InvalidWeight,
        ),
        (
            [0, 1],
            vec![item(1, ItemId::new(0), [1, 1])],
            LootTableError::UnknownItem,
        ),
        (
            [0, 1],
            vec![item(1, ItemId::new(u16::MAX), [1, 1])],
            LootTableError::UnknownItem,
        ),
        (
            [0, 1],
            vec![item(1, FUR, [0, 1])],
            LootTableError::InvalidQuantity,
        ),
        (
            [0, 1],
            vec![item(1, FUR, [2, 1])],
            LootTableError::InvalidQuantity,
        ),
        (
            [0, 1],
            vec![item(1, FUR, [1, 21])],
            LootTableError::InvalidQuantity,
        ),
        (
            [0, 1],
            vec![item(1, DAGGER, [1, 2])],
            LootTableError::InvalidQuantity,
        ),
    ] {
        assert_eq!(LootTable::new(money, &outcomes), Err(expected));
    }
}

#[test]
fn weighted_buckets_and_inclusive_ranges_have_exact_boundaries() {
    let table = LootTable::new(
        [10, 12],
        &[
            LootOutcome::Nothing { weight: 2 },
            item(1, DAGGER, [1, 1]),
            item(3, FUR, [2, 4]),
        ],
    )
    .unwrap();
    let expected = [
        None,
        None,
        Some((DAGGER, 1)),
        Some((FUR, 4)),
        Some((FUR, 4)),
        Some((FUR, 4)),
    ];
    for (bucket, expected) in (0..6).zip(expected) {
        let rolls = LootRolls {
            money: 2,
            outcome: bucket,
            quantity: 2,
        };
        let result = table.roll(rolls);
        assert_eq!(result.money, 12);
        assert_eq!(
            result.item.map(|stack| (stack.item(), stack.quantity())),
            expected
        );
        assert_eq!(
            table.roll(LootRolls {
                outcome: bucket + 6,
                ..rolls
            }),
            result
        );
    }
    for (roll, expected_money) in [(0, 10), (1, 11), (2, 12), (3, 10), (u32::MAX, 10)] {
        assert_eq!(
            table
                .roll(LootRolls {
                    money: roll,
                    outcome: 0,
                    quantity: 0
                })
                .money,
            expected_money
        );
    }
    for (roll, expected_quantity) in [(0, 2), (1, 3), (2, 4), (3, 2), (u32::MAX, 2)] {
        assert_eq!(
            table
                .roll(LootRolls {
                    money: 0,
                    outcome: 3,
                    quantity: roll
                })
                .item
                .unwrap()
                .quantity(),
            expected_quantity
        );
    }
}

#[test]
fn extreme_money_ranges_and_weights_never_overflow() {
    let table = LootTable::new(
        [0, u32::MAX],
        &[LootOutcome::Nothing { weight: u16::MAX }; 4],
    )
    .unwrap();
    for roll in [0, 1, u32::MAX - 1, u32::MAX] {
        let result = table.roll(LootRolls {
            money: roll,
            outcome: roll,
            quantity: roll,
        });
        assert_eq!(result.money, roll);
        assert_eq!(result.item, None);
    }
    let single = LootTable::new([u32::MAX, u32::MAX], &[item(1, FUR, [20, 20])]).unwrap();
    let result = single.roll(LootRolls {
        money: u32::MAX,
        outcome: u32::MAX,
        quantity: u32::MAX,
    });
    assert_eq!(result.money, u32::MAX);
    assert_eq!(result.item.unwrap().quantity(), 20);
}

#[test]
fn starter_tables_are_pinned_and_cover_every_hosted_template() {
    assert_eq!(LOOT_CATALOG_REVISION, 2);
    let gear = |id| item(1, ItemId::new(id), [1, 1]);
    let nothing = |weight| LootOutcome::Nothing { weight };
    let rows = [
        (
            [0, 2],
            vec![item(3, FUR, [1, 2]), LootOutcome::Nothing { weight: 1 }],
        ),
        (
            [0, 3],
            vec![item(1, FUR, [1, 1]), LootOutcome::Nothing { weight: 1 }],
        ),
        (
            [0, 1],
            vec![item(1, FUR, [1, 1]), LootOutcome::Nothing { weight: 3 }],
        ),
        ([2, 6], vec![gear(2), gear(8), gear(9), nothing(6)]),
        ([1, 4], vec![LootOutcome::Nothing { weight: 1 }]),
        ([4, 9], vec![gear(2), gear(3), gear(7), nothing(3)]),
        ([25, 35], vec![gear(4), gear(5), gear(6), gear(3)]),
    ];
    let content = greyhaven_vale::content();
    assert_eq!(content.creature_templates().len(), rows.len());
    for (template, (money, outcomes)) in content.creature_templates().iter().zip(rows) {
        let table = loot_table(template.id).unwrap();
        assert_eq!(table.money_range(), money);
        assert_eq!(table.outcomes(), outcomes);
        assert_eq!(*table, LootTable::new(money, &outcomes).unwrap());
    }
    for unknown in [0, 8, u16::MAX] {
        assert!(loot_table(CreatureTemplateId::new(unknown)).is_none());
    }
}

#[test]
fn supplied_rolls_repeat_with_valid_rewards_and_do_not_touch_live_zone_state() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let before = zone.snapshot().unwrap();
    for template in zone.content().creature_templates() {
        let table = loot_table(template.id).unwrap();
        for money in [0, 1, 7, u32::MAX] {
            for outcome in [0, 1, 7, u32::MAX] {
                for quantity in [0, 1, 7, u32::MAX] {
                    let rolls = LootRolls {
                        money,
                        outcome,
                        quantity,
                    };
                    let result = table.roll(rolls);
                    assert_eq!(result, table.roll(rolls));
                    assert!(
                        (table.money_range()[0]..=table.money_range()[1]).contains(&result.money)
                    );
                    if let Some(stack) = result.item {
                        assert!(stack.quantity() > 0);
                        assert!(
                            stack.quantity()
                                <= mmorpg_core::item_template(stack.item()).unwrap().max_stack
                        );
                    }
                }
            }
        }
    }
    assert_eq!(zone.snapshot().unwrap(), before);
}
