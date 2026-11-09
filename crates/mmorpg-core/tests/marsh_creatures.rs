//! The Marsh-Lake Wetlands' creatures (#506): on valid ground or water, clear of every collider,
//! inside the protocol's entity packet budget, and with the Causeway's walk left open.
use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, ids, marsh};
use mmorpg_core::physics::to_units;
use mmorpg_core::water::FLOAT_DEPTH_UNITS;
use mmorpg_core::{
    CreatureSpawn, INTEREST_RADIUS_UNITS, WaterMode, ZoneCommand, ZoneContent, ZoneId,
    ZoneSimulation,
};

/// `MAX_WIRE_ENTITIES` in `mmorpg-protocol`: the entity records one projection can carry.
const WIRE_ENTITIES: usize = 35;
/// Records the wetlands must leave free for the viewer and other players.
const PLAYER_ROOM: usize = 15;
/// Clearance kept between a floating lurker's feet and the bed under it.
const BED_CLEARANCE: i32 = 20;

fn wildlife(content: &ZoneContent) -> Vec<&CreatureSpawn> {
    content
        .creature_spawns()
        .iter()
        .filter(|spawn| (1_100..1_200).contains(&spawn.id.get()))
        .collect()
}

fn is_lurker(content: &ZoneContent, spawn: &CreatureSpawn) -> bool {
    content
        .creature_template(spawn.template)
        .is_some_and(|template| template.family.swims())
}

/// The named water holding `(x, z)`, if any.
fn water_at(content: &ZoneContent, x: i32, z: i32) -> Option<&mmorpg_core::WaterVolume> {
    content.waters().iter().find(|water| water.contains(x, z))
}

/// The centre and eight points round the rim of a spawn's reach.
fn disc(at: [i32; 2], reach: i32) -> [[i32; 2]; 9] {
    let [x, z] = at;
    let d = reach * 7 / 10;
    [
        [x, z],
        [x - reach, z],
        [x + reach, z],
        [x, z - reach],
        [x, z + reach],
        [x - d, z - d],
        [x + d, z - d],
        [x - d, z + d],
        [x + d, z + d],
    ]
}

#[test]
fn the_wetlands_hold_lurkers_boars_and_a_wolf_pack_at_the_templates_top_levels() {
    let content = greyhaven_vale::content();
    let mut counts: BTreeMap<(String, u8), usize> = BTreeMap::new();
    for spawn in wildlife(&content) {
        let template = content.creature_template(spawn.template).unwrap();
        assert_eq!(
            greyhaven_vale::area_at(spawn.position[0], spawn.position[1]).map(|area| area.id()),
            Some(marsh::MARSH_LAKE_WETLANDS),
            "spawn {} is outside the wetlands",
            spawn.id.get()
        );
        let levels = spawn.level_range(template);
        assert_eq!(levels, [template.max_level; 2], "spawn {}", spawn.id.get());
        *counts
            .entry((template.name.clone(), levels[0]))
            .or_default() += 1;
    }
    let expected = BTreeMap::from([
        (("Mirefin Lurker".to_owned(), 3), 15),
        (("Young Boar".to_owned(), 2), 8),
        (("Timber Wolf".to_owned(), 2), 8),
    ]);
    assert_eq!(counts, expected);
}

#[test]
fn lurkers_float_over_deep_water_in_the_lake_and_the_swimmable_pools() {
    let content = greyhaven_vale::content();
    let definition = content.definition();
    let mut homes: BTreeMap<&str, usize> = BTreeMap::new();
    for spawn in wildlife(&content)
        .into_iter()
        .filter(|spawn| is_lurker(&content, spawn))
    {
        let template = content.creature_template(spawn.template).unwrap();
        let reach = spawn.wander_radius + template.half_extents[0];
        let water = water_at(&content, spawn.position[0], spawn.position[1])
            .unwrap_or_else(|| panic!("lurker {} is not in water", spawn.id.get()));
        *homes.entry(&water.name).or_default() += 1;
        for [x, z] in disc(spawn.position, reach) {
            let depth = water.surface - definition.ground_at(x, z);
            assert!(
                depth >= FLOAT_DEPTH_UNITS + BED_CLEARANCE,
                "lurker {} would touch the bed at ({x}, {z}): {depth} units deep",
                spawn.id.get()
            );
        }
    }
    assert_eq!(
        homes,
        BTreeMap::from([("Marsh Lake", 11), ("Cottage Pool", 2), ("Southmere", 2)])
    );
}

