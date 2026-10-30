//! Explicit arena policy stays outside ordinary safe-world combat.
mod support;
use mmorpg_core::{PlayerCombatPolicy, ZoneContent, ZoneId, ZoneSimulation, greyhaven_vale};
use std::sync::Arc;

#[test]
fn arena_policy_is_explicit_and_bound_to_recovery_content() {
    let safe = greyhaven_vale::content();
    assert_eq!(safe.player_combat_policy(), PlayerCombatPolicy::Safe);
    let arena = Arc::new(
        Arc::unwrap_or_clone(Arc::clone(&safe)).with_player_combat(PlayerCombatPolicy::FreeForAll),
    );
    assert_ne!(safe.revision(), arena.revision());
    let zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&arena)).unwrap();
    let snapshot = zone.snapshot().unwrap();
    assert!(ZoneSimulation::from_snapshot(snapshot.clone(), Arc::clone(&safe)).is_err());
    ZoneSimulation::from_snapshot(snapshot, arena).unwrap();
    let restored_safe: ZoneContent = Arc::unwrap_or_clone(safe);
    assert_eq!(
        restored_safe
            .clone()
            .with_player_combat(PlayerCombatPolicy::FreeForAll)
            .with_player_combat(PlayerCombatPolicy::Safe),
        restored_safe
    );
}

