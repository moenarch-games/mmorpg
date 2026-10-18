//! Greyhaven Vale content invariants: bounds, clear spawn plaza, open road
//! corridors, walkable woods, disjoint named areas and blocking structures.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{
    self, PLAYABLE_BOUNDS, REVISION, ROAD_CLEARANCE_UNITS, SPAWN_GRID, SPAWN_PLAZA, area_at, areas,
    clear_of_roads, ids, roads,
};
use mmorpg_core::{
    Area, AreaId, MAX_CONTENT_COORDINATE_UNITS, MAX_PLAYERS_PER_ZONE, MAX_STATIC_COLLIDERS,
    PLAYER_HALF_EXTENTS_UNITS, StaticCollider, XzBounds, ZoneCommand, ZoneId, ZoneSimulation,
    greyhaven_vale_definition,
};

/// Map north is −Z (the Redbrand cliffs); yaw 0 faces +Z.
const FACE_NORTH: u16 = 32_768;
const FACE_SOUTH: u16 = 0;

fn is_terrain(collider: &StaticCollider) -> bool {
    collider.id == ids::GROUND || ids::BOUNDARY_WALLS.contains(&collider.id)
}

fn footprint(collider: &StaticCollider) -> ([i32; 2], [i32; 2]) {
    (
        [
            collider.position[0] - collider.half_extents[0],
            collider.position[2] - collider.half_extents[2],
        ],
        [
            collider.position[0] + collider.half_extents[0],
            collider.position[2] + collider.half_extents[2],
        ],
    )
}

fn run(facing: u16) -> ZoneCommand {
    ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing,
    }
}

#[test]
fn content_is_a_bounded_deterministic_revision() {
    let definition = greyhaven_vale_definition();
    assert_eq!(definition, greyhaven_vale_definition());
    assert_eq!(definition.revision(), REVISION);
    assert_eq!(definition.gravity(), greyhaven_vale::GRAVITY);
    assert_eq!(definition.spawn_grid(), SPAWN_GRID);
    assert!(definition.colliders().len() <= MAX_STATIC_COLLIDERS);
    let trees = definition
        .colliders()
        .iter()
        .filter(|collider| ids::TREES.contains(&collider.id))
        .count();
    assert!((800..=1_200).contains(&trees), "{trees} trees");
    for collider in definition.colliders() {
        let (min, max) = footprint(collider);
        let reach = PLAYABLE_BOUNDS.max[0] + 200;
        assert!(
            min.iter().chain(&max).all(|value| value.abs() <= reach),
            "collider {} lies beyond the boundary walls",
            collider.id
        );
        assert!(
            collider
                .position
                .iter()
                .zip(collider.half_extents)
                .all(
                    |(position, half)| (position + half).abs() <= MAX_CONTENT_COORDINATE_UNITS
                        && (position - half).abs() <= MAX_CONTENT_COORDINATE_UNITS
                )
        );
        if !is_terrain(collider) {
            // Structures stand on the ground (the inn's lintel spans its doorway) and stay
            // inside the playable square.
            // Out on the hills they stand on the terrain, sunk 0.3 m below their lowest corner (#159).
            // The cave's roof rests on its walls.
            if !ids::INN_PARTS.contains(&collider.id) && collider.id != ids::CAVE_ROOF {
                let ground = definition.terrain().map_or(0, |terrain| {
                    [min, [max[0], min[1]], max, [min[0], max[1]]]
                        .iter()
                        .map(|corner| terrain.height_at(corner[0], corner[1]))
                        .min()
                        .unwrap_or(0)
                });
                let base = collider.position[1] - collider.half_extents[1];
                assert!(
                    base == 0 || base == ground - 30,
                    "collider {} stands at {base} on {ground}",
                    collider.id
                );
            }
            assert!(PLAYABLE_BOUNDS.contains(min[0], min[1]), "{}", collider.id);
            assert!(
                PLAYABLE_BOUNDS.contains(max[0] - 1, max[1] - 1),
                "{}",
                collider.id
            );
        }
    }
}

