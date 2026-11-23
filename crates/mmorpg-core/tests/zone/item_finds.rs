//! Hybrid loot (#918): two finds of one base item play differently, Garrick Redbrand's corpse
//! yields his named cleaver, the shortsword's finds roll from Redbrand corpses on the
//! per-creature loot stream, and a looted find survives save and replays exactly.
use crate::support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::units::templates::{GARRICK_REDBRAND, REDBRAND_BANDIT};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureBehaviour, CreatureId, CreatureLife, CreatureTemplate,
    EntityRef, EquipmentSlot, Inventory, ItemId, ItemQuality, ItemStack, LootClaim, LootOutcome,
    ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, item_find, loot_table,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;
const SHORTSWORD: ItemId = ItemId::new(3);
const FINE_SHORTSWORD: ItemId = ItemId::new(24);
const BALANCED_SHORTSWORD: ItemId = ItemId::new(25);
const SPEAR: ItemId = ItemId::new(23);
const LONG_SPEAR: ItemId = ItemId::new(26);
const CLEAVER: ItemId = ItemId::new(27);
/// The empty bag slot a test weapon is put in.
const WEAPON_SLOT: u8 = 2;
const CREATURE: EntityRef = EntityRef::Creature(CreatureId::new(1));

fn send(zone: &mut ZoneSimulation, command: ZoneCommand) {
    let sequence = zone.snapshot().unwrap().players[0].last_sequence + 1;
    zone.apply_command(1, sequence, command).unwrap();
}

/// What one standing strike with `weapon` did: the recovery it started, whether it covered a
/// sturdy creature at `creature`, and the Warden's equipped strength.
fn strike(weapon: ItemId, creature: [i32; 2]) -> (u16, bool, u16) {
    let template = CreatureTemplate {
        behaviour: CreatureBehaviour::Neutral,
        ..arena::wolf(50_000, [1, 1])
    };
    let content = Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            vec![template],
            vec![arena::spawn(1, WOLF, creature)],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Default::default())
        .unwrap(),
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    send(&mut zone, ZoneCommand::ChooseClass { class: 0, sex: 0 });
    send(&mut zone, arena::stand(SOUTH));
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    let mut slots = *state.players[0].inventory.slots();
    slots[usize::from(WEAPON_SLOT)] = Some(ItemStack::new(weapon, 1).unwrap());
    state.players[0].inventory = Inventory::from_slots(slots);
    let mut zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    send(
        &mut zone,
        ZoneCommand::EquipItem {
            bag_slot: WEAPON_SLOT,
        },
    );
    zone.advance_tick().unwrap();
    let player = &zone.snapshot().unwrap().players[0];
    assert_eq!(player.equipment.item(EquipmentSlot::MainHand), Some(weapon));
    let strength = player.equipment.totals().strength;
    send(&mut zone, ZoneCommand::Swing { facing: SOUTH });
    zone.advance_tick().unwrap();
    let recovery = zone.snapshot().unwrap().players[0].combat.swing_timer;
    for _ in 0..120 {
        zone.advance_tick().unwrap();
        for event in zone.snapshot_for_player(1).unwrap().events {
            if let ZoneEvent::AttackPerformed {
                source: EntityRef::Player(1),
                target,
            } = event
            {
                return (recovery, target == Some(CREATURE), strength);
            }
        }
    }
    panic!("the strike never reached its contact");
}

#[test]
fn two_finds_of_one_base_item_play_differently() {
    let ahead = [0, -200];
    let (base, _, base_strength) = strike(SHORTSWORD, ahead);
    let (fine, _, fine_strength) = strike(FINE_SHORTSWORD, ahead);
    let (balanced, _, balanced_strength) = strike(BALANCED_SHORTSWORD, ahead);
    // The fine blade hits harder; the balanced one recovers 15% sooner at the base's strength.
    assert!(fine_strength > base_strength && fine == base);
    assert!(balanced < base && balanced_strength == base_strength);
    // The hunter's long haft reaches 3.6 m, past the militia spear's 3.3 m thrust.
    let far = [0, -360];
    assert!(!strike(SPEAR, far).1 && strike(LONG_SPEAR, far).1);
    // Replay gives the same outcomes.
    assert_eq!(strike(BALANCED_SHORTSWORD, ahead).0, balanced);
}

