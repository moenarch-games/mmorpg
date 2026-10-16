//! Equipment through the public zone API: equipping, unequipping and
//! swapping, atomic refusals, gear-aware maximum health and the class damage
//! bonus in auto-attacks, direct spells and the projected melee range.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::unit::{player_damage, player_max_health};
use mmorpg_core::{
    CreatureId, EQUIPMENT_SLOTS, EntityRef, Equipment, ErrorCode, INVENTORY_SLOTS, Inventory,
    ItemId, ItemStack, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, WOLF};
use support::classes;

const FUR: ItemId = ItemId::new(1);
const DAGGER: ItemId = ItemId::new(2);
const SHORTSWORD: ItemId = ItemId::new(3);
const WAND: ItemId = ItemId::new(4);
const HOOD: ItemId = ItemId::new(6);
const TUNIC: ItemId = ItemId::new(7);
const BOOTS: ItemId = ItemId::new(9);
const MAIN_HAND: usize = 0;
const CHEST: usize = 3;
const TARGET: EntityRef = EntityRef::Creature(CreatureId::new(1));

type Bag = [Option<ItemStack>; INVENTORY_SLOTS];
type Slots = [Option<ItemId>; EQUIPMENT_SLOTS];

fn stack(item: ItemId, quantity: u16) -> Option<ItemStack> {
    Some(ItemStack::new(item, quantity).unwrap())
}

/// Equipment slots holding each item in its own catalog slot.
fn slots(items: &[ItemId]) -> Slots {
    let mut slots = [None; EQUIPMENT_SLOTS];
    for &item in items {
        let slot = mmorpg_core::item_template(item).unwrap().slot.unwrap();
        slots[usize::from(slot.index())] = Some(item);
    }
    slots
}

