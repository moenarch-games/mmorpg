use std::sync::Arc;

use mmorpg_core::{
    CreatureTemplateId, ItemId, LootOutcome, LootTable, MAX_CREATURE_TEMPLATES, ZoneCommand,
    ZoneContent, ZoneDefinition, ZoneId, ZoneSimulation, greyhaven_vale,
};

const WOLF: CreatureTemplateId = CreatureTemplateId::new(1);
const BOAR: CreatureTemplateId = CreatureTemplateId::new(2);
const FUR: ItemId = ItemId::new(1);

fn table(money: [u32; 2], weight: u16, item: ItemId, quantity: [u16; 2]) -> LootTable {
    LootTable::new(
        money,
        &[
            LootOutcome::Nothing { weight: 3 },
            LootOutcome::Item {
                weight,
                item,
                quantity,
            },
        ],
    )
    .unwrap()
}

fn rule() -> LootTable {
    table([1, 3], 2, FUR, [1, 2])
}

// Frozen pre-activation identity, derived from the unchanged hosted geometry/units.
fn unbound() -> ZoneContent {
    let current = greyhaven_vale::content();
    let definition = current.definition();
    ZoneContent::new(
        ZoneDefinition::with_spawn_grid(
            4,
            definition.gravity(),
            definition.spawn_grid(),
            definition.colliders().to_vec(),
        )
        .unwrap(),
        current.areas().clone(),
        current.creature_templates().to_vec(),
        current.creature_spawns().to_vec(),
        current.npcs().to_vec(),
        current.graveyard(),
    )
    .unwrap()
    .with_rng_seed(current.rng_seed())
}

fn bound(revision: u64, tables: Vec<(CreatureTemplateId, LootTable)>) -> ZoneContent {
    unbound().with_loot_tables(revision, tables).unwrap()
}

#[test]
fn binding_orders_template_ids_and_keeps_missing_templates_explicit() {
    let first = bound(1, vec![(BOAR, rule()), (WOLF, rule())]);
    let second = bound(1, vec![(WOLF, rule()), (BOAR, rule())]);
    assert_eq!(first, second);
    assert_eq!(first.loot_revision(), 1);
    assert_eq!(
        first
            .loot_tables()
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        [WOLF, BOAR]
    );
    assert_eq!(first.loot_table(WOLF), Some(&rule()));
    let reseeded = first.clone().with_rng_seed(7);
    assert_eq!(reseeded.loot_tables(), first.loot_tables());
    assert_eq!(reseeded.loot_revision(), first.loot_revision());
    assert_eq!(reseeded.rng_seed(), 7);
    assert_ne!(reseeded.fingerprint(), first.fingerprint());
    assert_eq!(
        reseeded,
        unbound()
            .with_rng_seed(7)
            .with_loot_tables(1, vec![(WOLF, rule()), (BOAR, rule())])
            .unwrap(),
    );
    assert!(first.loot_table(CreatureTemplateId::new(3)).is_none());
    assert!(
        first
            .loot_table(CreatureTemplateId::new(u16::MAX))
            .is_none()
    );
    let empty = bound(7, vec![]);
    assert_eq!(empty.loot_revision(), 7);
    assert!(empty.loot_tables().is_empty());
    assert_ne!(empty.fingerprint(), unbound().fingerprint());
}

#[test]
fn malformed_bindings_fail_closed() {
    for (revision, tables) in [
        (0, vec![(WOLF, rule())]),
        (0, vec![]),
        (1, vec![(WOLF, rule()), (WOLF, rule())]),
        (1, vec![(CreatureTemplateId::new(u16::MAX), rule())]),
        (1, vec![(WOLF, rule()); MAX_CREATURE_TEMPLATES + 1]),
    ] {
        assert!(unbound().with_loot_tables(revision, tables).is_err());
    }
}

