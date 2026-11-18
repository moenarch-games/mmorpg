//! The Greyfang Approach's Redbrand patrol (#911): posted clear of colliders and of other fights, it
//! engages as one, the Brute holds the front, the Bandit flanks inside one swing arc of it and
//! falls back after its swings, the Crossbowman keeps its distance and shoots, and the fight
//! replays exactly, also through a mid-fight restore.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, ids, redbrand_patrol};
use mmorpg_core::physics::{from_units, to_units};
use mmorpg_core::{
    CreatureAi, CreatureBehaviour, CreatureId, EntityRef, PatrolRole, ZoneContent, ZoneEvent,
    ZoneId, ZoneSimulation, ability::ids::CROSSBOW_BOLT, golden::t30, trig,
};

const BRUTE: CreatureId = CreatureId::new(1_700);
const BANDIT: CreatureId = CreatureId::new(1_701);
const CROSSBOWMAN: CreatureId = CreatureId::new(1_702);
/// The player waits on the Greyfang Approach road, west of the patrol's posts.
const ROAD: [i32; 2] = [9_700, -12_250];
/// A creature's melee reach against a player: 2 m plus the player's half-width.
const REACH: i64 = 230;

fn content() -> Arc<ZoneContent> {
    greyhaven_vale::content()
}

fn xz(position: [f32; 3]) -> [i64; 2] {
    let [x, _, z] = to_units(position);
    [i64::from(x), i64::from(z)]
}

fn distance([ax, az]: [i64; 2], [bx, bz]: [i64; 2]) -> i64 {
    ((bx - ax).pow(2) + (bz - az).pow(2)).isqrt()
}

/// The yaw from `from` to `to`.
fn bearing(from: [i64; 2], to: [i64; 2]) -> u16 {
    trig::yaw_from_vector(
        i32::try_from(to[0] - from[0]).unwrap(),
        i32::try_from(to[1] - from[1]).unwrap(),
    )
    .unwrap()
}

