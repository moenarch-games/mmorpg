//! Versioned presentation scenery for the browser's `SceneryProvider`.
//!
//! The export is JSON (format [`SCENERY_FORMAT`], version
//! [`SCENERY_FORMAT_VERSION`]) so `mmorpg-core` stays free of serde. It maps
//! the hosted content's `mmorpg-scenery` value, the same one the native
//! client draws, onto that format:
//!
//! - **terrain**: [`Scenery::terrain_grid`] every [`TERRAIN_STEP_UNITS`]
//!   over ±200 m, with one biome ID per sample, and **farTerrain**: the
//!   same relief every [`FAR_TERRAIN_STEP_UNITS`] over ±600 m for a coarse
//!   ring of distant mountains around it. The browser renderer has no vertex
//!   colours, so clients draw one surface per biome in its base colour;
//! - **props**: a kind table (`propKinds`, [`PropKind::name`]) and one
//!   compact record per scenery prop: `[kind, x, feetY, z, yaw,
//!   scalePermille, halfX, halfY, halfZ]` in units. The feet stand on the
//!   relief; `yaw` is the prop's `u16` yaw and the half extents its
//!   ground-level body box before yaw. Clients build a model per kind
//!   around that anchor;
//! - **structures**: for every prop that visualises a core collider, the
//!   prop index and the exact collider box (centre and half extents), so
//!   clients draw buildings where physics has them;
//! - **roads** (centre lines and visual half width), **water** and the
//!   core's named **areas**.
//!
//! Compact records keep the payload lean: about 0.3 MB for the vale's
//! ~4 200 props and both terrain grids.
//!
//! [`relief_at`] answers from the same `Scenery::height_at`, so browser and
//! native units stand on the same ground. None of this is gameplay: the
//! local zone host ([`crate::host`]) never reads it, and network hosts never
//! link `mmorpg-scenery`.

use std::sync::OnceLock;

use mmorpg_core::{
    PLAYER_HALF_EXTENTS_UNITS, StaticCollider, UNITS_PER_METRE, ZoneAreas, greyhaven_vale,
    greyhaven_vale_definition,
};
use mmorpg_scenery::{PropKind, Scenery, TerrainGrid, greyhaven_vale_scenery};
use serde::Serialize;

pub const SCENERY_FORMAT: &str = "mmorpg.scenery";
pub const SCENERY_FORMAT_VERSION: u32 = 3;
/// Where this export's props and terrain come from.
pub const SCENERY_SOURCE: &str = "mmorpg-scenery";
/// Terrain sample spacing: 4 m, 101 × 101 samples over ±200 m.
pub const TERRAIN_STEP_UNITS: i32 = 400;
/// Far ring sample spacing: 20 m, 61 × 61 samples over ±600 m.
pub const FAR_TERRAIN_STEP_UNITS: i32 = 2_000;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneryExport {
    pub format: &'static str,
    pub version: u32,
    pub source: &'static str,
    /// Decimal `u64`, so JavaScript never rounds it.
    pub content_revision: String,
    /// Presentation-only identity; mask edits do not change gameplay content.
    pub presentation_fingerprint: String,
    pub units_per_metre: i32,
    /// The player collision box from core, so clients place feet correctly.
    pub player_half_extents: [i32; 3],
    pub terrain: Terrain,
    pub far_terrain: Terrain,
    pub biomes: Vec<Biome>,
    /// Prop kind names; a record's first field indexes this table.
    pub prop_kinds: Vec<&'static str>,
    pub props: Vec<PropRecord>,
    pub structures: Vec<Structure>,
    pub roads: Vec<RoadExport>,
    pub water: Vec<Water>,
    pub areas: Vec<AreaExport>,
}

/// A regular height grid. Sample `(column, row)` lies at
/// `origin_xz + step × (column, row)`; arrays are row-major (Z outer).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Terrain {
    pub origin_xz: [i32; 2],
    pub step: i32,
    pub columns: u32,
    pub rows: u32,
    /// Presentation height per sample in units.
    pub heights: Vec<i32>,
    /// Biome ID per sample.
    pub biomes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Biome {
    pub id: u8,
    pub name: &'static str,
    pub color: String,
}