#[test]
fn every_authored_reward_field_changes_identity_and_refuses_old_recovery() {
    let original = bound(1, vec![(WOLF, rule())]);
    assert_eq!(original.fingerprint(), 0xd27d_e1e5_e1a8_2192);
    let mut zone =
        ZoneSimulation::with_content(ZoneId::new(1), Arc::new(original.clone())).unwrap();
    zone.add_player(1).unwrap();
    zone.advance_tick().unwrap();
    let snapshot = zone.snapshot().unwrap();
    let mut reordered = rule().outcomes().to_vec();
    reordered.reverse();
    let variants = [
        bound(2, vec![(WOLF, rule())]),
        bound(1, vec![(BOAR, rule())]),
        bound(1, vec![]),
        bound(1, vec![(WOLF, rule()), (BOAR, rule())]),
        bound(1, vec![(WOLF, table([0, 3], 2, FUR, [1, 2]))]),
        bound(1, vec![(WOLF, table([1, 4], 2, FUR, [1, 2]))]),
        bound(1, vec![(WOLF, table([1, 3], 1, FUR, [1, 2]))]),
        bound(1, vec![(WOLF, table([1, 3], 2, ItemId::new(2), [1, 1]))]),
        bound(1, vec![(WOLF, table([1, 3], 2, FUR, [2, 2]))]),
        bound(1, vec![(WOLF, table([1, 3], 2, FUR, [1, 3]))]),
        bound(
            1,
            vec![(
                WOLF,
                LootTable::new([1, 3], &[LootOutcome::Nothing { weight: 3 }]).unwrap(),
            )],
        ),
        bound(1, vec![(WOLF, LootTable::new([1, 3], &reordered).unwrap())]),
        bound(
            1,
            vec![(
                WOLF,
                LootTable::new(
                    [1, 3],
                    &[
                        LootOutcome::Nothing { weight: 4 },
                        LootOutcome::Item {
                            weight: 2,
                            item: FUR,
                            quantity: [1, 2],
                        },
                    ],
                )
                .unwrap(),
            )],
        ),
        bound(
            1,
            vec![(
                WOLF,
                LootTable::new(
                    [1, 3],
                    &[
                        LootOutcome::Nothing { weight: 3 },
                        LootOutcome::Nothing { weight: 2 },
                    ],
                )
                .unwrap(),
            )],
        ),
    ];
    for changed in variants {
        assert_ne!(changed.fingerprint(), original.fingerprint());
        assert!(ZoneSimulation::from_snapshot(snapshot.clone(), Arc::new(changed)).is_err());
    }
    // Isolate item identity from the dagger's smaller quantity limit.
    assert_ne!(
        bound(1, vec![(WOLF, table([1, 3], 2, FUR, [1, 1]))]).fingerprint(),
        bound(1, vec![(WOLF, table([1, 3], 2, ItemId::new(2), [1, 1]))]).fingerprint(),
    );
    assert!(ZoneSimulation::from_snapshot(snapshot, Arc::new(original)).is_ok());
}

#[test]
fn opt_in_binding_preserves_live_content_and_complete_simulation_continuation() {
    let current = Arc::new(unbound());
    assert_eq!(current.revision(), 4);
    assert_eq!(current.fingerprint(), 0xcaa7_cf4a_ff98_b6fd);
    assert_eq!(current.loot_revision(), 0);
    assert!(current.loot_tables().is_empty());
    let authored = Arc::new(bound(1, vec![(WOLF, rule())]));
    assert_eq!(current.rng_seed(), authored.rng_seed());
    let mut baseline = ZoneSimulation::with_content(ZoneId::new(1), current).unwrap();
    let mut candidate = ZoneSimulation::with_content(ZoneId::new(1), authored).unwrap();
    baseline.add_player(1).unwrap();
    candidate.add_player(1).unwrap();
    for sequence in 1..=360 {
        let command = ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 0,
        };
        baseline.apply_command(1, sequence, command).unwrap();
        candidate.apply_command(1, sequence, command).unwrap();
        baseline.advance_tick().unwrap();
        candidate.advance_tick().unwrap();
        let expected = baseline.snapshot().unwrap();
        let mut actual = candidate.snapshot().unwrap();
        actual.content_fingerprint = expected.content_fingerprint;
        assert_eq!(
            actual, expected,
            "tick {sequence}: only content identity may differ"
        );
        assert_eq!(
            baseline.snapshot_for_player(1).unwrap(),
            candidate.snapshot_for_player(1).unwrap()
        );
    }
}