/// One starter player in an empty arena, restored with `bag`, `equipped`
/// and `health` (full when `None`).
fn restored(bag: Bag, equipped: &[ItemId], health: Option<u32>) -> ZoneSimulation {
    let mut zone =
        ZoneSimulation::with_content(ZoneId::new(1), arena::arena(vec![], vec![], vec![])).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let player = &mut state.players[0];
    player.inventory = Inventory::from_slots(bag);
    player.equipment = Equipment::from_slots(slots(equipped)).unwrap();
    player.combat.health =
        health.unwrap_or_else(|| player_max_health(1) + player.equipment.totals().bonus_health());
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// The starter bag: three Torn Fur, then the Worn Dagger.
fn starter_bag() -> Bag {
    let mut bag = [None; INVENTORY_SLOTS];
    bag[0] = stack(FUR, 3);
    bag[1] = stack(DAGGER, 1);
    bag
}

/// Applies `command` as the next sequence, advances one tick and returns
/// the player's feedback.
fn step(zone: &mut ZoneSimulation, sequence: u32, command: ZoneCommand) -> Vec<ZoneEvent> {
    zone.apply_command(1, sequence, command).unwrap();
    zone.advance_tick().unwrap();
    zone.snapshot_for_player(1).unwrap().events
}

fn health(zone: &ZoneSimulation) -> [u32; 2] {
    let viewer = zone.snapshot_for_player(1).unwrap().viewer;
    [viewer.health, viewer.max_health]
}

#[test]
fn equipping_unequipping_and_swapping_share_the_bag_revision_and_sheet() {
    let mut zone = restored(starter_bag(), &[], None);
    let before = zone.snapshot().unwrap().players[0].inventory_revision;
    assert!(step(&mut zone, 1, ZoneCommand::EquipItem { bag_slot: 1 }).is_empty());
    let state = zone.snapshot().unwrap();
    let player = &state.players[0];
    assert_eq!(player.equipment.slots()[MAIN_HAND], Some(DAGGER));
    assert_eq!(player.inventory.slots()[1], None);
    assert_eq!(player.inventory.slots()[0], stack(FUR, 3));
    assert_eq!(player.inventory_revision, before + 1);
    assert_eq!(player.inventory_changed_at, state.tick);
    // The change tick sends the whole sheet under the shared revision.
    let view = zone.snapshot_for_player(1).unwrap();
    assert_eq!(view.inventory_revision, before + 1);
    assert_eq!(view.inventory.as_ref(), Some(&player.inventory));
    assert_eq!(view.equipment, Some(player.equipment));
    assert_eq!(view.equipment.unwrap().totals().strength, 2);

    // Unequipping goes to the lowest empty bag slot.
    assert!(step(&mut zone, 2, ZoneCommand::UnequipItem { equipment_slot: 0 }).is_empty());
    let player = zone.snapshot().unwrap().players.remove(0);
    assert_eq!(player.equipment, Equipment::default());
    assert_eq!(player.inventory.slots()[1], stack(DAGGER, 1));
    assert_eq!(player.inventory_revision, before + 2);

    // Equipping over an occupied slot swaps into the same bag slot, even
    // when the bag is full.
    let mut bag = [stack(FUR, 20); INVENTORY_SLOTS];
    bag[9] = stack(SHORTSWORD, 1);
    let mut zone = restored(bag, &[DAGGER], None);
    assert!(step(&mut zone, 1, ZoneCommand::EquipItem { bag_slot: 9 }).is_empty());
    let player = zone.snapshot().unwrap().players.remove(0);
    assert_eq!(player.equipment.slots()[MAIN_HAND], Some(SHORTSWORD));
    assert_eq!(player.inventory.slots()[9], stack(DAGGER, 1));
    assert_eq!(player.inventory.slots()[8], stack(FUR, 20));
}

#[test]
fn every_refusal_leaves_bag_equipment_health_and_revision_unchanged() {
    let mut full = [stack(FUR, 20); INVENTORY_SLOTS];
    full[15] = stack(SHORTSWORD, 1);
    let cases: [(Bag, Option<u32>, u64, ZoneCommand, ErrorCode); 9] = [
        (
            starter_bag(),
            None,
            1,
            ZoneCommand::EquipItem { bag_slot: 16 },
            ErrorCode::InvalidInventoryMove,
        ),
        (
            starter_bag(),
            None,
            1,
            ZoneCommand::EquipItem { bag_slot: u8::MAX },
            ErrorCode::InvalidInventoryMove,
        ),
        (
            starter_bag(),
            None,
            1,
            ZoneCommand::EquipItem { bag_slot: 2 },
            ErrorCode::InvalidInventoryMove,
        ),
        (
            starter_bag(),
            None,
            1,
            ZoneCommand::EquipItem { bag_slot: 0 },
            ErrorCode::NotEquippable,
        ),
        (
            starter_bag(),
            None,
            1,
            ZoneCommand::UnequipItem { equipment_slot: 6 },
            ErrorCode::InvalidInventoryMove,
        ),
        (
            starter_bag(),
            None,
            1,
            ZoneCommand::UnequipItem { equipment_slot: 1 },
            ErrorCode::InvalidInventoryMove,
        ),
        (
            full,
            None,
            1,
            ZoneCommand::UnequipItem { equipment_slot: 3 },
            ErrorCode::InventoryFull,
        ),
        (
            starter_bag(),
            Some(0),
            1,
            ZoneCommand::EquipItem { bag_slot: 1 },
            ErrorCode::YouAreDead,
        ),
        (
            starter_bag(),
            None,
            u64::MAX,
            ZoneCommand::EquipItem { bag_slot: 1 },
            ErrorCode::InvalidInventoryMove,
        ),
    ];
    for (bag, health, revision, command, code) in cases {
        let mut zone = restored(bag, &[TUNIC], health);
        let mut state = zone.snapshot().unwrap();
        state.players[0].inventory_revision = revision;
        zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let before = zone.snapshot().unwrap().players.remove(0);
        let events = step(&mut zone, 1, command);
        assert_eq!(
            events,
            [ZoneEvent::Error { code, target: None }],
            "{command:?}"
        );
        let after = zone.snapshot().unwrap().players.remove(0);
        assert_eq!(after.inventory, before.inventory, "{command:?}");
        assert_eq!(after.equipment, before.equipment, "{command:?}");
        assert_eq!(after.combat.health, before.combat.health, "{command:?}");
        assert_eq!(after.inventory_revision, revision, "{command:?}");
        assert_eq!(after.inventory_changed_at, before.inventory_changed_at);
    }
    // The dead are refused unequipping too.
    let mut zone = restored(starter_bag(), &[TUNIC], Some(0));
    assert_eq!(
        step(&mut zone, 1, ZoneCommand::UnequipItem { equipment_slot: 3 }),
        [ZoneEvent::Error {
            code: ErrorCode::YouAreDead,
            target: None
        }]
    );
    assert_eq!(
        zone.snapshot().unwrap().players[0].equipment.slots()[CHEST],
        Some(TUNIC)
    );
}

#[test]
fn health_follows_the_gear_maximum_and_never_falls_below_one() {
    let base = player_max_health(1);
    let mut bag = starter_bag();
    bag[2] = stack(TUNIC, 1);
    let mut zone = restored(bag, &[], None);
    assert_eq!(health(&zone), [base, base]);
    // Two stamina: ten more maximum and current health.
    step(&mut zone, 1, ZoneCommand::EquipItem { bag_slot: 2 });
    assert_eq!(health(&zone), [base + 10, base + 10]);
    step(&mut zone, 2, ZoneCommand::UnequipItem { equipment_slot: 3 });
    assert_eq!(health(&zone), [base, base]);

    // A loss lowers current health by the same amount, never below 1.
    for (current, expected) in [(30, 20), (10, 1), (1, 1)] {
        let mut zone = restored(starter_bag(), &[TUNIC], Some(current));
        assert_eq!(health(&zone), [current, base + 10]);
        step(&mut zone, 1, ZoneCommand::UnequipItem { equipment_slot: 3 });
        assert_eq!(health(&zone), [expected, base], "from {current}");
    }

    // Recovery bounds health by the gear-aware maximum.
    let zone = restored(starter_bag(), &[TUNIC], None);
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.health = base + 11;
    assert!(ZoneSimulation::from_snapshot(state.clone(), Arc::clone(zone.content())).is_err());
    state.players[0].combat.health = base + 10;
    state.players[0].equipment = Equipment::default();
    assert!(ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).is_err());
}

