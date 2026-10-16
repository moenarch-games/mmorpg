//! Greyhaven Vale scenery: deterministic, derived from core colliders, never
//! intersecting physical content it does not visualise, and flat wherever
//! units walk on structures, roads, the plaza or water.
use std::collections::BTreeMap;

use mmorpg_core::greyhaven_vale::{self, PLAYABLE_BOUNDS, SPAWN_GRID, SPAWN_PLAZA, ids};
use mmorpg_core::{Area, MAX_PLAYERS_PER_ZONE, StaticCollider, greyhaven_vale_definition};
use mmorpg_scenery::{
    Biome, FAR_PEAK_UNITS, FAR_TERRAIN_EXTENT_UNITS, LAKE_BED_COLOR, MOUNTAIN_PEAK_UNITS,
    PLAZA_COLOR, PropKind, ROAD_COLOR, Rect, SNOW_COLOR, SNOW_LINE_UNITS, Scenery,
    TERRAIN_EXTENT_UNITS, WALKABLE_RELIEF_UNITS, greyhaven_vale_scenery, visual_kind,
};

/// Recorded from this revision; any change to content, placement or relief
/// must update it deliberately. Saved Outpost grass and relief masks change only
/// presentation, leaving authored geometry unchanged through gameplay revision 5.
const STABLE_HASH: u64 = 0x9068_3364_fa48_a752;

fn is_terrain(collider: &StaticCollider) -> bool {
    collider.id == ids::GROUND || ids::BOUNDARY_WALLS.contains(&collider.id)
}

fn footprint(collider: &StaticCollider) -> Rect {
    Rect::around(
        [collider.position[0], collider.position[2]],
        [collider.half_extents[0], collider.half_extents[2]],
    )
}

fn beyond_playable(x: i32, z: i32) -> i32 {
    (PLAYABLE_BOUNDS.min[0] - x)
        .max(x - PLAYABLE_BOUNDS.max[0])
        .max(PLAYABLE_BOUNDS.min[1] - z)
        .max(z - PLAYABLE_BOUNDS.max[1])
}

#[test]
fn scenery_is_deterministic_with_a_stable_hash() {
    let first = greyhaven_vale_scenery();
    let second = greyhaven_vale_scenery();
    assert_eq!(first, second);
    assert_eq!(first.content_revision, greyhaven_vale::REVISION);
    let hash = first.stable_hash();
    assert_eq!(hash, second.stable_hash());
    assert_eq!(hash, STABLE_HASH, "scenery changed: {hash:#018x}");
}

#[test]
fn authored_grass_retains_stable_ids_and_clears_the_selected_approach() {
    let scenery = greyhaven_vale_scenery();
    let clearing = Rect {
        min: [-2_800, 800],
        max: [-1_800, 1_600],
    };
    let placements: Vec<_> = mmorpg_scenery::outpost_grass_placements().collect();
    assert_eq!(placements.len(), 55);
    let mut ids: Vec<_> = placements.iter().map(|(id, _)| *id).collect();
    let ordered = ids.clone();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids, ordered,
        "saved IDs remain unique and in accepted order"
    );
    for (_, prop) in placements {
        assert!(scenery.props.contains(&prop));
        assert!(prop.collider.is_none());
        assert!(!prop.footprint().intersects(clearing));
    }
    assert_eq!(
        greyhaven_vale::content().fingerprint(),
        0x2630_ca15_7169_73f3
    );
    assert_eq!(scenery.content_revision, 9);
}

