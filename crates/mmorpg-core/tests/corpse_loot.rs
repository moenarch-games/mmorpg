mod support;

use std::sync::Arc;

use mmorpg_core::{
    CreatureId, CreatureLife, EntityRef, ErrorCode, INVENTORY_SLOTS, Inventory, ItemId, ItemStack,
    LootClaim, LootOutcome, LootRewards, LootTable, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, WOLF};

const FUR: ItemId = ItemId::new(1);
const CREATURE: CreatureId = CreatureId::new(1);
const TARGET: EntityRef = EntityRef::Creature(CREATURE);

fn fixture(table: Option<LootTable>) -> ZoneSimulation {
    let original = arena::arena(
        vec![arena::wolf(1, [1, 1])],
        vec![arena::spawn(1, WOLF, [-180, 0])],
        vec![],
    );
    let content = if let Some(table) = table {
        Arc::new(
            original
                .as_ref()
                .clone()
                .with_loot_tables(1, vec![(WOLF, table)])
                .unwrap(),
        )
    } else {
        original
    };
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.creatures[0].health = 1;
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn rule(money: u32, item: bool) -> LootTable {
    let outcome = if item {
        LootOutcome::Item {
            weight: 1,
            item: FUR,
            quantity: [1, 1],
        }
    } else {
        LootOutcome::Nothing { weight: 1 }
    };
    LootTable::new([money, money], &[outcome]).unwrap()
}

fn command(zone: &mut ZoneSimulation, player: u32, command: ZoneCommand) {
    let sequence = zone
        .snapshot()
        .unwrap()
        .players
        .iter()
        .find(|record| record.player_id == player)
        .unwrap()
        .last_sequence
        + 1;
    zone.apply_command(player, sequence, command).unwrap();
}

fn kill(zone: &mut ZoneSimulation) -> LootClaim {
    command(zone, 1, ZoneCommand::SelectTarget(Some(TARGET)));
    command(zone, 1, ZoneCommand::StartAttack);
    for _ in 0..180 {
        zone.advance_tick().unwrap();
        if let CreatureLife::Corpse { died_at } = zone.snapshot().unwrap().creatures[0].life {
            return LootClaim {
                creature: CREATURE,
                died_at,
            };
        }
    }
    panic!("creature did not die");
}

fn corpse() -> (ZoneSimulation, LootClaim) {
    let mut zone = fixture(Some(rule(2, true)));
    let claim = kill(&mut zone);
    (zone, claim)
}

fn restore(
    zone: &mut ZoneSimulation,
    change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot),
) {
    let mut state = zone.snapshot().unwrap();
    change(&mut state);
    *zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
}

