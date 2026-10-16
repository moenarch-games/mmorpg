//! Canonical recovery validation through the public zone API: restored state
//! must match its content and stay inside the ranges the simulation can
//! continue from, and recovered extreme player positions never overflow
//! combat or AI arithmetic.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureAi, CreatureId, CreatureLife, CreatureSpawn, CreatureTemplate,
    EntityRef, ErrorCode, MAX_CONTENT_COORDINATE_UNITS, ThreatEntry, ZoneCommand, ZoneContent,
    ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, WOLF};

const HOME: [i32; 2] = [5_000, 5_000];
const WANDER_RADIUS: i32 = 600;

fn wandering_wolf(template: CreatureTemplate) -> Arc<ZoneContent> {
    arena::arena(
        vec![template],
        vec![CreatureSpawn {
            wander_radius: WANDER_RADIUS,
            ..arena::spawn(1, WOLF, HOME)
        }],
        vec![],
    )
}

/// A zone with one admitted player, far from the wolf, and its checkpoint.
fn checkpoint(content: &Arc<ZoneContent>) -> CanonicalZoneSnapshot {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), Arc::clone(content)).unwrap();
    zone.add_player(1).unwrap();
    zone.snapshot().unwrap()
}

fn restore(
    snapshot: CanonicalZoneSnapshot,
    content: &Arc<ZoneContent>,
) -> Result<ZoneSimulation, String> {
    ZoneSimulation::from_snapshot(snapshot, Arc::clone(content))
        .map_err(|error| error.message().to_owned())
}

#[test]
fn recovery_refuses_changed_content_of_the_same_revision() {
    let content = wandering_wolf(arena::wolf(30, [1, 2]));
    let retuned = wandering_wolf(CreatureTemplate {
        swing_ticks: 61,
        ..arena::wolf(30, [1, 2])
    });
    assert_eq!(content.revision(), retuned.revision());
    assert_ne!(content.fingerprint(), retuned.fingerprint());

    let state = checkpoint(&content);
    assert!(restore(state.clone(), &content).is_ok());
    assert_eq!(
        restore(state, &retuned).err().unwrap(),
        "snapshot content identity does not match the supplied zone content"
    );
}

/// Dead units cannot move. A checkpoint from an earlier build may still hold
/// a dead player's movement or jump intent; recovery lets go of it, as death
/// does, so the restored spirit stands.
#[test]
fn recovery_lets_a_dead_player_go_of_legacy_movement_intent() {
    let content = wandering_wolf(arena::wolf(30, [1, 2]));
    let mut dead = checkpoint(&content);
    dead.players[0].combat.health = 0;
    let holding = |forward: i8, strafe: i8, jump_pending: bool| {
        let mut state = dead.clone();
        state.players[0].forward = forward;
        state.players[0].strafe = strafe;
        state.players[0].jump_pending = jump_pending;
        state
    };
    for (forward, strafe, jump) in [(1, 0, false), (0, -1, false), (-1, 1, false), (0, 0, true)] {
        let zone = restore(holding(forward, strafe, jump), &content).unwrap();
        let player = &zone.snapshot().unwrap().players[0];
        assert_eq!(
            (player.forward, player.strafe, player.jump_pending),
            (0, 0, false),
            "{forward} {strafe} {jump}"
        );
    }
    // The living keep both.
    let mut alive = holding(1, -1, true);
    alive.players[0].combat.health = 1;
    let player = restore(alive, &content)
        .unwrap()
        .snapshot()
        .unwrap()
        .players
        .remove(0);
    assert_eq!(
        (player.forward, player.strafe, player.jump_pending),
        (1, -1, true)
    );
}

#[test]
fn an_authored_rng_seed_is_bound_to_recovery_identity() {
    let content = wandering_wolf(arena::wolf(30, [1, 2]));
    let retuned = Arc::new(
        content
            .as_ref()
            .clone()
            .with_rng_seed(content.rng_seed() ^ 1),
    );
    assert_eq!(content.revision(), retuned.revision());
    assert_ne!(content.fingerprint(), retuned.fingerprint());
    assert_eq!(
        restore(checkpoint(&content), &retuned).err().unwrap(),
        "snapshot content identity does not match the supplied zone content"
    );
}