#[test]
fn every_structure_collider_has_exactly_one_visual_and_vice_versa() {
    let definition = greyhaven_vale_definition();
    let scenery = greyhaven_vale_scenery();
    let mut visuals: BTreeMap<u32, Vec<PropKind>> = BTreeMap::new();
    for prop in &scenery.props {
        if let Some(id) = prop.collider {
            visuals.entry(id).or_default().push(prop.kind);
        }
    }
    let colliders: BTreeMap<_, _> = definition
        .colliders()
        .iter()
        .map(|collider| (collider.id, collider))
        .collect();
    for collider in definition.colliders() {
        let kinds = visuals.get(&collider.id);
        if is_terrain(collider) {
            assert!(visual_kind(collider).is_none());
            assert!(
                kinds.is_none(),
                "terrain collider {} has a prop",
                collider.id
            );
        } else {
            assert_eq!(
                kinds.map(Vec::as_slice),
                Some(&[visual_kind(collider).unwrap()][..]),
                "collider {}",
                collider.id
            );
        }
    }
    for (id, kinds) in &visuals {
        assert!(colliders.contains_key(id), "prop for missing collider {id}");
        assert_eq!(kinds.len(), 1);
    }
    for prop in scenery.props.iter().filter(|prop| prop.collider.is_some()) {
        let collider = colliders[&prop.collider.unwrap()];
        assert_eq!(prop.half_extents, collider.half_extents);
        assert_eq!(prop.footprint(), footprint(collider));
        assert_eq!(prop.yaw, 0);
    }
}

#[test]
fn no_prop_intersects_a_collider_it_does_not_visualise() {
    let definition = greyhaven_vale_definition();
    let scenery = greyhaven_vale_scenery();
    let structures: Vec<_> = definition
        .colliders()
        .iter()
        .filter(|collider| !is_terrain(collider))
        .collect();
    for prop in &scenery.props {
        assert_eq!(prop.position[1], 0, "feet rest on y = 0 before relief");
        let area = prop.footprint();
        for collider in &structures {
            if prop.collider != Some(collider.id) {
                assert!(
                    !area.intersects(footprint(collider)),
                    "{:?} at {:?} intersects collider {}",
                    prop.kind,
                    prop.position,
                    collider.id
                );
            }
        }
    }
}

#[test]
fn walkable_water_has_no_colliders() {
    let definition = greyhaven_vale_definition();
    let scenery = greyhaven_vale_scenery();
    let [lake] = &scenery.water[..] else {
        panic!("the vale has one lake");
    };
    assert!(lake.contains(5_500, -5_500));
    assert_eq!(
        greyhaven_vale::area_at(lake.centre[0], lake.centre[1]).map(Area::id),
        Some(greyhaven_vale::STILLWATER_LAKE)
    );
    for collider in definition.colliders().iter().filter(|c| !is_terrain(c)) {
        // The nearest footprint point to the centre is the clamped centre.
        let rect = footprint(collider);
        let nearest = [
            lake.centre[0].clamp(rect.min[0], rect.max[0]),
            lake.centre[1].clamp(rect.min[1], rect.max[1]),
        ];
        assert!(
            !lake.contains(nearest[0], nearest[1]),
            "collider {}",
            collider.id
        );
    }
}

#[test]
fn relief_is_flat_under_structures_roads_plaza_spawns_and_water() {
    let definition = greyhaven_vale_definition();
    let scenery = greyhaven_vale_scenery();
    let flat = |x: i32, z: i32| scenery.height_at(x, z) == 0;
    for collider in definition.colliders().iter().filter(|c| !is_terrain(c)) {
        let rect = footprint(collider);
        let xs = [rect.min[0], (rect.min[0] + rect.max[0]) / 2, rect.max[0]];
        let zs = [rect.min[1], (rect.min[1] + rect.max[1]) / 2, rect.max[1]];
        for x in xs {
            for z in zs {
                assert!(flat(x, z), "collider {} at ({x}, {z})", collider.id);
            }
        }
    }
    for road in &scenery.roads {
        for segment in road.points.windows(2) {
            let [a, b] = [segment[0], segment[1]];
            let steps = (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) / 50;
            for step in 0..=steps {
                let x = a[0] + (b[0] - a[0]) * step / steps.max(1);
                let z = a[1] + (b[1] - a[1]) * step / steps.max(1);
                for offset in [-100, 0, 100] {
                    assert!(
                        flat(x + offset, z) && flat(x, z + offset),
                        "{} at ({x}, {z})",
                        road.name
                    );
                }
            }
        }
    }
    for x in (SPAWN_PLAZA.min[0]..=SPAWN_PLAZA.max[0]).step_by(100) {
        for z in (SPAWN_PLAZA.min[1]..=SPAWN_PLAZA.max[1]).step_by(100) {
            assert!(flat(x, z), "plaza ({x}, {z})");
        }
    }
    for slot in 0..MAX_PLAYERS_PER_ZONE {
        let feet = SPAWN_GRID.feet(u16::try_from(slot).unwrap()).unwrap();
        assert!(flat(feet[0], feet[2]));
    }
    let lake = scenery.water[0];
    for x in (lake.centre[0] - lake.radii[0]..=lake.centre[0] + lake.radii[0]).step_by(100) {
        for z in (lake.centre[1] - lake.radii[1]..=lake.centre[1] + lake.radii[1]).step_by(100) {
            if lake.contains(x, z) {
                assert!(flat(x, z), "lake ({x}, {z})");
            }
        }
    }
}

