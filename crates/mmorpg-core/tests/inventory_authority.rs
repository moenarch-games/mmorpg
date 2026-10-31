use std::sync::Arc;

use mmorpg_core::{
    ErrorCode, Inventory, ItemId, ItemStack, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
};

fn zone() -> ZoneSimulation {
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    zone
}

fn move_item(source: u8, destination: u8, quantity: u16) -> ZoneCommand {
    ZoneCommand::MoveItem {
        source,
        destination,
        quantity,
    }
}

fn stack(quantity: u16) -> Option<ItemStack> {
    Some(ItemStack::new(ItemId::new(1), quantity).unwrap())
}

#[test]
fn moves_are_queued_owned_sequenced_and_recover_before_consumption() {
    let mut zone = zone();
    let initial = zone.snapshot().unwrap();
    assert_eq!(initial.players[0].inventory.slots()[0], stack(3));
    assert_eq!(
        initial.players[0].inventory.slots()[1].unwrap().item(),
        ItemId::new(2)
    );
    zone.apply_command(1, 1, move_item(0, 15, 2)).unwrap();
    let pending = zone.snapshot().unwrap();
    assert_eq!(pending.players[0].inventory, initial.players[0].inventory);
    let mut restored = ZoneSimulation::from_snapshot(pending, Arc::clone(zone.content())).unwrap();
    for simulation in [&mut zone, &mut restored] {
        simulation.advance_tick().unwrap();
        let before_stale = simulation.snapshot().unwrap();
        assert_eq!(before_stale.players[0].inventory.slots()[0], stack(1));
        assert_eq!(before_stale.players[0].inventory.slots()[15], stack(2));
        assert_eq!(before_stale.players[0].inventory_revision, 2);
        assert_eq!(
            before_stale.players[1].inventory,
            initial.players[1].inventory
        );
        assert!(simulation.apply_command(1, 1, move_item(15, 0, 2)).is_err());
        assert_eq!(simulation.snapshot().unwrap(), before_stale);
    }
    assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    for simulation in [&mut zone, &mut restored] {
        simulation.apply_command(1, 2, move_item(15, 0, 2)).unwrap();
        simulation.apply_command(1, 3, move_item(0, 0, 1)).unwrap();
        simulation.advance_tick().unwrap();
        let state = simulation.snapshot().unwrap();
        assert_eq!(state.players[0].inventory, initial.players[0].inventory);
        assert_eq!(
            state.players[0].inventory_revision, 3,
            "no-op does not change revision"
        );
    }
    for _ in 0..mmorpg_core::golden::t30(20) {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
        assert_eq!(
            zone.snapshot_for_player(1).unwrap(),
            restored.snapshot_for_player(1).unwrap()
        );
    }
}

#[test]
fn every_projection_carries_the_complete_bag() {
    // Delivery on change is the protocol's (#544); the zone always projects the whole bag.
    let mut zone = zone();
    assert!(zone.snapshot_for_player(1).unwrap().inventory.is_some());
    zone.apply_command(1, 1, move_item(0, 15, 2)).unwrap();
    zone.advance_tick().unwrap();
    let changed = zone.snapshot_for_player(1).unwrap();
    assert_eq!(changed.inventory_revision, 2);
    for tick in 2..5 {
        zone.advance_tick().unwrap();
        let before_query = zone.snapshot().unwrap();
        let received = zone.snapshot_for_player(1).unwrap();
        assert_eq!(received.tick, tick);
        assert_eq!(received.inventory_revision, 2);
        assert_eq!(received.inventory, changed.inventory);
        assert!(received.equipment.is_some() && received.quests.is_some());
        assert_eq!(
            zone.snapshot().unwrap(),
            before_query,
            "projection never mutates authority"
        );
    }
}

#[test]
fn invalid_moves_are_feedback_and_do_not_mutate_bags_or_revisions() {
    let mut zone = zone();
    let original = zone.snapshot().unwrap().players[0].inventory.clone();
    for (sequence, command) in [
        (1, move_item(255, 0, 1)),
        (2, move_item(0, 16, 1)),
        (3, move_item(15, 0, 1)),
        (4, move_item(0, 15, 0)),
        (5, move_item(0, 15, 4)),
        (6, move_item(0, 1, 1)),
    ] {
        zone.apply_command(1, sequence, command).unwrap();
        zone.advance_tick().unwrap();
        let received = zone.snapshot_for_player(1).unwrap();
        assert_eq!(received.acknowledged_sequence, sequence);
        assert_eq!(
            received.events,
            vec![ZoneEvent::Error {
                code: ErrorCode::InvalidInventoryMove,
                target: None
            }]
        );
        let state = zone.snapshot().unwrap();
        assert_eq!(state.players[0].inventory, original);
        assert_eq!(state.players[0].inventory_revision, 1);
    }
}

#[test]
fn full_destination_death_and_revision_exhaustion_fail_without_partial_mutation() {
    let mut original = zone();
    let content = Arc::clone(original.content());
    let mut state = original.snapshot().unwrap();
    let mut slots = [None; 16];
    slots[0] = stack(3);
    slots[1] = stack(19);
    state.players[0].inventory = Inventory::from_slots(slots);
    original = ZoneSimulation::from_snapshot(state.clone(), Arc::clone(&content)).unwrap();
    original.apply_command(1, 1, move_item(0, 1, 2)).unwrap();
    original.advance_tick().unwrap();
    assert_eq!(
        original.snapshot().unwrap().players[0].inventory,
        state.players[0].inventory
    );
    assert!(
        original
            .snapshot_for_player(1)
            .unwrap()
            .events
            .contains(&ZoneEvent::Error {
                code: ErrorCode::InventoryFull,
                target: None
            })
    );
    for (dead, revision, expected) in [
        (true, 1, ErrorCode::YouAreDead),
        (false, u64::MAX, ErrorCode::InvalidInventoryMove),
    ] {
        let mut checkpoint = state.clone();
        if dead {
            checkpoint.players[0].combat.health = 0;
        }
        checkpoint.players[0].inventory_revision = revision;
        let mut simulation =
            ZoneSimulation::from_snapshot(checkpoint.clone(), Arc::clone(&content)).unwrap();
        simulation.apply_command(1, 1, move_item(0, 15, 1)).unwrap();
        simulation.advance_tick().unwrap();
        let player = &simulation.snapshot().unwrap().players[0];
        assert_eq!(player.inventory, checkpoint.players[0].inventory);
        assert_eq!(player.inventory_revision, revision);
        assert!(
            simulation
                .snapshot_for_player(1)
                .unwrap()
                .events
                .contains(&ZoneEvent::Error {
                    code: expected,
                    target: None
                })
        );
    }
}

#[test]
fn recovery_rejects_a_zero_revision() {
    let zone = zone();
    let mut state = zone.snapshot().unwrap();
    state.players[0].inventory_revision = 0;
    assert!(ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).is_err());
}
