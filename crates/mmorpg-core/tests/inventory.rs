use mmorpg_core::{INVENTORY_SLOTS, Inventory, InventoryError, ItemId, ItemStack, item_template};

const FUR: ItemId = ItemId::new(1);
const DAGGER: ItemId = ItemId::new(2);

fn stack(item: ItemId, quantity: u16) -> Option<ItemStack> {
    Some(ItemStack::new(item, quantity).unwrap())
}

fn bag() -> Inventory {
    let mut slots = [None; INVENTORY_SLOTS];
    slots[0] = stack(FUR, 12);
    slots[1] = stack(FUR, 18);
    slots[2] = stack(DAGGER, 1);
    Inventory::from_slots(slots)
}

fn totals(bag: &Inventory) -> [u32; 2] {
    let mut totals = [0; 2];
    for stack in bag.slots().iter().flatten() {
        match stack.item() {
            FUR => {
                assert!((1..=20).contains(&stack.quantity()));
                totals[0] += u32::from(stack.quantity());
            }
            DAGGER => {
                assert_eq!(stack.quantity(), 1);
                totals[1] += 1;
            }
            unknown => panic!("unknown item {unknown:?}"),
        }
    }
    totals
}

#[test]
fn catalog_and_stack_import_fail_closed_at_boundaries() {
    assert_eq!(item_template(FUR).unwrap().name, "Torn Fur");
    assert_eq!(item_template(DAGGER).unwrap().name, "Worn Dagger");
    for id in [0, 11, u16::MAX] {
        assert!(item_template(ItemId::new(id)).is_none());
        assert_eq!(
            ItemStack::new(ItemId::new(id), 1),
            Err(InventoryError::UnknownItem)
        );
    }
    for (item, quantity) in [
        (FUR, 0),
        (FUR, 21),
        (DAGGER, 0),
        (DAGGER, 2),
        (FUR, u16::MAX),
    ] {
        assert_eq!(
            ItemStack::new(item, quantity),
            Err(InventoryError::InvalidQuantity)
        );
    }
    assert_eq!(ItemStack::new(FUR, 20).unwrap().quantity(), 20);
    assert_eq!(ItemStack::new(DAGGER, 1).unwrap().item().get(), 2);
    assert_eq!(Inventory::default().slots(), &[None; 16]);
}

#[test]
fn insert_fills_matching_stacks_before_first_empty_slots_without_reordering() {
    let mut slots = [None; INVENTORY_SLOTS];
    slots[9] = stack(FUR, 18);
    slots[14] = stack(FUR, 5);
    let mut bag = Inventory::from_slots(slots);
    assert_eq!(bag.slots(), &slots);
    bag.insert(FUR, 9).unwrap();
    slots[9] = stack(FUR, 20);
    slots[14] = stack(FUR, 12);
    assert_eq!(bag.slots(), &slots);
    bag.insert(FUR, 20).unwrap();
    slots[14] = stack(FUR, 20);
    slots[0] = stack(FUR, 12);
    assert_eq!(bag.slots(), &slots);
    bag.insert(DAGGER, 2).unwrap();
    slots[1] = stack(DAGGER, 1);
    slots[2] = stack(DAGGER, 1);
    assert_eq!(bag.slots(), &slots);
    assert_eq!(totals(&bag), [52, 2]);
}

#[test]
fn capacity_failures_retain_every_original_slot_even_after_possible_partial_fill() {
    let mut slots = [stack(FUR, 20); INVENTORY_SLOTS];
    slots[7] = stack(FUR, 19);
    let mut bag = Inventory::from_slots(slots);
    for (item, quantity, expected) in [
        (FUR, 2, InventoryError::NoCapacity),
        (FUR, u16::MAX, InventoryError::NoCapacity),
        (DAGGER, 1, InventoryError::NoCapacity),
        (FUR, 0, InventoryError::InvalidQuantity),
        (ItemId::new(0), 1, InventoryError::UnknownItem),
    ] {
        assert_eq!(bag.insert(item, quantity), Err(expected));
        assert_eq!(bag.slots(), &slots);
    }
    bag.insert(FUR, 1).unwrap();
    assert_eq!(totals(&bag), [320, 0]);
    let mut empty = Inventory::default();
    empty.insert(FUR, 320).unwrap();
    assert_eq!(empty, bag);
}

#[test]
fn split_merge_move_swap_and_same_slot_preserve_items() {
    let mut bag = bag();
    bag.move_stack(0, 4, 5).unwrap();
    assert_eq!(bag.slots()[0], stack(FUR, 7));
    assert_eq!(bag.slots()[4], stack(FUR, 5));
    bag.move_stack(0, 1, 2).unwrap();
    assert_eq!(bag.slots()[0], stack(FUR, 5));
    assert_eq!(bag.slots()[1], stack(FUR, 20));
    bag.move_stack(4, 15, 5).unwrap();
    assert_eq!(bag.slots()[4], None);
    bag.move_stack(2, 15, 1).unwrap();
    assert_eq!(bag.slots()[2], stack(FUR, 5));
    assert_eq!(bag.slots()[15], stack(DAGGER, 1));
    let before = bag.clone();
    bag.move_stack(2, 2, 2).unwrap();
    assert_eq!(bag, before);
    assert_eq!(totals(&bag), [30, 1]);
}

#[test]
fn invalid_moves_are_atomic_and_never_silently_move_a_partial_quantity() {
    let mut bag = bag();
    let before = bag.clone();
    for (source, destination, quantity, expected) in [
        (16, 0, 1, InventoryError::InvalidSlot),
        (0, usize::MAX, 1, InventoryError::InvalidSlot),
        (15, 0, 1, InventoryError::EmptySlot),
        (0, 1, 0, InventoryError::InvalidQuantity),
        (0, 1, 13, InventoryError::InsufficientItems),
        (0, 1, 3, InventoryError::NoCapacity),
        (0, 2, 1, InventoryError::IncompatibleStacks),
        (0, 0, 13, InventoryError::InsufficientItems),
    ] {
        assert_eq!(bag.move_stack(source, destination, quantity), Err(expected));
        assert_eq!(bag, before);
    }
}

#[test]
fn bounded_move_matrix_conserves_item_totals_and_preserves_every_refused_bag() {
    let original = bag();
    let expected = totals(&original);
    for source in 0..=INVENTORY_SLOTS {
        for destination in 0..=INVENTORY_SLOTS {
            for quantity in [0, 1, 2, 5, 12, 18, 20, 21, u16::MAX] {
                let mut candidate = original.clone();
                let result = candidate.move_stack(source, destination, quantity);
                assert_eq!(totals(&candidate), expected);
                if result.is_err() {
                    assert_eq!(candidate, original);
                }
            }
        }
    }
}