#[test]
fn relief_is_gentle_inside_and_encloses_the_vale_with_mountains() {
    let scenery = greyhaven_vale_scenery();
    let mut rolling = 0;
    for x in (PLAYABLE_BOUNDS.min[0]..=PLAYABLE_BOUNDS.max[0]).step_by(200) {
        for z in (PLAYABLE_BOUNDS.min[1]..=PLAYABLE_BOUNDS.max[1]).step_by(200) {
            let height = scenery.height_at(x, z);
            assert!(
                height.abs() <= WALKABLE_RELIEF_UNITS,
                "({x}, {z}) = {height}"
            );
            rolling += usize::from(height != 0);
        }
    }
    assert!(rolling > 5_000, "the meadows are not flat: {rolling}");
    let grid = scenery.terrain_grid(400);
    let mut peak = i32::MIN;
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            let (vertex, _) = grid.vertex(column, row).unwrap();
            let height = vertex[1];
            assert!((-WALKABLE_RELIEF_UNITS..=MOUNTAIN_PEAK_UNITS).contains(&height));
            if beyond_playable(vertex[0], vertex[2]) >= 4_000 {
                assert!(height >= 1_500, "({}, {}) = {height}", vertex[0], vertex[2]);
            }
            peak = peak.max(height);
        }
    }
    assert!(peak >= 3_500, "mountains peak at {peak}");
}

#[test]
fn terrain_grid_samples_relief_and_biomes() {
    let scenery: Scenery = greyhaven_vale_scenery();
    let grid = scenery.terrain_grid(200);
    assert_eq!(grid.step, 200);
    assert_eq!((grid.columns, grid.rows), (201, 201));
    assert_eq!(grid.origin, [-TERRAIN_EXTENT_UNITS, -TERRAIN_EXTENT_UNITS]);
    assert_eq!(grid.heights.len(), 201 * 201);
    assert_eq!(grid.colors.len(), 201 * 201);
    for (column, row) in [(0, 0), (100, 100), (37, 158), (200, 200)] {
        let (vertex, _) = grid.vertex(column, row).unwrap();
        assert_eq!(vertex[1], scenery.height_at(vertex[0], vertex[2]));
    }
    assert!(grid.vertex(201, 0).is_none());
    let color = |x: i32, z: i32| {
        let column = usize::try_from((x + TERRAIN_EXTENT_UNITS) / 200).unwrap();
        let row = usize::try_from((z + TERRAIN_EXTENT_UNITS) / 200).unwrap();
        grid.vertex(column, row).unwrap().1
    };
    // Road dirt, the plaza, the lake bed, meadows, then rock and far snow on the peaks.
    assert_eq!(color(0, -3_000), ROAD_COLOR);
    assert_eq!(color(0, 8_000), ROAD_COLOR);
    assert_eq!(color(1_000, 1_600), PLAZA_COLOR);
    assert_eq!(color(5_600, -5_600), LAKE_BED_COLOR);
    for meadow in [
        color(-4_000, 8_000),
        color(9_000, -1_000),
        color(-7_000, 3_000),
    ] {
        assert!(![ROAD_COLOR, PLAZA_COLOR, LAKE_BED_COLOR, SNOW_COLOR].contains(&meadow));
        assert!(
            meadow[1] > meadow[0] && meadow[1] > meadow[2],
            "green: {meadow:?}"
        );
    }
    // The near mountains stay below the snow line; the distant ranges carry it.
    let highest = (0..grid.heights.len())
        .max_by_key(|&index| grid.heights[index])
        .unwrap();
    assert!(grid.heights[highest] < SNOW_LINE_UNITS);
    assert_eq!(grid.biomes[highest], Biome::Rock);
    let far = scenery.far_terrain_grid(2_000);
    let highest = (0..far.heights.len())
        .max_by_key(|&index| far.heights[index])
        .unwrap();
    assert_eq!(far.colors[highest], SNOW_COLOR);
}