/// `[kind, x, feetY, z, yaw, scalePermille, halfX, halfY, halfZ]` in units.
pub type PropRecord = [i32; 9];

/// A prop that visualises a core collider, with that collider's exact box.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Structure {
    /// Index into `props`.
    pub prop: u32,
    pub collider_id: u32,
    pub center: [i32; 3],
    pub half_extents: [i32; 3],
}

/// A road surface along a core centre line.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoadExport {
    pub name: &'static str,
    pub half_width: i32,
    pub points: Vec<[i32; 2]>,
}

/// A flat elliptical water surface.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Water {
    pub center_xz: [i32; 2],
    pub radii_xz: [i32; 2],
    pub surface_y: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaExport {
    pub id: u16,
    pub name: String,
    pub min_xz: [i32; 2],
    pub max_xz: [i32; 2],
}

/// The areas of the hosted content.
pub fn hosted_areas() -> &'static ZoneAreas {
    greyhaven_vale::areas()
}

/// The presentation scenery of the hosted content, built once.
pub fn hosted_scenery() -> &'static Scenery {
    static SCENERY: OnceLock<Scenery> = OnceLock::new();
    SCENERY.get_or_init(greyhaven_vale_scenery)
}

/// Presentation relief at `(x, z)` in units; clients raise units by it.
#[must_use]
pub fn relief_at(x: i32, z: i32) -> i32 {
    hosted_scenery().height_at(x, z)
}

#[must_use]
pub fn hosted_export() -> SceneryExport {
    // The hosted content definition, which `crate::host` also loads; the
    // scenery was derived from the same colliders.
    let definition = greyhaven_vale_definition();
    export(hosted_scenery(), definition.colliders(), hosted_areas())
}

/// The serialized export. Serializing plain data cannot fail.
#[must_use]
pub fn hosted_scenery_json() -> &'static str {
    static JSON: OnceLock<String> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::to_string(&hosted_export()).expect("scenery export is plain serializable data")
    })
}

fn export(scenery: &Scenery, colliders: &[StaticCollider], areas: &ZoneAreas) -> SceneryExport {
    let structures = scenery
        .props
        .iter()
        .enumerate()
        .filter_map(|(index, prop)| {
            let id = prop.collider?;
            let collider = colliders.iter().find(|collider| collider.id == id)?;
            Some(Structure {
                prop: u32::try_from(index).expect("prop count fits u32"),
                collider_id: id,
                center: collider.position,
                half_extents: collider.half_extents,
            })
        })
        .collect();
    let mut export = SceneryExport {
        format: SCENERY_FORMAT,
        version: SCENERY_FORMAT_VERSION,
        source: SCENERY_SOURCE,
        content_revision: scenery.content_revision.to_string(),
        presentation_fingerprint: String::new(),
        units_per_metre: UNITS_PER_METRE,
        player_half_extents: PLAYER_HALF_EXTENTS_UNITS,
        terrain: terrain(&scenery.terrain_grid(TERRAIN_STEP_UNITS)),
        far_terrain: terrain(&scenery.far_terrain_grid(FAR_TERRAIN_STEP_UNITS)),
        biomes: mmorpg_scenery::Biome::ALL
            .iter()
            .map(|biome| Biome {
                id: biome.id(),
                name: biome.name(),
                color: hex(biome.base_color()),
            })
            .collect(),
        prop_kinds: PropKind::ALL.iter().map(|kind| kind.name()).collect(),
        props: scenery
            .props
            .iter()
            .map(|prop| record(scenery, prop))
            .collect(),
        structures,
        roads: scenery
            .roads
            .iter()
            .map(|road| RoadExport {
                name: road.name,
                half_width: road.half_width,
                points: road.points.clone(),
            })
            .collect(),
        water: scenery
            .water
            .iter()
            .map(|water| Water {
                center_xz: water.centre,
                radii_xz: water.radii,
                surface_y: water.surface,
            })
            .collect(),
        areas: areas
            .areas()
            .iter()
            .map(|area| AreaExport {
                id: area.id().get(),
                name: area.name().to_owned(),
                min_xz: area.min_xz(),
                max_xz: area.max_xz(),
            })
            .collect(),
    };
    refresh_presentation_fingerprint(&mut export);
    export
}

