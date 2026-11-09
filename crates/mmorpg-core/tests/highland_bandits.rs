//! The Bandit Highlands' Redbrand garrison (#502): spawned on the region's real ground, clear of
//! every collider, idling without getting stuck, and inside the protocol's entity packet budget.
use std::collections::BTreeMap;

use mmorpg_core::greyhaven_vale::{self, highland_bandits, highlands, ids};
use mmorpg_core::physics::to_units;
use mmorpg_core::{CreatureSpawn, INTEREST_RADIUS_UNITS, ZoneId, ZoneSimulation};

/// `MAX_WIRE_ENTITIES` in `mmorpg-protocol`: the entity records one projection can carry.
const WIRE_ENTITIES: usize = 35;
/// Records the garrison must leave free at the stronghold for the viewer and other players.
const PLAYER_ROOM: usize = 15;

fn garrison(content: &mmorpg_core::ZoneContent) -> Vec<&CreatureSpawn> {
    content
        .creature_spawns()
        .iter()
        .filter(|spawn| (1_000..1_100).contains(&spawn.id.get()))
        .collect()
}

#[test]
fn the_garrison_holds_the_stronghold_the_trail_and_the_lookout() {
    let content = greyhaven_vale::content();
    let (yard_min, yard_max) = highlands::STRONGHOLD;
    let in_yard = |at: [i32; 2]| {
        (yard_min[0]..=yard_max[0]).contains(&at[0]) && (yard_min[1]..=yard_max[1]).contains(&at[1])
    };
    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for spawn in garrison(&content) {
        let template = content.creature_template(spawn.template).unwrap();
        let place = if in_yard(spawn.position) {
            "stronghold"
        } else if spawn.wander_radius == highland_bandits::LOOKOUT_WANDER_RADIUS_UNITS {
            "lookout"
        } else {
            "trail"
        };
        *counts.entry((place, template.name.as_str())).or_default() += 1;
        assert_eq!(
            greyhaven_vale::area_at(spawn.position[0], spawn.position[1]).map(|area| area.id()),
            Some(highlands::BANDIT_HIGHLANDS),
            "spawn {} is outside the highlands",
            spawn.id.get()
        );
        let levels = spawn.level_range(template);
        assert!(levels[0] >= 4, "spawn {} rolls Vale levels", spawn.id.get());
    }
    let expected = BTreeMap::from([
        (("stronghold", "Redbrand Bandit"), 7),
        (("stronghold", "Redbrand Crossbowman"), 4),
        (("stronghold", "Redbrand Brute"), 2),
        (("trail", "Redbrand Bandit"), 7),
        (("trail", "Redbrand Crossbowman"), 3),
        (("trail", "Redbrand Brute"), 1),
        (("lookout", "Redbrand Crossbowman"), 1),
    ]);
    assert_eq!(counts, expected);
}

#[test]
fn the_garrison_stands_clear_of_every_collider() {
    let content = greyhaven_vale::content();
    let definition = content.definition();
    let solid: Vec<_> = definition
        .colliders()
        .iter()
        .filter(|collider| {
            collider.id != ids::GROUND && !ids::BOUNDARY_WALLS.contains(&collider.id)
        })
        .collect();
    for spawn in garrison(&content) {
        let template = content.creature_template(spawn.template).unwrap();
        // The whole wander disc plus the body must miss every collider's XZ box, so no
        // idle wander can walk into a wall, a tent or the keep.
        let reach = spawn.wander_radius + template.half_extents[0].max(template.half_extents[2]);
        let [x, z] = spawn.position;
        for collider in &solid {
            let [cx, _, cz] = collider.position;
            let [hx, _, hz] = collider.half_extents;
            let gap = ((x - cx).abs() - hx).max((z - cz).abs() - hz);
            assert!(
                gap >= reach,
                "spawn {} reaches collider {} (gap {gap}, reach {reach})",
                spawn.id.get(),
                collider.id
            );
        }
        // The disc rises no more than 1:2 across its diameter: walkable ground everywhere.
        let heights = [
            definition.ground_at(x, z),
            definition.ground_at(x - reach, z),
            definition.ground_at(x + reach, z),
            definition.ground_at(x, z - reach),
            definition.ground_at(x, z + reach),
        ];
        let spread = heights.iter().max().unwrap() - heights.iter().min().unwrap();
        assert!(
            spread <= reach,
            "spawn {} stands on a {spread}-unit slope (reach {reach})",
            spawn.id.get()
        );
    }
}

/// Every viewer position in the yard, on a 1 m lattice, counts each spawn whose wander disc can
/// reach its interest radius: the garrison alone leaves room for players in the packet.
#[test]
fn the_stronghold_stays_inside_the_entity_budget() {
    let content = greyhaven_vale::content();
    let (min, max) = highlands::STRONGHOLD;
    let mut worst = 0;
    for x in (min[0]..=max[0]).step_by(100) {
        for z in (min[1]..=max[1]).step_by(100) {
            let visible = content
                .creature_spawns()
                .iter()
                .filter(|spawn| {
                    let dx = i64::from(spawn.position[0] - x);
                    let dz = i64::from(spawn.position[1] - z);
                    let reach = i64::from(INTEREST_RADIUS_UNITS + spawn.wander_radius);
                    dx * dx + dz * dz <= reach * reach
                })
                .count();
            worst = worst.max(visible);
        }
    }
    assert!(
        worst + PLAYER_ROOM <= WIRE_ENTITIES,
        "{worst} creatures reach a viewer at the stronghold"
    );

    // A live projection at the yard's centre agrees.
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let ground = zone.content().definition().ground_at(0, -52_800);
    state.players[0].position = mmorpg_core::physics::from_units([0, ground + 100, -52_800]);
    let zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    let view = zone.snapshot_for_player(1).unwrap();
    assert!(view.entities.len() <= worst + 1, "{}", view.entities.len());
}

#[test]
fn the_garrison_idles_on_its_ground_without_straying() {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    for _ in 0..mmorpg_core::golden::t30(9_000) {
        zone.advance_tick().unwrap();
    }
    let snapshot = zone.snapshot().unwrap();
    for spawn in garrison(&content) {
        let creature = snapshot
            .creatures
            .iter()
            .find(|creature| creature.creature_id == spawn.id)
            .unwrap();
        let [x, y, z] = to_units(creature.position);
        let limit = i64::from(spawn.wander_radius) + 50;
        let dx = i64::from(x - spawn.position[0]);
        let dz = i64::from(z - spawn.position[1]);
        assert!(
            dx * dx + dz * dz <= limit * limit,
            "creature {} strayed to ({x}, {z})",
            spawn.id.get()
        );
        let template = content.creature_template(spawn.template).unwrap();
        let drop = y - template.half_extents[1] - content.definition().ground_at(x, z);
        assert!(
            drop.abs() <= 60,
            "creature {} stands {drop} units off the ground",
            spawn.id.get()
        );
    }
}