#[test]
fn an_explicit_arena_weapon_ability_uses_shared_player_damage_and_safe_world_refuses() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    for policy in [PlayerCombatPolicy::Safe, PlayerCombatPolicy::FreeForAll] {
        let mut zone = two_players(policy);
        let before = zone.snapshot_for_player(2).unwrap().viewer.health;
        assert_eq!(
            zone.players_are_hostile(1, 2),
            policy == PlayerCombatPolicy::FreeForAll
        );
        assert!(!zone.players_are_hostile(1, 1));
        let view = zone.snapshot_for_player(1).unwrap();
        let opponent = view
            .entities
            .iter()
            .find(|entity| entity.entity() == EntityRef::Player(2))
            .unwrap();
        assert_eq!(
            opponent.flags.hostile,
            policy == PlayerCombatPolicy::FreeForAll
        );
        assert_eq!(
            opponent.flags.attackable,
            policy == PlayerCombatPolicy::FreeForAll
        );
        assert!(!view.entities[0].flags.attackable);

        zone.apply_command(
            1,
            2,
            ZoneCommand::UseAbility {
                ability: 1,
                target: Some(EntityRef::Player(2)),
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
        let after = zone.snapshot_for_player(2).unwrap();
        if policy == PlayerCombatPolicy::FreeForAll {
            assert!(after.viewer.health < before);
            assert!(after.events.iter().any(|event| matches!(
                event,
                mmorpg_core::ZoneEvent::DamageTaken {
                    source: EntityRef::Player(1),
                    ..
                }
            )));
        } else {
            assert_eq!(after.viewer.health, before);
            assert_eq!(
                zone.snapshot_for_player(1)
                    .unwrap()
                    .viewer
                    .resource
                    .unwrap()
                    .value,
                40
            );
        }
    }
}

fn two_players(policy: PlayerCombatPolicy) -> ZoneSimulation {
    two_classes(policy, [0, 0])
}

fn two_classes(policy: PlayerCombatPolicy, classes: [u8; 2]) -> ZoneSimulation {
    let content =
        Arc::new(Arc::unwrap_or_clone(greyhaven_vale::content()).with_player_combat(policy));
    two_classes_on(content, classes)
}

fn two_classes_on(content: Arc<ZoneContent>, classes: [u8; 2]) -> ZoneSimulation {
    use mmorpg_core::ZoneCommand;
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    for player in [1, 2] {
        zone.add_player(player).unwrap();
        zone.apply_command(
            player,
            1,
            ZoneCommand::ChooseClass {
                class: classes[(player - 1) as usize],
                sex: 0,
            },
        )
        .unwrap();
    }
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    for (index, player) in state.players.iter_mut().enumerate() {
        player.position = mmorpg_core::physics::from_units([index as i32 * 100, 90, -2000]);
        player.combat.abilities.resource = 40;
        // The two face each other (+X and -X), so aimed abilities find the opponent (#517).
        player.facing = if index == 0 { 16_384 } else { 49_152 };
    }
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

#[test]
fn an_arena_weapon_hit_respects_the_existing_raised_shield() {
    use mmorpg_core::{EntityRef, StrikeHeight, ZoneCommand};
    let mut zone = two_players(PlayerCombatPolicy::FreeForAll);
    zone.apply_command(
        2,
        2,
        ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing: 49152,
        },
    )
    .unwrap();
    zone.apply_command(
        2,
        3,
        ZoneCommand::Guard {
            lane: Some(StrikeHeight::High),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let before = zone.snapshot_for_player(2).unwrap().viewer.health;
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: 1,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let after = zone.snapshot_for_player(2).unwrap();
    assert_eq!(after.viewer.health, before);
    assert!(after.events.iter().any(|event| matches!(
        event,
        mmorpg_core::ZoneEvent::Blocked {
            source: EntityRef::Player(1),
            target: EntityRef::Player(2),
            ..
        }
    )));
}

#[test]
fn arena_swings_use_the_existing_swing_clock() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    for policy in [PlayerCombatPolicy::Safe, PlayerCombatPolicy::FreeForAll] {
        let mut zone = two_players(policy);
        let before = zone.snapshot_for_player(2).unwrap().viewer.health;
        zone.apply_command(1, 2, ZoneCommand::SelectTarget(Some(EntityRef::Player(2))))
            .unwrap();
        let mut sequence = 3;
        for _ in 0..mmorpg_core::golden::t30(150) {
            // Swing at the opponent whenever it is in reach (#517).
            if let Some(swing) = support::arena::swing_at(&zone, 1, EntityRef::Player(2)) {
                zone.apply_command(1, sequence, swing).unwrap();
                sequence += 1;
            }
            zone.advance_tick().unwrap();
        }
        let after = zone.snapshot_for_player(2).unwrap();
        assert_eq!(
            after.viewer.health < before,
            policy == PlayerCombatPolicy::FreeForAll
        );
    }
}

#[test]
fn an_arena_firebolt_completes_its_cast_and_hits_through_a_real_projectile() {
    use mmorpg_core::{EntityRef, StrikeHeight, ZoneCommand};
    for guarding in [false, true] {
        let mut zone = two_classes(PlayerCombatPolicy::FreeForAll, [2, 0]);
        if guarding {
            zone.apply_command(
                2,
                2,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 0,
                    facing: 49152,
                },
            )
            .unwrap();
            zone.apply_command(
                2,
                3,
                ZoneCommand::Guard {
                    lane: Some(StrikeHeight::High),
                },
            )
            .unwrap();
            zone.advance_tick().unwrap();
        }
        let before = zone.snapshot_for_player(2).unwrap().viewer.health;
        zone.apply_command(
            1,
            2,
            ZoneCommand::UseAbility {
                ability: 9,
                target: Some(EntityRef::Player(2)),
            },
        )
        .unwrap();
        let mut shot = false;
        let mut damaged = false;
        let mut blocked = false;
        for _ in 0..mmorpg_core::golden::t30(120) {
            zone.advance_tick().unwrap();
            shot |= !zone.snapshot_for_player(1).unwrap().projectiles.is_empty();
            let victim = zone.snapshot_for_player(2).unwrap();
            damaged |= victim.viewer.health < before;
            blocked |= victim.events.iter().any(|event| {
                matches!(
                    event,
                    mmorpg_core::ZoneEvent::Blocked {
                        source: EntityRef::Player(1),
                        ..
                    }
                )
            });
        }
        assert!(shot, "the projectile, not the cast, carries the hit");
        assert_eq!(damaged, !guarding);
        assert_eq!(blocked, guarding);
    }
}

#[test]
fn an_arena_cast_restores_and_continues_identically() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    let mut zone = two_classes(PlayerCombatPolicy::FreeForAll, [2, 0]);
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: 9,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    for _ in 0..mmorpg_core::golden::t30(15) {
        zone.advance_tick().unwrap();
    }
    assert!(zone.snapshot_for_player(1).unwrap().viewer.cast.is_some());
    let mut restored =
        ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()))
            .unwrap();
    for _ in 0..mmorpg_core::golden::t30(100) {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
}

#[test]
fn an_arena_damage_over_time_aura_restores_and_pulses_identically() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    let zone = two_classes(PlayerCombatPolicy::FreeForAll, [1, 0]);
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.level = 2;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: 6,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    for _ in 0..mmorpg_core::golden::t30(30) {
        zone.advance_tick().unwrap();
    }
    let state = zone.snapshot().unwrap();
    assert!(
        state.players[1]
            .combat
            .abilities
            .auras
            .iter()
            .any(|aura| aura.ability.get() == 6)
    );
    let mut restored = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let before = zone.snapshot_for_player(2).unwrap().viewer.health;
    for _ in 0..mmorpg_core::golden::t30(100) {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert!(zone.snapshot_for_player(2).unwrap().viewer.health < before);
}