#[test]
fn structures_touch_but_never_overlap() {
    let definition = greyhaven_vale_definition();
    let structures: Vec<_> = definition
        .colliders()
        .iter()
        .filter(|collider| !is_terrain(collider))
        .collect();
    for (index, left) in structures.iter().enumerate() {
        for right in &structures[index + 1..] {
            let overlaps = (0..3).all(|axis| {
                (left.position[axis] - right.position[axis]).abs()
                    < left.half_extents[axis] + right.half_extents[axis]
            });
            assert!(!overlaps, "colliders {} and {} overlap", left.id, right.id);
        }
    }
}

#[test]
fn every_spawn_slot_stands_clear_inside_the_hub_plaza() {
    let definition = greyhaven_vale_definition();
    for collider in definition.colliders() {
        if collider.id == ids::GROUND {
            continue;
        }
        let (min, max) = footprint(collider);
        let plaza = mmorpg_core::XzBounds { min, max };
        assert!(
            !plaza.overlaps(SPAWN_PLAZA),
            "collider {} is in the plaza",
            collider.id
        );
    }
    let body = PLAYER_HALF_EXTENTS_UNITS;
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition).unwrap();
    for player_id in 1..=MAX_PLAYERS_PER_ZONE {
        zone.add_player(u32::try_from(player_id).unwrap()).unwrap();
    }
    let players = zone.snapshot().unwrap().players;
    let mut positions = std::collections::BTreeSet::new();
    for player in &players {
        let [x, y, z] = mmorpg_core::physics::to_units(player.position);
        assert_eq!(y, body[1], "feet rest on y = 0");
        assert!(SPAWN_PLAZA.contains(x - body[0], z - body[2]));
        assert!(SPAWN_PLAZA.contains(x + body[0] - 1, z + body[2] - 1));
        assert!(positions.insert((x, z)));
        assert_eq!(area_at(x, z).map(Area::id), Some(greyhaven_vale::OUTPOST));
    }
    zone.advance_tick().unwrap();
    assert_eq!(
        zone.snapshot().unwrap().players,
        players,
        "a full plaza rests"
    );
}

#[test]
fn roads_are_open_corridors() {
    let definition = greyhaven_vale_definition();
    for road in roads() {
        assert!(road.points.len() >= 2, "{}", road.name);
        for point in road.points {
            // Road ends keep clear of the boundary walls too.
            let margin = ROAD_CLEARANCE_UNITS;
            assert!(PLAYABLE_BOUNDS.contains(point[0] - margin, point[1] - margin));
            assert!(PLAYABLE_BOUNDS.contains(point[0] + margin, point[1] + margin));
        }
    }
    for collider in definition.colliders() {
        if !is_terrain(collider) {
            assert!(
                clear_of_roads(collider),
                "collider {} blocks a road",
                collider.id
            );
        }
    }
}

#[test]
fn road_clearance_is_exact_at_the_boundary() {
    let post = |x: i32, z: i32| StaticCollider {
        id: 99,
        position: [x, 50, z],
        half_extents: [10, 50, 10],
    };
    // Beside the Hollow Road (x = 0) and beyond its northern end (z = −8 600).
    assert!(clear_of_roads(&post(210, -5_000)));
    assert!(!clear_of_roads(&post(209, -5_000)));
    assert!(!clear_of_roads(&post(0, -5_000)), "standing on the road");
    assert!(clear_of_roads(&post(0, -8_810)));
    assert!(!clear_of_roads(&post(0, -8_809)));
    // A long box straddling the road without an endpoint near it.
    let wall = StaticCollider {
        id: 99,
        position: [0, 50, -6_000],
        half_extents: [2_000, 50, 10],
    };
    assert!(!clear_of_roads(&wall));
}

