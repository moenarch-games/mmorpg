//! Wildlife dens (#912): a pack rests at its den, roams out to an outing spot and hunts the prey
//! grazing there, comes home again, keeps its schedule unseen, and a zone restored mid-hunt
//! continues exactly.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureId, CreatureSpawn, DEN_LEG_TICKS, FLEE_RADIUS_UNITS,
    WildlifeDen, ZoneContent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR, WOLF};

const DEN: [i32; 2] = [-2_500, 3_000];
const OUTING: [i32; 2] = [2_500, 3_000];
const PACK: [u32; 3] = [1, 2, 3];
const PREY: CreatureId = CreatureId::new(4);
const PREY_HOME: [i32; 2] = [3_000, 3_500];

/// Three aggressive wolves at their den 30 m north of the player, and a boar by the outing spot.
fn content() -> Arc<ZoneContent> {
    let roaming = |id, template, position| CreatureSpawn {
        wander_radius: 300,
        ..arena::spawn(id, template, position)
    };
    let spawns = vec![
        roaming(1, WOLF, DEN),
        roaming(2, WOLF, [DEN[0] + 180, DEN[1] + 120]),
        roaming(3, WOLF, [DEN[0] - 180, DEN[1] + 120]),
        roaming(4, BOAR, PREY_HOME),
    ];
    let content = arena::arena(vec![arena::wolf(40, [2, 3]), arena::boar()], spawns, vec![]);
    Arc::new(
        content
            .as_ref()
            .clone()
            .with_rng_seed(0x912)
            .with_dens(vec![WildlifeDen {
                name: "Test Den".into(),
                den: DEN,
                pack: PACK.into_iter().map(CreatureId::new).collect(),
                outings: vec![OUTING],
                prey: vec![PREY],
            }])
            .unwrap(),
    )
}

fn at(zone: &ZoneSimulation, id: CreatureId) -> [i32; 2] {
    let creature = zone
        .snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == id)
        .unwrap();
    #[allow(clippy::cast_possible_truncation)]
    [creature.position[0] as i32, creature.position[2] as i32]
}

fn distance(a: [i32; 2], b: [i32; 2]) -> i64 {
    (i64::from(a[0] - b[0]).pow(2) + i64::from(a[1] - b[1]).pow(2)).isqrt()
}

fn pack(zone: &ZoneSimulation) -> Vec<[i32; 2]> {
    PACK.into_iter()
        .map(|id| at(zone, CreatureId::new(id)))
        .collect()
}

fn advance(zone: &mut ZoneSimulation, until: u64) {
    while zone.current_tick() < until {
        zone.advance_tick().unwrap();
    }
}

#[test]
fn a_pack_roams_from_its_den_hunts_and_comes_home_deterministically() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content()).unwrap();
    zone.add_player(1).unwrap();

    // Resting at the den for the first leg.
    advance(&mut zone, DEN_LEG_TICKS - 1);
    assert!(pack(&zone).iter().all(|&wolf| distance(wolf, DEN) <= 600));

    // Out on the hunt: the pack runs at the boar, which flees well beyond its grazing range.
    let mut fled = 0;
    let mut restored: Option<(ZoneSimulation, CanonicalZoneSnapshot)> = None;
    while zone.current_tick() < 2 * DEN_LEG_TICKS - 1 {
        zone.advance_tick().unwrap();
        let boar = at(&zone, PREY);
        let chased = pack(&zone)
            .iter()
            .any(|&wolf| distance(wolf, boar) <= i64::from(FLEE_RADIUS_UNITS));
        fled = fled.max(distance(boar, PREY_HOME));
        // Mid-hunt, a copy restored from the canonical snapshot takes over.
        if chased && restored.is_none() && distance(boar, PREY_HOME) > 500 {
            let state = zone.snapshot().unwrap();
            let copy = ZoneSimulation::from_snapshot(state.clone(), content()).unwrap();
            restored = Some((copy, state));
        }
    }
    assert!(fled > 1_200, "the boar fled only {fled} units");
    assert!(
        pack(&zone).iter().all(|&wolf| distance(wolf, DEN) > 3_000),
        "the pack roams away from its den"
    );

    // Home again for the next leg.
    advance(&mut zone, 3 * DEN_LEG_TICKS - 1);
    assert!(pack(&zone).iter().all(|&wolf| distance(wolf, DEN) <= 600));

    // The copy restored mid-hunt matches the original tick for tick.
    let (mut copy, state) = restored.expect("the pack caught up with the boar");
    let mut original = ZoneSimulation::from_snapshot(state, content()).unwrap();
    for _ in 0..DEN_LEG_TICKS {
        original.advance_tick().unwrap();
        copy.advance_tick().unwrap();
        assert_eq!(original.snapshot().unwrap(), copy.snapshot().unwrap());
    }
    let mut replayed = ZoneSimulation::with_content(ZoneId::new(9), content()).unwrap();
    replayed.add_player(1).unwrap();
    advance(&mut replayed, 3 * DEN_LEG_TICKS - 1);
    assert_eq!(replayed.snapshot().unwrap(), zone.snapshot().unwrap());
}

#[test]
fn an_unseen_pack_keeps_its_schedule_without_waking() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content()).unwrap();
    advance(&mut zone, DEN_LEG_TICKS + 1);
    for (wolf, slot) in pack(&zone)
        .into_iter()
        .zip([[0, 0], [180, 120], [-180, 120]])
    {
        assert!(distance(wolf, [OUTING[0] + slot[0], OUTING[1] + slot[1]]) <= 1);
    }
}