#[test]
fn the_wetlands_creatures_stand_clear_of_every_collider() {
    let content = greyhaven_vale::content();
    let definition = content.definition();
    let solid: Vec<_> = definition
        .colliders()
        .iter()
        .filter(|collider| {
            collider.id != ids::GROUND && !ids::BOUNDARY_WALLS.contains(&collider.id)
        })
        .collect();
    let mut violations = Vec::new();
    for spawn in wildlife(&content) {
        let template = content.creature_template(spawn.template).unwrap();
        let reach = spawn.wander_radius + template.half_extents[0].max(template.half_extents[2]);
        let [x, z] = spawn.position;
        let id = spawn.id.get();
        for collider in &solid {
            let [cx, _, cz] = collider.position;
            let [hx, _, hz] = collider.half_extents;
            let gap = ((x - cx).abs() - hx).max((z - cz).abs() - hz);
            if gap < reach {
                violations.push(format!(
                    "{id} reaches collider {} at [{cx}, {cz}] (gap {gap}, reach {reach})",
                    collider.id
                ));
            }
        }
        if is_lurker(&content, spawn) {
            continue;
        }
        let points = disc(spawn.position, reach);
        for [px, pz] in points {
            if water_at(&content, px, pz).is_some() {
                violations.push(format!("{id} reaches water at ({px}, {pz})"));
            }
        }
        // The disc rises no more than 1:2 across its diameter: walkable ground everywhere.
        let heights = points.map(|[px, pz]| definition.ground_at(px, pz));
        let spread = heights.iter().max().unwrap() - heights.iter().min().unwrap();
        if spread > reach {
            violations.push(format!(
                "{id} stands on a {spread}-unit slope (reach {reach})"
            ));
        }
    }
    assert!(violations.is_empty(), "{violations:#?}");
}

/// Every viewer position in the wetlands, on a 2 m lattice, counts each spawn whose wander disc
/// can reach its interest radius.
#[test]
fn the_wetlands_stay_inside_the_entity_budget() {
    let content = greyhaven_vale::content();
    let mut worst = (0, [0, 0]);
    for x in (marsh::RIDGE[3]..marsh::BOUNDS.max[0]).step_by(200) {
        for z in (marsh::BOUNDS.min[1]..marsh::BOUNDS.max[1]).step_by(200) {
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
            if visible > worst.0 {
                worst = (visible, [x, z]);
            }
        }
    }
    assert!(
        worst.0 + PLAYER_ROOM <= WIRE_ENTITIES,
        "{} creatures reach a viewer at {:?}",
        worst.0,
        worst.1
    );
}