#[test]
fn terrain_biomes_name_every_vertex_colour() {
    let ids: Vec<u8> = Biome::ALL.iter().map(|biome| biome.id()).collect();
    assert_eq!(
        ids,
        (0..13).collect::<Vec<u8>>(),
        "ids follow declaration order"
    );
    let mut names: Vec<_> = Biome::ALL.iter().map(|biome| biome.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), Biome::ALL.len(), "names are unique");

    let scenery = greyhaven_vale_scenery();
    let grid = scenery.terrain_grid(200);
    assert_eq!(grid.biomes.len(), grid.heights.len());
    let at = |x: i32, z: i32| {
        let column = usize::try_from((x + TERRAIN_EXTENT_UNITS) / 200).unwrap();
        let row = usize::try_from((z + TERRAIN_EXTENT_UNITS) / 200).unwrap();
        let index = row * grid.columns + column;
        (grid.biomes[index], grid.colors[index])
    };
    for (point, biome) in [
        ([0, -3_000], Biome::Road),
        ([1_000, 1_600], Biome::Plaza),
        ([5_600, -5_600], Biome::LakeBed),
        ([-7_000, 3_000], Biome::Woods),
        ([9_000, -1_000], Biome::Meadow),
        ([7_000, 7_000], Biome::Farmland),
        ([2_000, -10_000], Biome::Hollow),
    ] {
        assert_eq!(at(point[0], point[1]).0, biome, "{point:?}");
    }
    // Uniform biomes use their base colour; tinted ones stay within the
    // ±8 tint (value noise / 128) of it.
    for (biome, color) in grid.biomes.iter().zip(&grid.colors) {
        let base = biome.base_color();
        if matches!(
            biome,
            Biome::Road | Biome::Plaza | Biome::LakeBed | Biome::Snow
        ) {
            assert_eq!(*color, base, "{biome:?}");
        } else {
            let offsets: Vec<i32> = (0..3)
                .map(|channel| i32::from(color[channel]) - i32::from(base[channel]))
                .collect();
            assert!(offsets.iter().all(|offset| offset.abs() <= 8), "{biome:?}");
        }
    }
    let far = scenery.far_terrain_grid(2_000);
    let beyond: Vec<_> = grid
        .biomes
        .iter()
        .zip(&grid.heights)
        .chain(far.biomes.iter().zip(&far.heights))
        .filter(|(biome, _)| {
            matches!(
                biome,
                Biome::Foothills | Biome::Highland | Biome::Rock | Biome::Snow
            )
        })
        .collect();
    for band in [Biome::Foothills, Biome::Highland, Biome::Rock, Biome::Snow] {
        assert!(beyond.iter().any(|(biome, _)| **biome == band), "{band:?}");
    }
    assert!(
        beyond
            .iter()
            .all(|(biome, height)| (**biome == Biome::Snow) == (**height >= SNOW_LINE_UNITS))
    );
}