#[test]
fn recovery_refuses_wander_destinations_beyond_the_wander_radius() {
    let content = wandering_wolf(arena::wolf(30, [1, 2]));
    let valid = checkpoint(&content);
    let with_destination = |destination: [i32; 2]| {
        let mut state = valid.clone();
        state.creatures[0].ai = CreatureAi::Idle {
            timer: 5,
            destination: Some(destination),
        };
        state
    };
    // Wander rounding may land one unit beyond the radius.
    for inside in [
        [HOME[0] + WANDER_RADIUS + 1, HOME[1]],
        [HOME[0], HOME[1] - WANDER_RADIUS - 1],
        [HOME[0] - 424, HOME[1] + 424],
    ] {
        let mut zone = restore(with_destination(inside), &content).unwrap();
        zone.advance_tick().unwrap();
    }
    for outside in [
        [HOME[0] + WANDER_RADIUS + 2, HOME[1]],
        [HOME[0] - 426, HOME[1] - 426],
        [i32::MAX, i32::MIN],
        [i32::MIN, i32::MAX],
    ] {
        assert_eq!(
            restore(with_destination(outside), &content).err().unwrap(),
            "creature position or wander destination is out of range",
            "{outside:?}"
        );
    }
}

#[test]
fn recovery_refuses_creature_and_corpse_positions_outside_the_content_bounds() {
    let content = wandering_wolf(arena::wolf(30, [1, 2]));
    let valid = checkpoint(&content);
    let limit = MAX_CONTENT_COORDINATE_UNITS;
    let at = |position: [i32; 3], corpse: bool| {
        let mut state = valid.clone();
        let creature = &mut state.creatures[0];
        creature.position = position;
        if corpse {
            creature.life = CreatureLife::Corpse { died_at: 0 };
            creature.health = 0;
            creature.velocity = [0; 3];
            creature.ai = CreatureAi::Idle {
                timer: 0,
                destination: None,
            };
        }
        state
    };
    for corpse in [false, true] {
        let mut zone = restore(at([limit - 40, 45, -limit + 40], corpse), &content).unwrap();
        zone.advance_tick().unwrap();
        for outside in [
            [limit + 1, 45, 0],
            [0, -limit - 1, 0],
            [0, 45, i32::MIN],
            [i32::MAX, 45, i32::MAX],
        ] {
            assert_eq!(
                restore(at(outside, corpse), &content).err().unwrap(),
                "creature position or wander destination is out of range",
                "{outside:?}, corpse: {corpse}"
            );
        }
    }
}

/// Recovery keeps player positions unchanged anywhere in the `i32` range,
/// so creature steering and every distance test must not overflow there.
#[test]
fn extreme_player_positions_never_overflow_combat_and_ai_arithmetic() {
    let content = wandering_wolf(arena::wolf(30, [1, 2]));
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [i32::MIN + 100, 50, i32::MIN + 100];
    state.players[1].position = [i32::MAX - 100, 50, i32::MAX - 100];
    let wolf = &mut state.creatures[0];
    wolf.ai = CreatureAi::Engaged;
    wolf.threat = vec![ThreatEntry {
        entity: EntityRef::Player(1),
        threat: 5,
    }];
    let mut zone = restore(state, &content).unwrap();
    zone.apply_command(1, 1, ZoneCommand::SelectTarget(Some(EntityRef::Player(2))))
        .unwrap();

    zone.advance_tick().unwrap();
    assert_eq!(
        zone.snapshot_for_player(1).unwrap().events,
        [ZoneEvent::Error {
            code: ErrorCode::InvalidTarget,
            target: Some(EntityRef::Player(2)),
        }],
        "a unit across the whole coordinate range is not visible"
    );
    let wolf = &zone.snapshot().unwrap().creatures[0];
    assert_eq!(wolf.creature_id, CreatureId::new(1));
    assert_eq!(wolf.ai, CreatureAi::Engaged, "the wolf chases its target");
    assert!(
        wolf.velocity[0] < 0 && wolf.velocity[2] < 0,
        "it steers toward the far corner: {:?}",
        wolf.velocity
    );
}