#[test]
fn the_causeway_and_its_approach_stay_open() {
    let content = greyhaven_vale::content();
    for spawn in wildlife(&content) {
        let template = content.creature_template(spawn.template).unwrap();
        let reach = spawn.wander_radius + template.half_extents[0].max(template.half_extents[2]);
        let [x, z] = spawn.position;
        for lane in &marsh::CAUSEWAY {
            let gap = (lane.min[0] - x)
                .max(x - lane.max[0])
                .max(lane.min[1] - z)
                .max(z - lane.max[1]);
            assert!(
                gap >= reach,
                "spawn {} can stand on the Causeway (gap {gap}, reach {reach})",
                spawn.id.get()
            );
        }
        if !is_lurker(&content, spawn) {
            // The Marsh Trail's last leg, down to the Causeway's foot.
            let gap = (x - 34_400).abs().max((-2_000 - z).max(z - 3_200));
            assert!(
                gap >= 2 * reach,
                "spawn {} guards the Causeway's approach (gap {gap})",
                spawn.id.get()
            );
        }
        // Nothing but water around the island's landing.
        let dx = i64::from(x - marsh::LAKE.centre[0]);
        let dz = i64::from(z - marsh::LAKE.centre[1]);
        assert!(
            dx * dx + dz * dz > (marsh::ISLAND_RADIUS * 3 / 2).pow(2),
            "spawn {} stands on the island",
            spawn.id.get()
        );
    }
}

#[test]
fn the_wetlands_creatures_idle_where_they_belong() {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    for _ in 0..mmorpg_core::golden::t30(9_000) {
        zone.advance_tick().unwrap();
    }
    let snapshot = zone.snapshot().unwrap();
    for spawn in wildlife(&content) {
        let creature = snapshot
            .creatures
            .iter()
            .find(|creature| creature.creature_id == spawn.id)
            .unwrap();
        let [x, y, z] = to_units(creature.position);
        let limit = i64::from(spawn.wander_radius) + 50;
        let (dx, dz) = (
            i64::from(x - spawn.position[0]),
            i64::from(z - spawn.position[1]),
        );
        assert!(
            dx * dx + dz * dz <= limit * limit,
            "creature {} strayed to ({x}, {z})",
            spawn.id.get()
        );
        let template = content.creature_template(spawn.template).unwrap();
        let feet = y - template.half_extents[1];
        if is_lurker(&content, spawn) {
            let water = water_at(&content, x, z).unwrap();
            assert_eq!(
                WaterMode::of_body(content.waters(), [x, feet, z], 2 * template.half_extents[1]),
                WaterMode::Swimming,
                "lurker {} feet at {feet}",
                spawn.id.get()
            );
            assert!(
                (water.surface - feet - FLOAT_DEPTH_UNITS).abs() <= 20,
                "lurker {} floats with feet at {feet}",
                spawn.id.get()
            );
        } else {
            let drop = feet - content.definition().ground_at(x, z);
            assert!(
                drop.abs() <= 60,
                "creature {} stands {drop} units off the ground",
                spawn.id.get()
            );
        }
    }
}

/// A level 1 player crosses from the Causeway's foot to the island and back among the live
/// creatures: the lurkers may strike, but nothing stands in the way.
#[test]
fn a_new_player_walks_the_causeway_with_the_creatures_alive() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let ground = zone.content().definition().ground_at(34_400, -2_000);
    state.players[0].position = mmorpg_core::physics::from_units([34_400, ground + 100, -2_000]);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let mut sequence = 0;
    for goal in [[44_800, -2_000], [34_400, -2_000]] {
        let mut ticks = 0;
        loop {
            let [x, y, z] = zone.snapshot().unwrap().players[0].position;
            assert!(y > 0.0, "fell through at ({x}, {y}, {z})");
            let (dx, dz) = (goal[0] as f32 - x, goal[1] as f32 - z);
            if dx.hypot(dz) < 60.0 {
                break;
            }
            ticks += 1;
            assert!(
                ticks < mmorpg_core::golden::t30(1_500),
                "stuck at ({x}, {z}) on the way to {goal:?}"
            );
            let angle = dx.atan2(dz).rem_euclid(std::f32::consts::TAU);
            sequence += 1;
            zone.apply_command(
                1,
                sequence,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing: (angle / std::f32::consts::TAU * 65_536.0) as u32 as u16,
                },
            )
            .unwrap();
            zone.advance_tick().unwrap();
        }
    }
    assert!(
        zone.snapshot().unwrap().players[0].combat.health > 0,
        "the walk killed a new player"
    );
}
