use mmorpg_core::{
    INVENTORY_SLOTS, Inventory, InventoryError, ItemId, ItemStack, LootRewards,
    LootSettlementError, ZoneId, ZoneSimulation, greyhaven_vale, settle_loot,
};

const FUR: ItemId = ItemId::new(1);
const DAGGER: ItemId = ItemId::new(2);

fn stack(item: ItemId, quantity: u16) -> ItemStack {
    ItemStack::new(item, quantity).unwrap()
}

#[test]
fn settlement_conserves_rewards_and_uses_existing_ordered_merge_then_empty_slots() {
    let mut slots = [Some(stack(DAGGER, 1)); INVENTORY_SLOTS];
    slots[1] = Some(stack(FUR, 18));
    slots[3] = Some(stack(FUR, 17));
    slots[5] = None;
    slots[8] = None;
    let mut bag = Inventory::from_slots(slots);
    let mut copper = 7;
    let rewards = LootRewards {
        money: 35,
        item: Some(stack(FUR, 20)),
        quest_item: None,
    };
    let expected_reward = rewards;
    settle_loot(&mut bag, &mut copper, rewards).unwrap();
    slots[1] = Some(stack(FUR, 20));
    slots[3] = Some(stack(FUR, 20));
    slots[5] = Some(stack(FUR, 15));
    assert_eq!(bag.slots(), &slots);
    assert_eq!(copper, 42);
    assert_eq!(rewards, expected_reward);
}

#[test]
fn insufficient_total_bag_capacity_never_partially_merges_or_credits_money() {
    for available in 0..20 {
        let mut slots = [Some(stack(DAGGER, 1)); INVENTORY_SLOTS];
        slots[4] = Some(stack(FUR, 20 - available));
        let original = Inventory::from_slots(slots);
        let rewards = LootRewards {
            money: 9,
            item: Some(stack(FUR, available + 1)),
            quest_item: None,
        };
        let mut bag = original.clone();
        let mut copper = 17;
        for _ in 0..3 {
            assert_eq!(
                settle_loot(&mut bag, &mut copper, rewards),
                Err(LootSettlementError::Inventory(InventoryError::NoCapacity))
            );
            assert_eq!(bag, original);
            assert_eq!(copper, 17);
        }
    }
}

#[test]
fn exact_copper_boundaries_and_overflow_refusals_leave_items_available() {
    for (initial, reward, expected) in [
        (0, 0, 0),
        (0, u32::MAX, u32::MAX),
        (u32::MAX - 1, 1, u32::MAX),
        (u32::MAX, 0, u32::MAX),
    ] {
        let mut bag = Inventory::default();
        let mut copper = initial;
        settle_loot(
            &mut bag,
            &mut copper,
            LootRewards {
                money: reward,
                item: Some(stack(DAGGER, 1)),
                quest_item: None,
            },
        )
        .unwrap();
        assert_eq!(copper, expected);
        assert_eq!(bag.slots()[0], Some(stack(DAGGER, 1)));
    }
    for (initial, reward) in [(1, u32::MAX), (u32::MAX, 1), (u32::MAX - 10, 11)] {
        let mut bag = Inventory::default();
        let before = bag.clone();
        let mut copper = initial;
        let rewards = LootRewards {
            money: reward,
            item: Some(stack(FUR, 2)),
            quest_item: None,
        };
        for _ in 0..3 {
            assert_eq!(
                settle_loot(&mut bag, &mut copper, rewards),
                Err(LootSettlementError::MoneyOverflow)
            );
            assert_eq!(copper, initial);
            assert_eq!(bag, before);
        }
    }
}

#[test]
fn money_only_empty_and_item_only_rewards_have_explicit_bounded_behavior() {
    let full = Inventory::from_slots([Some(stack(DAGGER, 1)); INVENTORY_SLOTS]);
    let mut bag = full.clone();
    let mut copper = u32::MAX - 1;
    settle_loot(
        &mut bag,
        &mut copper,
        LootRewards {
            money: 1,
            item: None,
            quest_item: None,
        },
    )
    .unwrap();
    settle_loot(
        &mut bag,
        &mut copper,
        LootRewards {
            money: 0,
            item: None,
            quest_item: None,
        },
    )
    .unwrap();
    assert_eq!(copper, u32::MAX);
    assert_eq!(bag, full);
    // Deterministic refusal priority when both limits would be exceeded.
    assert_eq!(
        settle_loot(
            &mut bag,
            &mut copper,
            LootRewards {
                money: 1,
                item: Some(stack(DAGGER, 1)),
                quest_item: None,
            }
        ),
        Err(LootSettlementError::MoneyOverflow)
    );
    assert_eq!(bag, full);
    assert_eq!(copper, u32::MAX);
    bag = Inventory::default();
    settle_loot(
        &mut bag,
        &mut copper,
        LootRewards {
            money: 0,
            item: Some(stack(FUR, 20)),
            quest_item: None,
        },
    )
    .unwrap();
    assert_eq!(copper, u32::MAX);
    assert_eq!(bag.slots()[0], Some(stack(FUR, 20)));
}

#[test]
fn standalone_settlement_has_no_live_zone_or_content_effect() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let before = zone.snapshot().unwrap();
    let mut bag = Inventory::default();
    let mut copper = 0;
    settle_loot(
        &mut bag,
        &mut copper,
        LootRewards {
            money: 35,
            item: Some(stack(DAGGER, 1)),
            quest_item: None,
        },
    )
    .unwrap();
    assert_eq!(zone.snapshot().unwrap(), before);
    assert_eq!(zone.content().revision(), 9);
    assert_eq!(zone.content().fingerprint(), 0x2630_ca15_7169_73f3);
}