/// Hash every exported field at the client's actual sample resolutions.
/// The fingerprint's own field is empty during hashing, avoiding recursion.
fn refresh_presentation_fingerprint(export: &mut SceneryExport) {
    export.presentation_fingerprint.clear();
    let bytes = serde_json::to_vec(export).expect("scenery export is plain serializable data");
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    export.presentation_fingerprint = format!("{hash:016x}");
}

fn terrain(grid: &TerrainGrid) -> Terrain {
    let count = |samples: usize| u32::try_from(samples).expect("terrain sample count fits u32");
    Terrain {
        origin_xz: grid.origin,
        step: grid.step,
        columns: count(grid.columns),
        rows: count(grid.rows),
        heights: grid.heights.clone(),
        biomes: grid.biomes.iter().map(|biome| biome.id()).collect(),
    }
}

/// The index of a kind in the exported kind table.
fn kind_index(kind: PropKind) -> i32 {
    let index = PropKind::ALL
        .iter()
        .position(|candidate| *candidate == kind)
        .expect("PropKind::ALL lists every kind");
    i32::try_from(index).expect("kind table fits i32")
}

/// The compact record of a prop, its feet standing on the relief.
fn record(scenery: &Scenery, prop: &mmorpg_scenery::Prop) -> PropRecord {
    let [x, feet, z] = prop.position;
    let [half_x, half_y, half_z] = prop.half_extents;
    [
        kind_index(prop.kind),
        x,
        feet + scenery.height_at(x, z),
        z,
        i32::from(prop.yaw),
        i32::from(prop.scale_permille),
        half_x,
        half_y,
        half_z,
    ]
}

fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02x}{green:02x}{blue:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{MAX_PLAYERS_PER_ZONE, greyhaven_vale::SPAWN_GRID};
    use mmorpg_scenery::{FAR_TERRAIN_EXTENT_UNITS, TERRAIN_EXTENT_UNITS, visual_kind};
    use serde_json::Value;

    use crate::host::hosted_definition;

    #[test]
    fn scenery_comes_from_the_hosted_content_revision() {
        let definition = hosted_definition();
        assert_eq!(hosted_scenery().content_revision, definition.revision());
        assert_eq!(
            hosted_export().content_revision,
            definition.revision().to_string()
        );
    }

    #[test]
    fn every_structure_collider_is_one_structure_with_its_exact_box() {
        let definition = hosted_definition();
        let export = hosted_export();
        let mut visualised: Vec<u32> = export
            .structures
            .iter()
            .map(|structure| structure.collider_id)
            .collect();
        visualised.sort_unstable();
        let structures: Vec<u32> = definition
            .colliders()
            .iter()
            .filter(|collider| visual_kind(collider).is_some())
            .map(|collider| collider.id)
            .collect();
        assert_eq!(visualised, structures, "one structure per visual collider");
        assert!(
            visualised
                .iter()
                .all(|id| *id != greyhaven_vale::ids::GROUND
                    && !greyhaven_vale::ids::BOUNDARY_WALLS.contains(id)),
            "the ground is terrain and the boundary walls are invisible"
        );
        for structure in &export.structures {
            let id = structure.collider_id;
            let collider = definition
                .colliders()
                .iter()
                .find(|collider| collider.id == id)
                .unwrap();
            assert_eq!(structure.center, collider.position, "collider {id}");
            assert_eq!(
                structure.half_extents, collider.half_extents,
                "collider {id}"
            );
            let prop = export.props[usize::try_from(structure.prop).unwrap()];
            let kind = PropKind::ALL[usize::try_from(prop[0]).unwrap()];
            assert_eq!(Some(kind), visual_kind(collider), "collider {id}");
            // The prop's feet are the bottom centre of its collider on flat relief.
            assert_eq!(
                [prop[1], prop[2], prop[3]],
                [
                    collider.position[0],
                    collider.position[1] - collider.half_extents[1],
                    collider.position[2]
                ],
                "collider {id}"
            );
            assert_eq!(prop[4], 0, "collider {id} is not turned");
            assert_eq!([prop[6], prop[7], prop[8]], collider.half_extents);
        }
    }

    #[test]
    fn every_scenery_prop_is_one_record_standing_on_the_relief() {
        let scenery = hosted_scenery();
        let export = hosted_export();
        assert_eq!(export.props.len(), scenery.props.len());
        assert_eq!(export.prop_kinds.len(), PropKind::ALL.len());
        for (record, prop) in export.props.iter().zip(&scenery.props) {
            let [x, feet, z] = prop.position;
            let kind = usize::try_from(record[0]).unwrap();
            assert_eq!(export.prop_kinds[kind], prop.kind.name());
            assert_eq!(
                record[1..],
                [
                    x,
                    feet + relief_at(x, z),
                    z,
                    i32::from(prop.yaw),
                    i32::from(prop.scale_permille),
                    prop.half_extents[0],
                    prop.half_extents[1],
                    prop.half_extents[2],
                ]
            );
        }
    }

    fn assert_samples_relief(terrain: &Terrain, step: i32, extent: i32) {
        let side = u32::try_from(2 * extent / step + 1).unwrap();
        assert_eq!((terrain.columns, terrain.rows), (side, side));
        assert_eq!(terrain.step, step);
        assert_eq!(terrain.origin_xz, [-extent, -extent]);
        let samples = usize::try_from(side * side).unwrap();
        assert_eq!(terrain.heights.len(), samples);
        assert_eq!(terrain.biomes.len(), samples);
        let columns = usize::try_from(terrain.columns).unwrap();
        let offset = |samples: usize| step * i32::try_from(samples).unwrap();
        for (index, height) in terrain.heights.iter().enumerate() {
            let x = terrain.origin_xz[0] + offset(index % columns);
            let z = terrain.origin_xz[1] + offset(index / columns);
            assert_eq!(*height, relief_at(x, z), "sample {index}");
        }
    }

    #[test]
    fn terrain_samples_the_shared_relief_and_biomes() {
        let export = hosted_export();
        let terrain = &export.terrain;
        assert_samples_relief(terrain, TERRAIN_STEP_UNITS, TERRAIN_EXTENT_UNITS);
        assert_eq!((terrain.columns, terrain.rows), (101, 101));
        let biome_ids: Vec<u8> = export.biomes.iter().map(|biome| biome.id).collect();
        assert!(terrain.biomes.iter().all(|id| biome_ids.contains(id)));
        assert!(
            terrain.heights.iter().any(|height| *height > 2_000),
            "mountains beyond the walls"
        );
        let columns = usize::try_from(terrain.columns).unwrap();
        let sample = |x: i32, z: i32| {
            let column = usize::try_from((x - terrain.origin_xz[0]) / TERRAIN_STEP_UNITS).unwrap();
            let row = usize::try_from((z - terrain.origin_xz[1]) / TERRAIN_STEP_UNITS).unwrap();
            terrain.biomes[row * columns + column]
        };
        assert_eq!(
            sample(0, 2_000),
            mmorpg_scenery::Biome::Plaza.id(),
            "the hub plaza"
        );
    }

    #[test]
    fn the_far_ring_samples_the_same_relief_more_coarsely() {
        let export = hosted_export();
        let far = &export.far_terrain;
        assert_samples_relief(far, FAR_TERRAIN_STEP_UNITS, FAR_TERRAIN_EXTENT_UNITS);
        assert_eq!((far.columns, far.rows), (61, 61));
        let biome_ids: Vec<u8> = export.biomes.iter().map(|biome| biome.id).collect();
        assert!(far.biomes.iter().all(|id| biome_ids.contains(id)));
        let highest_near = export.terrain.heights.iter().max().unwrap();
        assert!(far.heights.iter().max().unwrap() > highest_near);
    }

    #[test]
    fn presentation_identity_covers_intervening_terrain_samples_and_all_geometry_inputs() {
        let original = hosted_export();
        let mutations: [fn(&mut SceneryExport); 10] = [
            // These 4 m / 20 m samples are absent from the old 10 m / 60 m hash.
            |value| value.terrain.heights[1] += 1,
            |value| value.far_terrain.heights[1] += 1,
            |value| value.terrain.biomes[1] = (value.terrain.biomes[1] + 1) % 13,
            |value| value.far_terrain.biomes[1] = (value.far_terrain.biomes[1] + 1) % 13,
            |value| value.terrain.step += 1,
            |value| value.props[0][1] += 1,
            |value| value.biomes[0].color = "#123456".to_owned(),
            |value| value.structures[0].center[0] += 1,
            |value| value.roads[0].half_width += 1,
            |value| value.water[0].surface_y += 1,
        ];
        for mutate in mutations {
            let mut changed = original.clone();
            mutate(&mut changed);
            refresh_presentation_fingerprint(&mut changed);
            assert_ne!(
                changed.presentation_fingerprint,
                original.presentation_fingerprint
            );
            assert_eq!(changed.content_revision, original.content_revision);
        }
        let mut repeated = original.clone();
        refresh_presentation_fingerprint(&mut repeated);
        assert_eq!(
            repeated.presentation_fingerprint,
            original.presentation_fingerprint
        );
    }

    #[test]
    fn every_spawn_slot_stands_on_flat_relief() {
        for slot in 0..MAX_PLAYERS_PER_ZONE {
            let [x, _, z] = SPAWN_GRID.feet(u16::try_from(slot).unwrap()).unwrap();
            assert_eq!(relief_at(x, z), 0, "spawn slot {slot}");
        }
    }

    #[test]
    fn export_is_versioned_deterministic_lean_json_with_core_areas_and_roads() {
        let json = hosted_scenery_json();
        assert_eq!(json, serde_json::to_string(&hosted_export()).unwrap());
        assert!(json.len() < 1_000_000, "{} bytes", json.len());
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["format"], SCENERY_FORMAT);
        assert_eq!(value["version"], SCENERY_FORMAT_VERSION);
        assert_eq!(value["source"], SCENERY_SOURCE);
        assert_eq!(value["contentRevision"], "9");
        assert_eq!(value["presentationFingerprint"], "3d83424fd061c312");
        assert_eq!(value["unitsPerMetre"], 100);
        assert_eq!(value["playerHalfExtents"], serde_json::json!([30, 90, 30]));
        let names: Vec<_> = value["areas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|area| area["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "Greyhaven Outpost",
                "Wolfrun Woods",
                "Millbrook Farm",
                "Stillwater Lake",
                "Redbrand Hollow"
            ]
        );
        assert_eq!(
            value["areas"][0]["maxXz"],
            serde_json::json!([3_499, 5_299]),
            "inclusive core bounds"
        );
        assert_eq!(value["propKinds"][0], "keep");
        assert_eq!(value["propKinds"][13], "tree-pine");
        assert_eq!(value["biomes"][6]["name"], "plaza");
        assert_eq!(value["biomes"][6]["color"], "#96886e");
        assert_eq!(
            value["water"],
            serde_json::json!([
                { "centerXz": [5_500, -5_500], "radiiXz": [2_200, 1_600], "surfaceY": 15 }
            ])
        );
        assert_eq!(value["roads"][0]["name"], "Hollow Road");
        assert_eq!(value["roads"][0]["halfWidth"], 150);
        assert_eq!(
            value["roads"][0]["points"],
            serde_json::json!([[0, 1_200], [0, -1_000], [0, -4_000], [0, -8_600]])
        );
        assert_eq!(value["structures"][0]["colliderId"], 100);
    }

    /// The local zone host is gameplay; scenery must never feed back into it.
    #[test]
    fn the_local_zone_host_never_reads_presentation_scenery() {
        let host = include_str!("host.rs");
        assert!(!host.contains("mmorpg_scenery") && !host.contains("crate::scenery"));
    }
}