#[test]
fn prop_counts_stay_within_their_budgets() {
    let definition = greyhaven_vale_definition();
    let scenery = greyhaven_vale_scenery();
    let mut counts: BTreeMap<PropKind, usize> = BTreeMap::new();
    let mut background_trees = 0;
    for prop in &scenery.props {
        *counts.entry(prop.kind).or_default() += 1;
        let [x, _, z] = prop.position;
        assert!(x.abs() <= TERRAIN_EXTENT_UNITS && z.abs() <= TERRAIN_EXTENT_UNITS);
        if matches!(prop.kind, PropKind::Tree(_)) && prop.collider.is_none() {
            background_trees += 1;
            assert!(
                beyond_playable(x, z) >= 400,
                "background trees stay outside"
            );
        } else {
            assert!(
                PLAYABLE_BOUNDS.contains(x, z),
                "{:?} at ({x}, {z})",
                prop.kind
            );
        }
    }
    let structures = definition
        .colliders()
        .iter()
        .filter(|collider| !is_terrain(collider))
        .count();
    let derived = scenery
        .props
        .iter()
        .filter(|prop| prop.collider.is_some())
        .count();
    assert_eq!(derived, structures);
    let count = |kind| counts.get(&kind).copied().unwrap_or(0);
    assert!((2_000..=2_400).contains(&count(PropKind::GrassTuft)));
    assert!((400..=700).contains(&count(PropKind::Flowers)));
    assert!((100..=160).contains(&count(PropKind::Bush)));
    assert!((100..=140).contains(&count(PropKind::Reeds)));
    assert!((400..=520).contains(&background_trees));
    assert_eq!(count(PropKind::CropRow), 13);
    assert_eq!(count(PropKind::Lamp), 4);
    assert_eq!(count(PropKind::Signpost), 3);
    assert_eq!(count(PropKind::Barrel), 4);
    assert_eq!(count(PropKind::Cart), 2);
    assert_eq!(count(PropKind::Dock), 1);
    assert_eq!(count(PropKind::Keep), 1);
    assert_eq!(count(PropKind::House), 4);
    assert!(scenery.props.len() <= 6_000, "{}", scenery.props.len());
}

#[test]
fn prop_kind_names_are_unique_and_cover_every_placed_kind() {
    let mut names: Vec<_> = PropKind::ALL.iter().map(|kind| kind.name()).collect();
    assert!(names.iter().all(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
    }));
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), PropKind::ALL.len(), "names are unique");
    let scenery = greyhaven_vale_scenery();
    for prop in &scenery.props {
        assert!(PropKind::ALL.contains(&prop.kind), "{:?}", prop.kind);
    }
}

#[test]
fn the_far_ring_continues_the_same_relief_into_distant_ranges() {
    let scenery = greyhaven_vale_scenery();
    let far = scenery.far_terrain_grid(2_000);
    assert_eq!((far.columns, far.rows), (61, 61));
    assert_eq!(
        far.origin,
        [-FAR_TERRAIN_EXTENT_UNITS, -FAR_TERRAIN_EXTENT_UNITS]
    );
    let near = scenery.terrain_grid(2_000);
    let offset =
        usize::try_from((FAR_TERRAIN_EXTENT_UNITS - TERRAIN_EXTENT_UNITS) / 2_000).unwrap();
    let mut peak = i32::MIN;
    for row in 0..far.rows {
        for column in 0..far.columns {
            let (vertex, color) = far.vertex(column, row).unwrap();
            assert_eq!(vertex[1], scenery.height_at(vertex[0], vertex[2]));
            assert!((-WALKABLE_RELIEF_UNITS..=FAR_PEAK_UNITS).contains(&vertex[1]));
            if let (Some(inner_column), Some(inner_row)) =
                (column.checked_sub(offset), row.checked_sub(offset))
                && let Some(inner) = near.vertex(inner_column, inner_row)
            {
                assert_eq!(
                    (vertex, color),
                    inner,
                    "the far ring samples the same relief"
                );
            }
            peak = peak.max(vertex[1]);
        }
    }
    assert!(
        peak > 8_000,
        "distant ranges tower over the near mountains: {peak}"
    );
    // No cliff at the terrain grid's edge: the ranges start from nothing.
    for z in (-TERRAIN_EXTENT_UNITS..=TERRAIN_EXTENT_UNITS).step_by(500) {
        let edge = scenery.height_at(TERRAIN_EXTENT_UNITS, z);
        let beyond = scenery.height_at(TERRAIN_EXTENT_UNITS + 100, z);
        assert!((beyond - edge).abs() <= 200, "z {z}: {edge} -> {beyond}");
    }
}