#[test]
fn palisade_gates_are_clear_openings_centred_on_their_roads() {
    let definition = greyhaven_vale_definition();
    let bounds = |id: u32| {
        footprint(
            definition
                .colliders()
                .iter()
                .find(|collider| collider.id == id)
                .unwrap(),
        )
    };
    // (palisade segments, gate posts, axis across the opening, road centre on
    // that axis): the Hollow Road and South Road run along x = 0, the Wolfrun
    // Trail and Millbrook Road along z = 20 m.
    for (segments, posts, axis, road) in [
        ([110, 111], [120, 121], 0, 0),
        ([112, 113], [122, 123], 0, 0),
        ([114, 115], [124, 125], 1, 2_000),
        ([116, 117], [126, 127], 1, 2_000),
    ] {
        // A 6 m palisade gap, framed by 0.8 m posts standing inside it.
        let gap = [bounds(segments[0]).1[axis], bounds(segments[1]).0[axis]];
        assert_eq!(gap[1] - gap[0], 600, "palisade gap {segments:?}");
        let (first, second) = (bounds(posts[0]), bounds(posts[1]));
        assert_eq!([first.0[axis], second.1[axis]], gap, "posts {posts:?}");
        let opening = [first.1[axis], second.0[axis]];
        assert_eq!(
            opening[1] - opening[0],
            440,
            "gate {posts:?} is 4.4 m clear"
        );
        assert_eq!(
            opening[0] + opening[1],
            2 * road,
            "gate {posts:?} is centred"
        );
    }
}

#[test]
fn wolfrun_woods_trunks_always_leave_a_walkable_gap() {
    let definition = greyhaven_vale_definition();
    let trees: Vec<_> = definition
        .colliders()
        .iter()
        .filter(|collider| ids::TREES.contains(&collider.id))
        .collect();
    let first_id = *ids::TREES.start();
    for (index, tree) in trees.iter().enumerate() {
        assert_eq!(tree.id, first_id + u32::try_from(index).unwrap());
        assert!((30..=45).contains(&tree.half_extents[0]));
        assert_eq!(tree.half_extents[0], tree.half_extents[2]);
        // Wolfrun Woods first, then the outer valley's woods (#159).
        let area = area_at(tree.position[0], tree.position[2]).map(Area::id);
        assert!(
            area.is_some_and(|area| area == greyhaven_vale::WOLFRUN_WOODS || area.get() >= 6),
            "tree {} in {area:?}",
            tree.id
        );
    }
    let body_width = 2 * PLAYER_HALF_EXTENTS_UNITS[0];
    for (index, left) in trees.iter().enumerate() {
        let (left_min, left_max) = footprint(left);
        for right in &trees[index + 1..] {
            let (right_min, right_max) = footprint(right);
            let gap = |axis: usize| {
                (right_min[axis] - left_max[axis]).max(left_min[axis] - right_max[axis])
            };
            assert!(
                gap(0).max(gap(1)) >= 150 && gap(0).max(gap(1)) > body_width,
                "trunks {} and {} are too close",
                left.id,
                right.id
            );
        }
    }
}

