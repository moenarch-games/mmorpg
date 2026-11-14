//! Timed swings and clashes in an arena (#679, Atlas 5.2.i679): a directed strike winds up
//! visibly, sweeps over its active ticks, clashes with another swing or a projectile, and a
//! zone restored mid-swing continues exactly.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    ClashKind, EntityRef, PlayerCombatPolicy, PlayerId, StrikeAim, StrikeHeight, StrikeSide,
    SwingPhase, TimedCombat, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};

const EAST: u16 = 16_384;
const WEST: u16 = 49_152;
const MIDDLE: StrikeAim = StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle);

fn arena(timed: bool) -> Arc<ZoneContent> {
    let content = ZoneContent::new(
        support::arena::ground(),
        mmorpg_core::ZoneAreas::default(),
        vec![],
        vec![],
        vec![],
        [0, 0],
    )
    .unwrap()
    .with_creature_abilities(vec![])
    .unwrap()
    .with_player_combat(PlayerCombatPolicy::FreeForAll);
    Arc::new(if timed {
        content.with_timed_combat(TimedCombat::ARENA).unwrap()
    } else {
        content
    })
}

/// Two players of `classes`, `gap` units apart on X, player 1 facing east toward player 2.
fn duel(content: &Arc<ZoneContent>, classes: [u8; 2], gap: i32) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(content)).unwrap();
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
        player.position = mmorpg_core::physics::from_units([index as i32 * gap, 90, -2_000]);
        player.combat.level = 10;
        player.combat.abilities.resource = 100;
        player.facing = if index == 0 { EAST } else { WEST };
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(content)).unwrap()
}

fn strike(zone: &mut ZoneSimulation, player: PlayerId, sequence: u32, facing: u16) {
    zone.apply_command(
        player,
        sequence,
        ZoneCommand::DirectedStrike {
            ability: 1,
            facing,
            aim: MIDDLE,
        },
    )
    .unwrap();
}

fn health(zone: &ZoneSimulation, player: PlayerId) -> u32 {
    zone.snapshot_for_player(player).unwrap().viewer.health
}

#[test]
fn swing_begin_reports_its_accepted_heading_after_a_later_turn() {
    let mut zone = duel(&arena(true), [0, 0], 100);
    strike(&mut zone, 1, 2, EAST);
    zone.advance_tick().unwrap();
    zone.apply_command(
        1,
        3,
        ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing: WEST,
        },
    )
    .unwrap();
    let view = zone.snapshot_for_player(1).unwrap();
    assert_eq!(
        view.entities
            .iter()
            .find(|entity| entity.entity() == EntityRef::Player(1))
            .unwrap()
            .facing,
        WEST
    );
    assert_eq!(zone.swing_of(1).unwrap().facing, EAST);
    assert!(
        view.events
            .iter()
            .any(|event| matches!(event, ZoneEvent::SwingBegan { facing: EAST, .. }))
    );
}

#[test]
fn greyhaven_adopts_the_approved_timing_and_recovers_a_running_swing() {
    let content = mmorpg_core::greyhaven_vale::content();
    assert_eq!(content.timed_combat(), Some(TimedCombat::ARENA));
    assert_eq!(content.player_combat_policy(), PlayerCombatPolicy::Safe);
    let mut zone = duel(&content, [0, 0], 100);
    strike(&mut zone, 1, 2, EAST);
    zone.advance_tick().unwrap();
    assert!(zone.swing_of(1).is_some());
    let mut restored = ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), content).unwrap();
    for _ in 0..40 {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
}