#[test]
fn directed_arena_strikes_use_the_aimed_guard_lane_and_restore_combo_followup() {
    use mmorpg_core::{EntityRef, StrikeAim, StrikeHeight, StrikeSide, ZoneCommand};
    for guarding in [false, true] {
        let zone = two_players(PlayerCombatPolicy::FreeForAll);
        let mut state = zone.snapshot().unwrap();
        state.players[0].combat.level = 2;
        let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        if guarding {
            zone.apply_command(
                2,
                2,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 0,
                    facing: 49152,
                },
            )
            .unwrap();
            zone.apply_command(
                2,
                3,
                ZoneCommand::Guard {
                    lane: Some(StrikeHeight::Middle),
                },
            )
            .unwrap();
            zone.advance_tick().unwrap();
        }
        let before = zone.snapshot_for_player(2).unwrap().viewer.health;
        zone.apply_command(
            1,
            2,
            ZoneCommand::DirectedStrike {
                ability: 1,
                facing: 16384,
                aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
        let state = zone.snapshot().unwrap();
        assert_eq!(
            zone.snapshot_for_player(2).unwrap().viewer.health < before,
            !guarding
        );
        assert_eq!(state.players[0].combat.combo.is_some(), !guarding);
        if !guarding {
            assert_eq!(
                state.players[0].combat.combo.unwrap().target,
                EntityRef::Player(2)
            );
            let mut restored =
                ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
            for host in [&mut zone, &mut restored] {
                host.apply_command(
                    1,
                    3,
                    ZoneCommand::UseAbility {
                        ability: 2,
                        target: None,
                    },
                )
                .unwrap();
            }
            let mut stunned = false;
            for _ in 0..mmorpg_core::golden::t30(30) {
                zone.advance_tick().unwrap();
                restored.advance_tick().unwrap();
                let state = zone.snapshot().unwrap();
                stunned |= state.players[1]
                    .combat
                    .abilities
                    .auras
                    .iter()
                    .any(|aura| aura.ability.get() == 2);
                assert_eq!(state, restored.snapshot().unwrap());
            }
            assert!(
                stunned,
                "the existing buffered Bash follows the confirmed player target"
            );
        }
    }
}

#[test]
fn area_roots_affect_eligible_opponents_without_harming_the_caster_or_safe_players() {
    use mmorpg_core::ZoneCommand;
    for policy in [PlayerCombatPolicy::Safe, PlayerCombatPolicy::FreeForAll] {
        let zone = two_classes(policy, [2, 0]);
        let mut state = zone.snapshot().unwrap();
        state.players[0].combat.level = 2;
        let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let caster = zone.snapshot_for_player(1).unwrap().viewer.health;
        let victim = zone.snapshot_for_player(2).unwrap().viewer.health;
        zone.apply_command(
            1,
            2,
            ZoneCommand::UseAbility {
                ability: 10,
                target: None,
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
        let state = zone.snapshot().unwrap();
        assert_eq!(state.players[0].combat.health, caster);
        assert!(state.players[0].combat.abilities.auras.is_empty());
        assert_eq!(
            state.players[1].combat.health < victim,
            policy == PlayerCombatPolicy::FreeForAll
        );
        assert_eq!(
            state.players[1]
                .combat
                .abilities
                .auras
                .iter()
                .any(|aura| aura.ability.get() == 10),
            policy == PlayerCombatPolicy::FreeForAll
        );
    }
}

#[test]
fn arena_death_reports_the_player_killer_and_restores_without_pve_rewards() {
    use mmorpg_core::{EntityRef, ZoneCommand, ZoneEvent};
    let zone = two_players(PlayerCombatPolicy::FreeForAll);
    let mut state = zone.snapshot().unwrap();
    state.players[1].combat.health = 1;
    let progress = (state.players[0].combat.experience, state.players[0].copper);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: 1,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let victim = zone.snapshot_for_player(2).unwrap();
    assert!(victim.viewer.dead);
    assert!(victim.events.iter().any(|event| matches!(
        event,
        ZoneEvent::Died {
            entity: EntityRef::Player(2),
            killer: Some(EntityRef::Player(1))
        }
    )));
    let state = zone.snapshot().unwrap();
    assert_eq!(
        (state.players[0].combat.experience, state.players[0].copper),
        progress
    );
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
}

#[test]
fn a_physics_wall_stops_aimed_melee_swings_and_directed_arena_sweeps() {
    use mmorpg_core::{
        EntityRef, StaticCollider, StrikeAim, StrikeHeight, StrikeSide, ZoneAreas, ZoneCommand,
        ZoneDefinition,
    };
    let ground = support::arena::ground();
    let mut colliders = ground.colliders().to_vec();
    colliders.push(StaticCollider {
        id: 2,
        position: [50, 150, -2000],
        half_extents: [5, 150, 400],
    });
    let definition =
        ZoneDefinition::with_spawn_grid(ground.gravity(), ground.spawn_grid(), colliders).unwrap();
    let content = Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![],
            vec![],
            vec![],
            [0, 0],
        )
        .unwrap()
        .with_creature_abilities(vec![])
        .unwrap()
        .with_player_combat(PlayerCombatPolicy::FreeForAll),
    );
    let mut zone = two_classes_on(content, [0, 0]);
    let health = zone.snapshot_for_player(2).unwrap().viewer.health;
    // Facing the opponent across the wall, Heroic Strike finds nobody in front (#517).
    zone.apply_command(1, 2, support::arena::stand(16_384))
        .unwrap();
    zone.apply_command(
        1,
        3,
        ZoneCommand::UseAbility {
            ability: 1,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let attacker = zone.snapshot_for_player(1).unwrap();
    assert_eq!(attacker.viewer.resource.unwrap().value, 40);
    assert!(attacker.events.iter().any(|event| matches!(
        event,
        mmorpg_core::ZoneEvent::Error {
            code: mmorpg_core::ErrorCode::NoTarget,
            ..
        }
    )));
    zone.apply_command(1, 4, ZoneCommand::SelectTarget(Some(EntityRef::Player(2))))
        .unwrap();
    // Swings at it whiff against the wall.
    for sequence in 5..5 + u32::try_from(mmorpg_core::golden::t30(10)).unwrap() {
        zone.apply_command(1, sequence, ZoneCommand::Swing { facing: 16_384 })
            .unwrap();
        zone.advance_tick().unwrap();
    }
    zone.apply_command(
        1,
        100,
        ZoneCommand::DirectedStrike {
            ability: 1,
            facing: 16384,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(zone.snapshot_for_player(2).unwrap().viewer.health, health);
    assert!(zone.snapshot().unwrap().players[0].combat.combo.is_none());
}

#[test]
fn a_charge_at_an_arena_player_restores_and_uses_existing_forced_movement() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    let content = Arc::new(
        Arc::unwrap_or_clone(support::arena::arena(vec![], vec![], vec![]))
            .with_creature_abilities(vec![])
            .unwrap()
            .with_player_combat(PlayerCombatPolicy::FreeForAll),
    );
    let zone = two_classes_on(content, [0, 0]);
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.level = 3;
    state.players[1].position = mmorpg_core::physics::from_units([900, 90, -2000]);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let before = zone.snapshot().unwrap().players[1].position;
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: 17,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let state = zone.snapshot().unwrap();
    assert!(
        state
            .forced
            .iter()
            .any(|(unit, state)| *unit == EntityRef::Player(1) && state.moving.is_some())
    );
    let mut restored = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    for _ in 0..mmorpg_core::golden::t30(40) {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert_ne!(
        zone.snapshot().unwrap().players[1].position,
        before,
        "source={:?} forced={:?}",
        zone.snapshot().unwrap().players[0].position,
        zone.snapshot().unwrap().forced
    );
}

#[test]
fn retirement_during_arena_cast_combo_or_charge_leaves_valid_continuation() {
    use mmorpg_core::{EntityRef, StrikeAim, StrikeHeight, StrikeSide, ZoneCommand};
    for mode in 0..3 {
        let zone = two_classes(
            PlayerCombatPolicy::FreeForAll,
            [if mode == 0 { 2 } else { 0 }, 0],
        );
        let mut state = zone.snapshot().unwrap();
        state.players[0].combat.level = 3;
        if mode == 2 {
            state.players[1].position = mmorpg_core::physics::from_units([900, 90, -2000]);
        }
        let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let command = if mode == 1 {
            ZoneCommand::DirectedStrike {
                ability: 1,
                facing: 16384,
                aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
            }
        } else {
            ZoneCommand::UseAbility {
                ability: if mode == 0 { 9 } else { 17 },
                target: Some(EntityRef::Player(2)),
            }
        };
        zone.apply_command(1, 2, command).unwrap();
        zone.advance_tick().unwrap();
        assert!(zone.remove_player(2));
        let state = zone.snapshot().unwrap();
        // A Firebolt flies freely (#517): its cast names nobody and goes on; nothing else
        // outlives the opponent.
        assert!(
            state.players[0]
                .combat
                .abilities
                .cast
                .is_none_or(|cast| cast.target.is_none())
        );
        assert!(state.players[0].combat.combo.is_none());
        ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    }
}