fn refuse(zone: &mut ZoneSimulation, player: u32, claim: LootClaim, code: ErrorCode) {
    let before = zone.snapshot().unwrap();
    command(zone, player, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    let after = zone.snapshot().unwrap();
    for (old, new) in before.players.iter().zip(&after.players) {
        assert_eq!(
            (old.copper, &old.inventory, old.inventory_revision),
            (new.copper, &new.inventory, new.inventory_revision)
        );
    }
    assert_eq!(after.loot_rng_state, before.loot_rng_state);
    assert!(
        zone.snapshot_for_player(player)
            .unwrap()
            .events
            .contains(&ZoneEvent::Error {
                code,
                target: Some(EntityRef::Creature(claim.creature))
            })
            || code == ErrorCode::YouAreDead
                && zone
                    .snapshot_for_player(player)
                    .unwrap()
                    .events
                    .contains(&ZoneEvent::Error { code, target: None })
    );
}

#[test]
fn death_rolls_once_on_a_separate_stream_and_recovery_preserves_the_rewards() {
    let mut zone = fixture(Some(rule(2, true)));
    let rng = zone.snapshot().unwrap().loot_rng_state;
    let claim = kill(&mut zone);
    let state = zone.snapshot().unwrap();
    assert_eq!(
        state.loot_rng_state,
        rng.wrapping_add(0x9e37_79b9_7f4a_7c15_u64.wrapping_mul(3))
    );
    let view = zone.snapshot_for_player(1).unwrap().loot.unwrap();
    assert_eq!(view.claim, claim);
    assert_eq!(
        view.rewards,
        LootRewards {
            money: 2,
            item: Some(ItemStack::new(FUR, 1).unwrap()),
            quest_item: None,
        }
    );
    assert_eq!(state.creatures[0].loot, Some(view.rewards));
    let mut recovered =
        ZoneSimulation::from_snapshot(state.clone(), Arc::clone(zone.content())).unwrap();
    for _ in 0..30 {
        assert_eq!(zone.snapshot_for_player(1).unwrap().loot, Some(view));
        assert_eq!(zone.snapshot().unwrap(), recovered.snapshot().unwrap());
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
    }
    assert_eq!(
        zone.snapshot().unwrap().loot_rng_state,
        state.loot_rng_state
    );
}

#[test]
fn sequenced_pending_claim_recovers_and_commits_items_and_copper_once() {
    let (mut zone, claim) = corpse();
    let before = zone.snapshot().unwrap();
    command(&mut zone, 1, ZoneCommand::Loot(claim));
    let pending = zone.snapshot().unwrap();
    assert_eq!(pending.players[0].copper, before.players[0].copper);
    assert_eq!(pending.creatures[0].loot, before.creatures[0].loot);
    let mut recovered = ZoneSimulation::from_snapshot(pending, Arc::clone(zone.content())).unwrap();
    zone.advance_tick().unwrap();
    recovered.advance_tick().unwrap();
    let after = zone.snapshot().unwrap();
    assert_eq!(after, recovered.snapshot().unwrap());
    assert_eq!(after.players[0].copper, 2);
    assert_eq!(
        after.players[0].inventory.slots()[0],
        Some(ItemStack::new(FUR, 4).unwrap())
    );
    assert_eq!(
        after.players[0].inventory_revision,
        before.players[0].inventory_revision + 1
    );
    assert_eq!(after.players[0].inventory_changed_at, after.tick);
    assert!(after.creatures[0].loot.is_none());
    assert!(zone.snapshot_for_player(1).unwrap().loot.is_none());
    assert!(
        !zone
            .snapshot_for_player(1)
            .unwrap()
            .entities
            .iter()
            .find(|e| e.entity() == TARGET)
            .unwrap()
            .flags
            .lootable
    );
    assert!(
        zone.apply_command(1, after.players[0].last_sequence, ZoneCommand::Loot(claim))
            .is_err()
    );
    for _ in 0..3 {
        refuse(&mut zone, 1, claim, ErrorCode::EmptyLoot);
    }
}

#[test]
fn later_duplicates_in_one_intent_queue_cannot_mint_another_reward() {
    let (mut zone, claim) = corpse();
    for _ in 0..6 {
        command(&mut zone, 1, ZoneCommand::Loot(claim));
    }
    zone.advance_tick().unwrap();
    let state = zone.snapshot().unwrap();
    assert_eq!(state.players[0].copper, 2);
    assert_eq!(state.players[0].inventory_revision, 2);
    assert_eq!(
        state.players[0]
            .combat
            .events
            .iter()
            .filter(|event| matches!(
                event,
                ZoneEvent::Error {
                    code: ErrorCode::EmptyLoot,
                    ..
                }
            ))
            .count(),
        5
    );
}

#[test]
fn ownership_life_and_three_dimensional_reach_are_authoritative() {
    let (mut zone, claim) = corpse();
    zone.add_player(2).unwrap();
    refuse(&mut zone, 2, claim, ErrorCode::NotLootOwner);
    assert!(zone.snapshot_for_player(2).unwrap().loot.is_none());
    restore(&mut zone, |state| state.players[0].combat.health = 0);
    refuse(&mut zone, 1, claim, ErrorCode::YouAreDead);
    assert!(zone.snapshot_for_player(1).unwrap().loot.is_none());
    restore(&mut zone, |state| {
        state.players[0].combat.health = 50;
        state.players[0].position = [
            state.creatures[0].position[0],
            state.creatures[0].position[1] + 301,
            state.creatures[0].position[2],
        ];
    });
    assert!(zone.snapshot_for_player(1).unwrap().loot.is_none());
    refuse(&mut zone, 1, claim, ErrorCode::OutOfRange);
    restore(&mut zone, |state| {
        state.players[0].position[1] = state.creatures[0].position[1] + 300
    });
    command(&mut zone, 1, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    assert_eq!(zone.snapshot().unwrap().players[0].copper, 2);
}

#[test]
fn removing_and_reusing_player_identity_cannot_inherit_corpse_ownership() {
    let (mut zone, claim) = corpse();
    assert!(zone.remove_player(1));
    let corpse = &zone.snapshot().unwrap().creatures[0];
    assert!(corpse.tapped_by.is_none());
    assert!(corpse.loot.is_none());
    zone.add_player(1).unwrap();
    refuse(&mut zone, 1, claim, ErrorCode::NotLootOwner);
    assert_eq!(zone.snapshot().unwrap().players[0].copper, 0);
}

#[test]
fn bag_full_refusals_retain_all_rewards_and_retry_without_rerolling() {
    let (mut zone, claim) = corpse();
    restore(&mut zone, |state| {
        state.players[0].inventory = Inventory::from_slots(
            [Some(ItemStack::new(ItemId::new(2), 1).unwrap()); INVENTORY_SLOTS],
        )
    });
    let loot = zone.snapshot().unwrap().creatures[0].loot;
    for _ in 0..3 {
        refuse(&mut zone, 1, claim, ErrorCode::InventoryFull);
        assert_eq!(zone.snapshot().unwrap().creatures[0].loot, loot);
    }
    let rng = zone.snapshot().unwrap().loot_rng_state;
    restore(&mut zone, |state| {
        state.players[0].inventory = Inventory::default()
    });
    command(&mut zone, 1, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    assert_eq!(zone.snapshot().unwrap().players[0].copper, 2);
    assert_eq!(zone.snapshot().unwrap().loot_rng_state, rng);
}

#[test]
fn copper_and_inventory_revision_overflows_refuse_without_consuming_the_claim() {
    for revision_overflow in [false, true] {
        let (mut zone, claim) = corpse();
        restore(&mut zone, |state| {
            if revision_overflow {
                state.players[0].inventory_revision = u64::MAX;
            } else {
                state.players[0].copper = u32::MAX - 1;
            }
        });
        let rewards = zone.snapshot().unwrap().creatures[0].loot;
        refuse(
            &mut zone,
            1,
            claim,
            if revision_overflow {
                ErrorCode::InvalidInventoryMove
            } else {
                ErrorCode::MoneyOverflow
            },
        );
        assert_eq!(zone.snapshot().unwrap().creatures[0].loot, rewards);
    }
    let (mut zone, claim) = corpse();
    restore(&mut zone, |state| state.players[0].copper = u32::MAX - 2);
    command(&mut zone, 1, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    assert_eq!(zone.snapshot().unwrap().players[0].copper, u32::MAX);
}

#[test]
fn money_only_rewards_work_with_a_full_bag_and_exhausted_inventory_revision() {
    let mut zone = fixture(Some(rule(2, false)));
    let claim = kill(&mut zone);
    restore(&mut zone, |state| {
        state.players[0].inventory_revision = u64::MAX;
        state.players[0].inventory = Inventory::from_slots(
            [Some(ItemStack::new(ItemId::new(2), 1).unwrap()); INVENTORY_SLOTS],
        );
    });
    let bag = zone.snapshot().unwrap().players[0].inventory.clone();
    command(&mut zone, 1, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    let state = zone.snapshot().unwrap();
    assert_eq!(state.players[0].copper, 2);
    assert_eq!(state.players[0].inventory, bag);
    assert_eq!(state.players[0].inventory_revision, u64::MAX);
    assert!(state.creatures[0].loot.is_none());
}

#[test]
fn stale_unknown_and_expiry_boundary_claims_fail_closed() {
    let (mut zone, claim) = corpse();
    refuse(
        &mut zone,
        1,
        LootClaim {
            died_at: claim.died_at + 1,
            ..claim
        },
        ErrorCode::InvalidLoot,
    );
    refuse(
        &mut zone,
        1,
        LootClaim {
            creature: CreatureId::new(u32::MAX),
            ..claim
        },
        ErrorCode::InvalidLoot,
    );
    let deadline = claim.died_at + u64::from(mmorpg_core::unit::CORPSE_TICKS);
    while zone.snapshot().unwrap().tick < deadline - 1 {
        zone.advance_tick().unwrap();
    }
    assert!(zone.snapshot_for_player(1).unwrap().loot.is_some());
    refuse(&mut zone, 1, claim, ErrorCode::InvalidLoot);
    assert!(zone.snapshot().unwrap().creatures[0].loot.is_none());
    assert!(zone.snapshot_for_player(1).unwrap().loot.is_none());
    let respawn = claim.died_at
        + u64::from(
            zone.content()
                .creature_template(WOLF)
                .unwrap()
                .respawn_ticks,
        );
    while zone.snapshot().unwrap().tick < respawn {
        zone.advance_tick().unwrap();
    }
    assert_eq!(
        zone.snapshot().unwrap().creatures[0].life,
        CreatureLife::Alive
    );
    refuse(&mut zone, 1, claim, ErrorCode::InvalidLoot);
    restore(&mut zone, |state| state.creatures[0].health = 1);
    let next = kill(&mut zone);
    assert!(next.died_at > claim.died_at);
    refuse(&mut zone, 1, claim, ErrorCode::InvalidLoot);
    command(&mut zone, 1, ZoneCommand::Loot(next));
    zone.advance_tick().unwrap();
    assert_eq!(zone.snapshot().unwrap().players[0].copper, 2);
}

#[test]
fn malformed_canonical_corpse_rewards_and_ownership_refuse_recovery() {
    let (zone, _) = corpse();
    let original = zone.snapshot().unwrap();
    for case in 0..7 {
        let mut state = original.clone();
        match case {
            0 => state.creatures[0].loot.as_mut().unwrap().money = 3,
            1 => state.creatures[0].loot.as_mut().unwrap().item = None,
            2 => {
                state.creatures[0].loot.as_mut().unwrap().item =
                    Some(ItemStack::new(FUR, 2).unwrap())
            }
            3 => state.creatures[0].tapped_by = None,
            4 => state.creatures[0].tapped_by = Some(u32::MAX),
            5 => state.tick += u64::from(mmorpg_core::unit::CORPSE_TICKS),
            6 => {
                state.creatures[0].life = CreatureLife::Alive;
                state.creatures[0].health = 1;
            }
            _ => unreachable!(),
        }
        assert!(
            ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn binding_and_generating_loot_leave_combat_ai_physics_and_their_rng_unchanged() {
    let mut baseline = fixture(None);
    let mut candidate = fixture(Some(mmorpg_core::loot_table(WOLF).unwrap().clone()));
    for zone in [&mut baseline, &mut candidate] {
        command(zone, 1, ZoneCommand::SelectTarget(Some(TARGET)));
        command(zone, 1, ZoneCommand::StartAttack);
    }
    let mut saw_corpse = false;
    for _ in 0..240 {
        baseline.advance_tick().unwrap();
        candidate.advance_tick().unwrap();
        let expected = baseline.snapshot().unwrap();
        let mut actual = candidate.snapshot().unwrap();
        saw_corpse |= actual.creatures[0].loot.is_some();
        actual.content_fingerprint = expected.content_fingerprint;
        actual.loot_rng_state = expected.loot_rng_state;
        for creature in &mut actual.creatures {
            creature.loot = None;
        }
        assert_eq!(actual, expected);
    }
    assert!(saw_corpse);
}