#[test]
fn a_timed_swing_winds_up_visibly_then_strikes_and_restores_mid_swing_exactly() {
    let content = arena(true);
    let mut zone = duel(&content, [0, 0], 100);
    let before = health(&zone, 2);
    let timed = zone.timed_combat().unwrap();
    strike(&mut zone, 1, 2, EAST);
    zone.advance_tick().unwrap();
    // The opponent sees the wind-up coming and is not struck yet.
    let seen = zone.snapshot_for_player(2).unwrap();
    assert!(seen.events.iter().any(|event| matches!(
        event,
        ZoneEvent::SwingBegan {
            source: EntityRef::Player(1),
            aim: MIDDLE,
            ..
        }
    )));
    assert_eq!(seen.viewer.health, before);
    let swing = zone.swing_of(1).unwrap();
    assert_eq!(swing.phase(), SwingPhase::Startup);
    assert_eq!(swing.startup, timed.startup_ticks());

    // Mid-wind-up the zone is recovered from its canonical snapshot; both continue alike.
    for _ in 0..timed.startup_ticks() / 2 {
        zone.advance_tick().unwrap();
    }
    let snapshot = zone.snapshot().unwrap();
    assert!(snapshot.players[0].combat.swing.is_some());
    // Content without the timed rules refuses the running swing.
    assert!(ZoneSimulation::from_snapshot(snapshot.clone(), arena(false)).is_err());
    let mut restored = ZoneSimulation::from_snapshot(snapshot, Arc::clone(&content)).unwrap();
    let mut struck_at = None;
    for tick in 0..(timed.startup_ticks() + timed.active_ticks() + 30) {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
        if struck_at.is_none() && health(&zone, 2) < before {
            struck_at = Some(tick);
        }
    }
    let struck_at = struck_at.expect("the sweep struck the opponent");
    assert!(
        struck_at >= timed.startup_ticks() / 2,
        "no damage during the wind-up"
    );
    assert!(zone.swing_of(1).is_none());
}

#[test]
fn a_swing_that_covers_nobody_whiffs_on_its_last_active_tick() {
    let content = arena(true);
    let mut zone = duel(&content, [0, 0], 100);
    let timed = zone.timed_combat().unwrap();
    // Swinging away from the opponent.
    strike(&mut zone, 1, 2, WEST);
    let mut whiffed = None;
    for tick in 1..=(timed.startup_ticks() + timed.active_ticks() + 2) {
        zone.advance_tick().unwrap();
        let events = zone.snapshot_for_player(1).unwrap().events;
        if events.iter().any(|event| {
            matches!(
                event,
                ZoneEvent::StrikeSwept {
                    source: EntityRef::Player(1),
                    target: None,
                    ..
                }
            )
        }) {
            whiffed = Some(tick);
        }
    }
    assert_eq!(whiffed, Some(timed.startup_ticks() + timed.active_ticks()));
    assert!(zone.swing_of(1).is_none());
    let state = zone.snapshot().unwrap();
    assert_eq!(state.players[1].combat.health, health(&zone, 2));
}

#[test]
fn two_swings_that_meet_clash_without_damage_and_both_recover() {
    let content = arena(true);
    let mut zone = duel(&content, [0, 0], 150);
    let before = [health(&zone, 1), health(&zone, 2)];
    strike(&mut zone, 1, 2, EAST);
    strike(&mut zone, 2, 2, WEST);
    let timed = zone.timed_combat().unwrap();
    let mut clashed = false;
    for _ in 0..(timed.startup_ticks() + timed.active_ticks() + 5) {
        zone.advance_tick().unwrap();
        clashed |= zone
            .snapshot_for_player(1)
            .unwrap()
            .events
            .iter()
            .any(|event| {
                *event
                    == ZoneEvent::Clashed {
                        source: EntityRef::Player(1),
                        target: EntityRef::Player(2),
                        kind: ClashKind::Swings,
                    }
            });
    }
    assert!(clashed);
    assert_eq!([health(&zone, 1), health(&zone, 2)], before);
    assert!(zone.swing_of(1).is_none() && zone.swing_of(2).is_none());

    // Without timed combat the same strikes trade blows at once.
    let mut immediate = duel(&arena(false), [0, 0], 150);
    strike(&mut immediate, 1, 2, EAST);
    strike(&mut immediate, 2, 2, WEST);
    immediate.advance_tick().unwrap();
    assert!(health(&immediate, 1) < before[0] && health(&immediate, 2) < before[1]);
}

/// Every free projectile's ID in the zone's projection for player 1.
fn flying(zone: &ZoneSimulation) -> usize {
    zone.snapshot_for_player(1)
        .unwrap()
        .projectiles
        .iter()
        .filter(|projectile| !projectile.stuck && !projectile.burst)
        .count()
}