#[test]
fn named_areas_are_ordered_disjoint_and_contain_their_landmarks() {
    let areas = areas().areas();
    let names: Vec<_> = areas.iter().map(Area::name).collect();
    assert_eq!(
        names,
        [
            "Greyhaven Outpost",
            "Wolfrun Woods",
            "Millbrook Farm",
            "Stillwater Lake",
            "Redbrand Hollow",
            "Greyfang Ridge",
            "Southmeadow Downs",
            "Hollowpine Reach",
            "Eastwater Fen"
        ]
    );
    // Area bounds are inclusive; as half-open rectangles they compare with
    // the rest of the content.
    let bounds = |area: &Area| XzBounds {
        min: area.min_xz(),
        max: area.max_xz().map(|value| value + 1),
    };
    for (index, area) in areas.iter().enumerate() {
        assert_eq!(area.id(), AreaId::new(u16::try_from(index + 1).unwrap()));
        assert!(PLAYABLE_BOUNDS.contains(area.min_xz()[0], area.min_xz()[1]));
        assert!(PLAYABLE_BOUNDS.contains(area.max_xz()[0], area.max_xz()[1]));
        for other in &areas[index + 1..] {
            assert!(
                !bounds(area).overlaps(bounds(other)),
                "{} / {}",
                area.name(),
                other.name()
            );
        }
    }
    // The authored half-open outpost rectangle [-3 500, 3 500) × [-1 300, 5 300).
    assert_eq!(
        area_at(3_499, 5_299).map(Area::id),
        Some(greyhaven_vale::OUTPOST)
    );
    assert_eq!(
        area_at(-3_500, -1_300).map(Area::id),
        Some(greyhaven_vale::OUTPOST)
    );
    assert_eq!(area_at(3_500, 0), None);
    assert_eq!(area_at(0, 5_300), None);
    // Approximate subzone centres from docs/STARTER_ZONE.md, in metres × 100.
    for (point, area) in [
        ([0, 2_000], greyhaven_vale::OUTPOST),
        ([-7_500, 0], greyhaven_vale::WOLFRUN_WOODS),
        ([7_000, 3_500], greyhaven_vale::MILLBROOK_FARM),
        ([5_500, -5_500], greyhaven_vale::STILLWATER_LAKE),
        ([0, -9_000], greyhaven_vale::REDBRAND_HOLLOW),
    ] {
        assert_eq!(area_at(point[0], point[1]).map(Area::id), Some(area));
    }
    assert_eq!(area_at(0, 11_000), None, "the south road is open country");
    let definition = greyhaven_vale_definition();
    for (id, area) in [
        (ids::KEEP, greyhaven_vale::OUTPOST),
        (ids::BARN, greyhaven_vale::MILLBROOK_FARM),
        (ids::MINE_ENTRANCE, greyhaven_vale::REDBRAND_HOLLOW),
        (*ids::SHORE_ROCKS.start(), greyhaven_vale::STILLWATER_LAKE),
    ] {
        let collider = definition
            .colliders()
            .iter()
            .find(|collider| collider.id == id)
            .unwrap();
        assert_eq!(
            area_at(collider.position[0], collider.position[2]).map(Area::id),
            Some(area),
            "collider {id}"
        );
    }
}

#[test]
fn structures_and_boundary_walls_block_movement() {
    let mut zone =
        ZoneSimulation::with_definition(ZoneId::new(1), greyhaven_vale_definition()).unwrap();
    // Slot 0 stands at (−15.5 m, 12.5 m); the keep's south face is at z = 7 m.
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, run(FACE_NORTH)).unwrap();
    for _ in 0..40 {
        zone.advance_tick().unwrap();
    }
    let player = &zone.snapshot().unwrap().players[0];
    assert_eq!(
        mmorpg_core::physics::to_units(player.position),
        [-1_550, 90, 730]
    );
    assert_eq!(mmorpg_core::physics::to_units(player.velocity), [0, 0, 0]);

    // Down the South Road, then jumping against the southern wall.
    let mut zone =
        ZoneSimulation::with_definition(ZoneId::new(1), greyhaven_vale_definition()).unwrap();
    zone.add_player(1).unwrap();
    let mut snapshot = zone.snapshot().unwrap();
    // On the downs' hills (#159): start standing on the ground.
    let ground = greyhaven_vale::hill_height(0, 16_000);
    snapshot.players[0].position = mmorpg_core::physics::from_units([0, ground + 92, 16_000]);
    let mut zone = ZoneSimulation::from_snapshot(snapshot, Arc::clone(zone.content())).unwrap();
    zone.apply_command(1, 1, run(FACE_SOUTH)).unwrap();
    let mut sequence = 1;
    for tick in 0..240 {
        if tick % 20 == 0 {
            sequence += 1;
            zone.apply_command(1, sequence, ZoneCommand::Jump).unwrap();
        }
        zone.advance_tick().unwrap();
        let z = mmorpg_core::physics::to_units(zone.snapshot().unwrap().players[0].position)[2];
        assert!(z <= PLAYABLE_BOUNDS.max[1] - PLAYER_HALF_EXTENTS_UNITS[2]);
    }
    let player = &zone.snapshot().unwrap().players[0];
    assert_eq!(
        mmorpg_core::physics::to_units(player.position)[2],
        PLAYABLE_BOUNDS.max[1] - PLAYER_HALF_EXTENTS_UNITS[2]
    );
}
