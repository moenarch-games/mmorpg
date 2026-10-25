//! #27 independent acceptance: presentation facts follow real combat and interest,
//! without converting participant feedback into observer credit or adding a resolver.
mod support;
use mmorpg_core::{
    CreatureId, EntityRef, INTEREST_RADIUS_UNITS, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
};
use std::sync::Arc;
use support::arena;
const PLAYER: EntityRef = EntityRef::Player(1);
const WOLF: EntityRef = EntityRef::Creature(CreatureId::new(1));

fn arena() -> ZoneSimulation {
    let content = arena::arena(
        vec![arena::wolf(10_000, [1, 1])],
        vec![arena::spawn(1, arena::WOLF, [-180, 0])],
        vec![],
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), Arc::clone(&content)).unwrap();
    for id in 1..=3 {
        zone.add_player(id).unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = mmorpg_core::physics::from_units([0, 90, 0]);
    state.players[0].combat.level = 6;
    state.players[0].combat.health = mmorpg_core::unit::player_max_health(6);
    state.players[1].position = mmorpg_core::physics::from_units([500, 90, 0]);
    state.players[2].position =
        mmorpg_core::physics::from_units([INTEREST_RADIUS_UNITS + 5_000, 90, 0]);
    ZoneSimulation::from_snapshot(state, content).unwrap()
}
fn fight(zone: &mut ZoneSimulation) {
    zone.apply_command(1, 1, ZoneCommand::SelectTarget(Some(WOLF)))
        .unwrap();
    zone.apply_command(1, 2, ZoneCommand::StartAttack).unwrap();
}
#[test]
fn ordinary_attempts_reach_visible_bystanders_once_without_participant_damage_credit() {
    let mut zone = arena();
    fight(&mut zone);
    let mut hits = 0;
    let mut misses = 0;
    let mut player_attempt_ticks = Vec::new();
    let mut creature_attempts = 0;
    for _ in 0..mmorpg_core::golden::t30(2_400) {
        zone.advance_tick().unwrap();
        let own = zone.snapshot_for_player(1).unwrap();
        let observer = zone.snapshot_for_player(2).unwrap();
        let distant = zone.snapshot_for_player(3).unwrap();
        let attempt = ZoneEvent::AttackPerformed {
            source: PLAYER,
            target: WOLF,
        };
        let own_count = own.events.iter().filter(|event| **event == attempt).count();
        let other_count = observer
            .events
            .iter()
            .filter(|event| **event == attempt)
            .count();
        assert_eq!(
            own_count,
            other_count,
            "visible observer at tick {}",
            zone.current_tick()
        );
        assert!(own_count <= 1);
        if own_count == 1 {
            player_attempt_ticks.push(zone.current_tick());
        }
        let outcome = own
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    ZoneEvent::DamageDealt {
                        source: PLAYER,
                        target: WOLF,
                        ..
                    } | ZoneEvent::Miss {
                        source: PLAYER,
                        target: WOLF
                    }
                )
            })
            .count();
        assert_eq!(
            own_count, outcome,
            "only accepted ordinary attempts in this ability-free fixture"
        );
        for event in &own.events {
            match *event {
                ZoneEvent::DamageDealt {
                    source: PLAYER,
                    target: WOLF,
                    amount,
                    critical,
                } => {
                    hits += 1;
                    assert_eq!(
                        observer
                            .events
                            .iter()
                            .filter(|event| **event
                                == ZoneEvent::DamageObserved {
                                    source: PLAYER,
                                    target: WOLF,
                                    amount,
                                    critical
                                })
                            .count(),
                        1
                    );
                }
                ZoneEvent::Miss {
                    source: PLAYER,
                    target: WOLF,
                } => {
                    misses += 1;
                    assert!(!observer.events.iter().any(|event| matches!(
                        event,
                        ZoneEvent::DamageObserved {
                            source: PLAYER,
                            target: WOLF,
                            ..
                        }
                    )));
                }
                _ => {}
            }
        }
        creature_attempts += observer
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    ZoneEvent::AttackPerformed {
                        source: WOLF,
                        target: PLAYER
                    }
                )
            })
            .count();
        assert!(
            !observer.events.iter().any(|event| matches!(
                event,
                ZoneEvent::DamageDealt { .. } | ZoneEvent::DamageTaken { .. }
            )),
            "observer must not inherit participant-relative HUD events"
        );
        assert!(!distant.events.iter().any(|event| matches!(
            event,
            ZoneEvent::AttackPerformed { .. }
                | ZoneEvent::DamageObserved { .. }
                | ZoneEvent::CastStarted { .. }
                | ZoneEvent::AbilityUsed { .. }
        )));
        assert!(!observer.viewer.in_combat && !observer.viewer.auto_attacking);
    }
    assert!(
        hits > 0 && misses > 0,
        "seed exercises hit and miss: {hits}/{misses}"
    );
    assert!(
        creature_attempts > 0,
        "actual creature ordinary attacks are observable too"
    );
    assert!(
        player_attempt_ticks
            .windows(2)
            .all(|pair| pair[1] - pair[0] == u64::from(mmorpg_core::unit::PLAYER_SWING_TICKS))
    );
}
#[test]
fn waiting_and_out_of_range_do_not_create_ordinary_attempts() {
    let mut zone = arena();
    zone.advance_tick().unwrap();
    assert!(
        !zone
            .snapshot_for_player(2)
            .unwrap()
            .events
            .iter()
            .any(|event| matches!(event, ZoneEvent::AttackPerformed { source: PLAYER, .. }))
    );
    let content = Arc::clone(zone.content());
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = mmorpg_core::physics::from_units([0, 90, -3_000]);
    let mut zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    fight(&mut zone);
    for _ in 0..mmorpg_core::golden::t30(10) {
        zone.advance_tick().unwrap();
        assert!(
            !zone
                .snapshot_for_player(1)
                .unwrap()
                .events
                .iter()
                .any(|event| matches!(event, ZoneEvent::AttackPerformed { source: PLAYER, .. }))
        );
    }
}
#[test]
fn recovered_combat_reproduces_observations_and_canonical_gameplay() {
    let mut zone = arena();
    fight(&mut zone);
    zone.advance_tick().unwrap();
    let mut recovered =
        ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()))
            .unwrap();
    for _ in 0..mmorpg_core::golden::t30(180) {
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), recovered.snapshot().unwrap());
        for id in 1..=3 {
            assert_eq!(
                zone.snapshot_for_player(id).unwrap(),
                recovered.snapshot_for_player(id).unwrap()
            );
        }
    }
}
#[test]
fn observers_see_existing_creature_casts_without_acquiring_player_feedback() {
    use mmorpg_core::ability::ids;
    use support::classes;
    for (template, ability) in [
        (classes::lurker(500, [1, 1]), ids::MUCK_BOLT),
        (classes::bandit(500, [1, 1]), ids::CRUDE_BANDAGE),
    ] {
        let template_id = template.id;
        let content = classes::arena(
            vec![template],
            vec![arena::spawn(1, template_id, [-180, 0])],
        );
        let mut zone = ZoneSimulation::with_content(ZoneId::new(5), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        let mut state = zone.snapshot().unwrap();
        state.players[1].position = mmorpg_core::physics::from_units([500, 90, 0]);
        if ability == ids::CRUDE_BANDAGE {
            state.creatures[0].health = 100;
        }
        let mut zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        let mut started = false;
        let mut resolved = false;
        for _ in 0..mmorpg_core::golden::t30(150) {
            zone.advance_tick().unwrap();
            let own = zone.snapshot_for_player(1).unwrap();
            let other = zone.snapshot_for_player(2).unwrap();
            for event in &own.events {
                if matches!(event, ZoneEvent::CastStarted { ability: id, .. } | ZoneEvent::AbilityUsed { ability: id, .. } if *id == ability)
                {
                    assert!(
                        other.events.contains(event),
                        "visible bystander receives {event:?}"
                    );
                }
            }
            started |= other.events.iter().any(|event| matches!(event, ZoneEvent::CastStarted { ability: id, .. } if *id == ability));
            resolved |= other.events.iter().any(|event| matches!(event, ZoneEvent::AbilityUsed { ability: id, .. } if *id == ability));
            assert!(!other.events.iter().any(|event| matches!(
                event,
                ZoneEvent::DamageTaken { .. } | ZoneEvent::DamageDealt { .. }
            )));
        }
        assert!(
            started && resolved,
            "existing creature ability {ability:?} plays start and resolution"
        );
    }
}
#[test]
fn partially_visible_interaction_does_not_disclose_hidden_actor_identity() {
    let mut zone = arena();
    let mut state = zone.snapshot().unwrap();
    // Wolf is inside viewer2's radius, source player1 outside it.
    state.players[1].position =
        mmorpg_core::physics::from_units([-INTEREST_RADIUS_UNITS - 100, 90, 0]);
    zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    fight(&mut zone);
    zone.advance_tick().unwrap();
    let other = zone.snapshot_for_player(2).unwrap();
    assert!(
        !other
            .entities
            .iter()
            .any(|entity| entity.entity() == PLAYER)
    );
    assert!(!other.events.iter().any(|event| matches!(
        event,
        ZoneEvent::AttackPerformed { source: PLAYER, .. }
            | ZoneEvent::DamageObserved { source: PLAYER, .. }
    )));
}
#[test]
fn saturated_cosmetic_observations_do_not_displace_damage_feedback() {
    let mut template = arena::wolf(100, [1, 1]);
    template.half_extents = [1, 1, 1];
    let spawns = (1..=32)
        .map(|id| {
            arena::spawn(
                id,
                arena::WOLF,
                [-80 - (id as i32 % 8) * 10, -40 + (id as i32 / 8) * 10],
            )
        })
        .collect();
    let content = arena::arena(vec![template], spawns, vec![]);
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), content).unwrap();
    zone.add_player(1).unwrap();
    zone.advance_tick().unwrap();
    let view = zone.snapshot_for_player(1).unwrap();
    assert_eq!(view.events.len(), mmorpg_core::MAX_EVENTS_PER_PLAYER);
    assert!(
        view.events
            .iter()
            .any(|event| matches!(event, ZoneEvent::DamageTaken { .. }))
    );
    assert!(
        !view.events.iter().any(|event| matches!(
            event,
            ZoneEvent::AttackPerformed { .. } | ZoneEvent::DamageObserved { .. }
        )),
        "cosmetic events lose to participant damage/death under saturation"
    );
}
#[test]
fn visible_remote_player_ability_has_start_resolution_and_neutral_impact() {
    use mmorpg_core::ability::ids;
    use support::classes;
    let content = classes::arena(
        vec![arena::wolf(500, [1, 1])],
        vec![arena::spawn(1, arena::WOLF, [-200, 0])],
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), content).unwrap();
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 2, sex: 0 })
        .unwrap();
    zone.advance_tick().unwrap();
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: ids::FIREBOLT.get(),
            target: Some(WOLF),
        },
    )
    .unwrap();
    let mut started = false;
    let mut used = false;
    let mut impact = false;
    for _ in 0..mmorpg_core::golden::t30(65) {
        zone.advance_tick().unwrap();
        let observer = zone.snapshot_for_player(2).unwrap();
        started |= observer.events.iter().any(|e| matches!(e, ZoneEvent::CastStarted { source: PLAYER, ability, .. } if *ability == ids::FIREBOLT));
        used |= observer.events.iter().any(|e| matches!(e, ZoneEvent::AbilityUsed { source: PLAYER, ability, .. } if *ability == ids::FIREBOLT));
        impact |= observer.events.iter().any(|e| {
            matches!(
                e,
                ZoneEvent::DamageObserved {
                    source: PLAYER,
                    target: WOLF,
                    amount: 1..,
                    ..
                }
            )
        });
        assert!(!observer.events.iter().any(|e| matches!(
            e,
            ZoneEvent::DamageDealt { .. } | ZoneEvent::DamageTaken { .. }
        )));
    }
    assert!(started && used && impact);
}
#[test]
fn observation_does_not_name_actors_omitted_by_projection_entity_cap() {
    let content = arena::arena(
        vec![arena::wolf(1000, [1, 1])],
        vec![arena::spawn(1, arena::WOLF, [-180, 0])],
        vec![],
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), Arc::clone(&content)).unwrap();
    for id in 1..=70 {
        zone.add_player(id).unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = mmorpg_core::physics::from_units([0, 90, 0]);
    for (index, player) in state.players.iter_mut().enumerate().skip(1) {
        player.position = mmorpg_core::physics::from_units([
            -4000 + (index as i32 % 8) * 100,
            90,
            (index as i32 / 8) * 100,
        ]);
    }
    zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    fight(&mut zone);
    zone.advance_tick().unwrap();
    let own = zone.snapshot_for_player(1).unwrap();
    assert!(own.events.contains(&ZoneEvent::AttackPerformed {
        source: PLAYER,
        target: WOLF
    }));
    let observer = zone.snapshot_for_player(2).unwrap();
    assert_eq!(observer.entities.len(), mmorpg_core::MAX_VISIBLE_ENTITIES);
    assert!(
        !observer
            .entities
            .iter()
            .any(|entity| entity.entity() == PLAYER)
    );
    assert!(
        !observer.events.iter().any(|event| matches!(
            event,
            ZoneEvent::AttackPerformed { source: PLAYER, .. }
                | ZoneEvent::DamageObserved { source: PLAYER, .. }
        )),
        "radius visibility alone is insufficient after projection truncation"
    );
}