#[test]
fn free_or_homing_firebolts_and_arrows_clash_only_in_a_timed_arena() {
    for (timed, class, ability, targeted, permitted) in [
        (true, 2, 9, false, true),
        (false, 2, 9, false, true),
        (true, 1, 5, false, true),
        (false, 1, 5, false, true),
        (true, 2, 9, true, true),
        (true, 2, 9, true, false),
        (true, 1, 5, true, true),
    ] {
        let mut content = arena(timed);
        if timed && !permitted {
            let mut rules = TimedCombat::ARENA;
            rules.clashes.firebolt_interception.firebolts = false;
            content = Arc::new((*content).clone().with_timed_combat(rules).unwrap());
        }
        let mut zone = duel(&content, [class, class], 1_200);
        let before = [health(&zone, 1), health(&zone, 2)];
        for player in [1, 2] {
            zone.apply_command(
                player,
                2,
                ZoneCommand::UseAbility {
                    ability,
                    target: targeted.then_some(EntityRef::Player(3 - player)),
                },
            )
            .unwrap();
        }
        let mut clashed = false;
        let mut shot = false;
        for _ in 0..mmorpg_core::golden::t30(150) {
            zone.advance_tick().unwrap();
            shot |= flying(&zone) > 0;
            clashed |= zone
                .snapshot_for_player(2)
                .unwrap()
                .events
                .iter()
                .any(|event| {
                    matches!(
                        event,
                        ZoneEvent::Clashed {
                            kind: ClashKind::Projectiles,
                            ..
                        }
                    )
                });
        }
        assert!(shot, "both casts released missiles");
        assert_eq!(clashed, timed && permitted);
        let after = [health(&zone, 1), health(&zone, 2)];
        if timed && permitted {
            assert_eq!(after, before);
        } else {
            assert!(after[0] < before[0] && after[1] < before[1]);
        }
    }
}

#[test]
fn ordinary_swords_parry_arrows_but_not_free_or_homing_firebolts() {
    for (class, ability, targeted, parry) in [
        (1, 5, false, true),
        (1, 5, true, true),
        (2, 9, false, false),
        (2, 9, true, false),
    ] {
        let content = arena(true);
        // The Warden faces the shooter 20 m east.
        let mut zone = duel(&content, [0, class], 2_000);
        let before = health(&zone, 1);
        zone.apply_command(
            2,
            2,
            ZoneCommand::UseAbility {
                ability,
                target: targeted.then_some(EntityRef::Player(1)),
            },
        )
        .unwrap();
        let timed = zone.timed_combat().unwrap();
        let lead = i32::from(timed.startup_ticks() + timed.active_ticks() / 2);
        let speed = if class == 1 {
            mmorpg_core::ARROW_SPEED_UNITS
        } else {
            mmorpg_core::MISSILE_SPEED_UNITS
        };
        let mut swung = false;
        let mut parried = false;
        for _ in 0..mmorpg_core::golden::t30(150) {
            zone.advance_tick().unwrap();
            let view = zone.snapshot_for_player(1).unwrap();
            if !swung
                && let Some(missile) = view.projectiles.first()
                && (missile.position[0] as f32) < speed * lead as f32 + 100.0
            {
                strike(&mut zone, 1, 2, EAST);
                swung = true;
            }
            parried |= view.events.iter().any(|event| {
                *event
                    == ZoneEvent::Clashed {
                        source: EntityRef::Player(1),
                        target: EntityRef::Player(2),
                        kind: ClashKind::Parry,
                    }
            });
        }
        assert!(swung);
        assert_eq!(parried, parry);
        if parry {
            assert_eq!(health(&zone, 1), before);
        } else {
            assert!(health(&zone, 1) < before);
        }
    }
}

#[test]
fn recovery_is_bound_to_interception_permissions() {
    let content = arena(true);
    let zone = duel(&content, [0, 2], 2_000);
    let mut rules = TimedCombat::ARENA;
    rules.clashes.firebolt_interception.arrows = true;
    let changed = Arc::new((*content).clone().with_timed_combat(rules).unwrap());
    assert!(ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), changed).is_err());
}