/// A zone with one level 5 player standing on the road beside the patrol.
fn zone() -> ZoneSimulation {
    let content = content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let ground = content.definition().ground_at(ROAD[0], ROAD[1]);
    state.players[0].position = from_units([ROAD[0], ground + 100, ROAD[1]]);
    // A level 5 player, as the Hollow's quests expect.
    state.players[0].combat.level = 5;
    state.players[0].combat.health = mmorpg_core::unit::player_max_health(5);
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

struct Fight {
    player: [i64; 2],
    units: [[i64; 2]; 3],
    ai: [CreatureAi; 3],
}

fn look(zone: &ZoneSimulation) -> Fight {
    let snapshot = zone.snapshot().unwrap();
    let creature = |id| {
        snapshot
            .creatures
            .iter()
            .find(|creature| creature.creature_id == id)
            .unwrap()
    };
    let [brute, bandit, crossbowman] = [BRUTE, BANDIT, CROSSBOWMAN].map(creature);
    Fight {
        player: xz(snapshot.players[0].position),
        units: [brute, bandit, crossbowman].map(|creature| xz(creature.position)),
        ai: [brute, bandit, crossbowman].map(|creature| creature.ai),
    }
}

#[test]
fn the_patrol_has_its_three_roles_and_posts_clear_of_other_fights() {
    let content = content();
    let roles: Vec<_> = content.patrols().iter().map(|member| member.role).collect();
    assert_eq!(
        roles,
        [
            PatrolRole::Bruiser,
            PatrolRole::Skirmisher,
            PatrolRole::Marksman
        ]
    );
    let definition = content.definition();
    for member in content.patrols() {
        let spawn = content.creature_spawn(member.creature).unwrap();
        assert!(redbrand_patrol::SPAWN_IDS.contains(&spawn.id.get()));
        let template = content.creature_template(spawn.template).unwrap();
        let reach = spawn.wander_radius + template.half_extents[0];
        let [x, z] = spawn.position;
        for collider in definition.colliders() {
            if collider.id == ids::GROUND || ids::BOUNDARY_WALLS.contains(&collider.id) {
                continue;
            }
            let [cx, _, cz] = collider.position;
            let [hx, _, hz] = collider.half_extents;
            let gap = ((x - cx).abs() - hx).max((z - cz).abs() - hz);
            assert!(
                gap >= reach,
                "{} reaches collider {}",
                spawn.id.get(),
                collider.id
            );
        }
        // No other hostile is near enough to join the fight: assist plus the widest aggro.
        for other in content.creature_spawns() {
            let template = content.creature_template(other.template).unwrap();
            if content.patrol_member(other.id).is_some()
                || template.behaviour != CreatureBehaviour::Aggressive
            {
                continue;
            }
            let gap = distance(spawn.position.map(i64::from), other.position.map(i64::from));
            assert!(
                gap > 3_000,
                "{} sits {gap} from {}",
                spawn.id.get(),
                other.id.get()
            );
        }
    }
}

#[test]
fn the_patrol_engages_by_role_and_replays_exactly() {
    let mut zone = zone();
    let mut checkpoint = None;
    let mut bolts = 0;
    let mut recovered = false;
    let mut swung_at = None;
    let mut lined_up = 0;
    for tick in 0..t30(180) {
        zone.advance_tick().unwrap();
        if tick == t30(90) {
            checkpoint = Some(zone.snapshot().unwrap());
        }
        for event in &zone.snapshot_for_player(1).unwrap().events {
            match *event {
                ZoneEvent::CastStarted {
                    source: EntityRef::Creature(CROSSBOWMAN),
                    ability,
                    ..
                } if ability == CROSSBOW_BOLT => bolts += 1,
                ZoneEvent::AttackPerformed {
                    source: EntityRef::Creature(BANDIT),
                    ..
                } => swung_at = Some(tick),
                _ => {}
            }
        }
        let fight = look(&zone);
        if fight.ai.iter().any(|&ai| ai != CreatureAi::Engaged) {
            assert!(tick < t30(60), "the patrol never engaged as one");
            continue;
        }
        let [brute, bandit, crossbowman] = fight.units.map(|unit| distance(fight.player, unit));
        // The Crossbowman never closes into melee.
        if tick > t30(60) {
            assert!(
                crossbowman >= 400,
                "the Crossbowman closed to {crossbowman}"
            );
        }
        // Brute and Bandit both in reach, inside one 100° arc: one slash cleaves both.
        if brute <= REACH + 20 && bandit <= REACH + 20 {
            let turn = bearing(fight.player, fight.units[1])
                .wrapping_sub(bearing(fight.player, fight.units[0]))
                .cast_signed()
                .unsigned_abs();
            assert!(turn <= 18_204, "the two melee Redbrands stand {turn} apart");
            if turn >= 5_461 {
                lined_up += 1;
            }
        }
        // After a swing the Bandit falls back out of reach.
        if swung_at.is_some_and(|at| tick > at + t30(9)) && bandit > REACH + 50 {
            recovered = true;
        }
    }
    assert!(bolts >= 1, "the Crossbowman never shot");
    assert!(
        lined_up > 0,
        "the Bandit never flanked within the Brute's arc"
    );
    assert!(recovered, "the Bandit never fell back after a swing");

    // The same fight replays exactly, from the start and from a mid-fight checkpoint.
    let mut replay = self::zone();
    for _ in 0..t30(180) {
        replay.advance_tick().unwrap();
        replay.snapshot_for_player(1).unwrap();
    }
    assert_eq!(replay.snapshot().unwrap(), zone.snapshot().unwrap());
    let mut restored = ZoneSimulation::from_snapshot(checkpoint.unwrap(), content()).unwrap();
    for _ in t30(90) + 1..t30(180) {
        restored.advance_tick().unwrap();
        restored.snapshot_for_player(1).unwrap();
    }
    assert_eq!(restored.snapshot().unwrap(), zone.snapshot().unwrap());
}