/// Two copies of one class fight after the class choice: the first without
/// gear, the second wearing `items`. Gear draws nothing from the zone's
/// random stream, so both see the same rolls.
fn twins(
    content: &Arc<ZoneContent>,
    level: u8,
    class: u8,
    items: &[ItemId],
) -> [ZoneSimulation; 2] {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(content)).unwrap();
    zone.add_player(1).unwrap();
    let mut zone = classes::at_level(zone, 1, level);
    zone.apply_command(1, 1, ZoneCommand::ChooseClass { class, sex: 0 })
        .unwrap();
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].equipment = Equipment::from_slots(slots(items)).unwrap();
    let geared = ZoneSimulation::from_snapshot(state, Arc::clone(content)).unwrap();
    [zone, geared]
}

/// Damage the player dealt to the target over `ticks`, with its tick and
/// critical flag.
fn hits(zone: &mut ZoneSimulation, ticks: u64) -> Vec<(u64, u16, bool)> {
    let mut hits = Vec::new();
    for _ in 0..ticks {
        zone.advance_tick().unwrap();
        let view = zone.snapshot_for_player(1).unwrap();
        for event in view.events {
            if let ZoneEvent::DamageDealt {
                target: TARGET,
                amount,
                critical,
                ..
            } = event
            {
                hits.push((view.tick, amount, critical));
            }
        }
    }
    hits
}

#[test]
fn the_primary_stat_raises_auto_attacks_and_the_projected_melee_range() {
    let content = classes::arena(
        vec![arena::wolf(1_000, [1, 1])],
        vec![arena::spawn(1, WOLF, [-200, 0])],
    );
    // Warden: strength 3 from the shortsword adds 1.
    let [mut plain, mut geared] = twins(&content, 1, 0, &[SHORTSWORD]);
    assert_eq!(plain.snapshot_for_player(1).unwrap().viewer.damage, [3, 6]);
    assert_eq!(geared.snapshot_for_player(1).unwrap().viewer.damage, [4, 7]);
    for zone in [&mut plain, &mut geared] {
        zone.apply_command(1, 2, ZoneCommand::SelectTarget(Some(TARGET)))
            .unwrap();
        zone.apply_command(1, 3, ZoneCommand::StartAttack).unwrap();
    }
    let (plain, geared) = (hits(&mut plain, 600), hits(&mut geared, 600));
    assert_eq!(plain.len(), geared.len());
    let mut ordinary = 0;
    for ((tick, low, critical), (geared_tick, high, geared_critical)) in
        plain.into_iter().zip(geared)
    {
        assert_eq!((tick, critical), (geared_tick, geared_critical));
        if !critical {
            assert_eq!(high, low + 1, "tick {tick}");
            ordinary += 1;
        }
    }
    assert!(ordinary >= 3, "{ordinary} ordinary swings");

    // Rangers use agility, Arcanists intellect; other stats add nothing.
    for (class, items, bonus) in [
        (1, &[DAGGER, BOOTS][..], 2),
        (1, &[SHORTSWORD][..], 0),
        (2, &[WAND, HOOD][..], 3),
        (2, &[DAGGER][..], 0),
        (0, &[WAND][..], 0),
    ] {
        let [_, geared] = twins(&content, 4, class, items);
        let [low, high] = player_damage(4);
        assert_eq!(
            geared.snapshot_for_player(1).unwrap().viewer.damage,
            [low + bonus, high + bonus],
            "class {class} with {items:?}"
        );
    }
}

#[test]
fn intellect_adds_to_each_direct_bolt_and_nova_hit() {
    let content = classes::arena(
        vec![arena::wolf(1_000, [1, 1])],
        vec![arena::spawn(1, WOLF, [-400, 0])],
    );
    // Six intellect: three more damage per direct hit.
    let [mut plain, mut geared] = twins(&content, 6, 2, &[WAND, HOOD]);
    let mut dealt = [Vec::new(), Vec::new()];
    for (zone, dealt) in [&mut plain, &mut geared].into_iter().zip(&mut dealt) {
        zone.apply_command(1, 2, ZoneCommand::SelectTarget(Some(TARGET)))
            .unwrap();
        zone.apply_command(
            1,
            3,
            ZoneCommand::UseAbility {
                ability: ids::FIREBOLT.get(),
                target: None,
            },
        )
        .unwrap();
        dealt.extend(hits(zone, 70));
        zone.apply_command(
            1,
            4,
            ZoneCommand::UseAbility {
                ability: ids::FROST_NOVA.get(),
                target: None,
            },
        )
        .unwrap();
        dealt.extend(hits(zone, 1));
    }
    let [plain, geared] = dealt;
    assert_eq!(plain.len(), 2, "one bolt and one nova hit: {plain:?}");
    assert_eq!(geared.len(), 2);
    for ((tick, low, _), (geared_tick, high, _)) in plain.into_iter().zip(geared) {
        assert_eq!(tick, geared_tick);
        assert_eq!(high, low + 3, "tick {tick}");
    }
}