/// An arena where a one-health creature carries `template`'s authored loot table.
fn corpse_zone(table_of: mmorpg_core::CreatureTemplateId, zone_id: u32) -> ZoneSimulation {
    let table = loot_table(table_of).unwrap().clone();
    let content = Arc::new(
        arena::arena(
            vec![arena::wolf(1, [1, 1])],
            vec![arena::spawn(1, WOLF, [-180, 0])],
            vec![],
        )
        .as_ref()
        .clone()
        .with_loot_tables(vec![(WOLF, table)])
        .unwrap(),
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(zone_id), content).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.creatures[0].health = 1;
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// Kills the creature; its corpse's claim and rolled item.
fn kill(zone: &mut ZoneSimulation) -> (LootClaim, Option<ItemId>) {
    send(zone, ZoneCommand::SelectTarget(Some(CREATURE)));
    for _ in 0..600 {
        if let Some(swing) = arena::swing_at(zone, 1, CREATURE) {
            send(zone, swing);
        }
        zone.advance_tick().unwrap();
        let state = zone.snapshot().unwrap();
        if let CreatureLife::Corpse { died_at } = state.creatures[0].life {
            let claim = LootClaim {
                creature: CreatureId::new(1),
                died_at,
            };
            return (
                claim,
                state.creatures[0].loot.unwrap().item.map(ItemStack::item),
            );
        }
    }
    panic!("creature did not die");
}

fn reload(state: &CanonicalZoneSnapshot, zone: &ZoneSimulation) -> ZoneSimulation {
    ZoneSimulation::from_snapshot(state.clone(), Arc::clone(zone.content())).unwrap()
}

#[test]
fn garricks_cleaver_drops_from_his_encounter_and_a_looted_find_saves_and_replays() {
    let garrick = loot_table(GARRICK_REDBRAND).unwrap();
    assert!(
        garrick
            .outcomes()
            .iter()
            .any(|outcome| matches!(outcome, LootOutcome::Item { item, .. } if *item == CLEAVER))
    );
    let named = item_find(CLEAVER).unwrap();
    assert_eq!(named.quality, ItemQuality::Named);
    assert!(named.source.contains("Garrick Redbrand"));

    // Each zone's corpse rolls on its own loot stream: one of the first few drops the cleaver.
    let zone_id = (1..40)
        .find(|&id| kill(&mut corpse_zone(GARRICK_REDBRAND, id)).1 == Some(CLEAVER))
        .expect("Garrick's corpse yields his cleaver");
    let mut zone = corpse_zone(GARRICK_REDBRAND, zone_id);
    let (claim, _) = kill(&mut zone);
    // The corpse's rolled find survives a save before the claim.
    let mut zone = reload(&zone.snapshot().unwrap(), &zone);
    send(&mut zone, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    let state = zone.snapshot().unwrap();
    assert_eq!(state.players[0].inventory.count(CLEAVER), 1);
    // Saved and restored, then replayed from the same inputs: the same bytes.
    let mut restored = reload(&state, &zone);
    for _ in 0..20 {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
    }
    assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    let mut replay = corpse_zone(GARRICK_REDBRAND, zone_id);
    assert_eq!(kill(&mut replay), (claim, Some(CLEAVER)));

    // A Redbrand bandit's shortsword rolls as the plain blade or either of its finds.
    let mut rolled = std::collections::BTreeSet::new();
    for id in 1..400 {
        if let Some(item) = kill(&mut corpse_zone(REDBRAND_BANDIT, id)).1 {
            rolled.insert(item.get());
        }
        if [3, 24, 25].iter().all(|item| rolled.contains(item)) {
            return;
        }
    }
    panic!("the bandit's shortsword finds never all rolled: {rolled:?}");
}
