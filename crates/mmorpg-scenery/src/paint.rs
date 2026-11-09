//! Ground painted in an image editor (`docs/WORLD_LAYOUT_EDITOR.md`): biome overrides on the
//! zone's terrain grid, and the samples a painted height may not move. An empty committed file
//! leaves the generated ground untouched.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use mmorpg_core::greyhaven_vale::{self, brook, cave, ids, layout, shrine, terrain};
use serde::Deserialize;

use crate::{Biome, Scenery, is_dressing};

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../content/greyhaven/paint/biome.json"
));

/// The biomes a brush may lay down, each with its colour in the painted layer. Roads, water and
/// the plaza follow their own geometry and always win over paint.
pub const PAINTABLE: &[(Biome, &str, [u8; 3])] = &[
    (Biome::Meadow, "meadow", [120, 200, 80]),
    (Biome::Woods, "woods", [20, 110, 50]),
    (Biome::Hollow, "hollow", [120, 90, 160]),
    (Biome::Farmland, "farmland", [200, 150, 60]),
    (Biome::Hub, "hub", [210, 190, 140]),
    (Biome::Foothills, "foothills", [170, 170, 90]),
    (Biome::Highland, "highland", [130, 120, 80]),
    (Biome::Rock, "rock", [120, 120, 125]),
    (Biome::Snow, "snow", [240, 245, 255]),
];
/// How far from a building, wall or fence the ground must stay as authored, in units.
const LOCK_MARGIN_UNITS: i32 = 400;
/// A collider wider than this both ways is a ground slab under the world, not a structure.
const GROUND_SLAB_UNITS: i32 = 3_000;
/// How far from open water the ground must stay as authored, in units.
const WATER_LOCK_UNITS: i64 = 400;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BiomePaint {
    version: u32,
    /// `[column, row, biome id]`, ordered by row, then column.
    cells: Vec<[i32; 3]>,
}

fn parse(source: &str) -> Result<BTreeMap<(i32, i32), Biome>, String> {
    let paint: BiomePaint = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if paint.version != 1 {
        return Err(format!("unknown painted biome version {}", paint.version));
    }
    let mut cells = BTreeMap::new();
    for [column, row, id] in paint.cells {
        if !(0..i32::from(terrain::COLUMNS)).contains(&column)
            || !(0..i32::from(terrain::ROWS)).contains(&row)
        {
            return Err(format!("painted cell [{column}, {row}] is off the terrain"));
        }
        let (biome, ..) = PAINTABLE
            .iter()
            .find(|(biome, ..)| i32::from(biome.id()) == id)
            .ok_or_else(|| format!("painted cell [{column}, {row}] has biome {id}"))?;
        if cells.insert((row, column), *biome).is_some() {
            return Err(format!("painted cell [{column}, {row}] is repeated"));
        }
    }
    Ok(cells)
}

static CELLS: LazyLock<BTreeMap<(i32, i32), Biome>> =
    LazyLock::new(|| parse(SOURCE).expect("committed painted biomes are valid"));

/// The painted biome of the terrain sample nearest `(x, z)`, if any.
pub(crate) fn biome_at(x: i32, z: i32) -> Option<Biome> {
    let cell =
        |value: i32, origin: i32| (value - origin + terrain::STEP / 2).div_euclid(terrain::STEP);
    let column = cell(x, terrain::ORIGIN[0]);
    let row = cell(z, terrain::ORIGIN[1]);
    CELLS.get(&(row, column)).copied()
}

/// Every painted biome as `(column, row, biome id)`, ordered by row, then column.
pub fn biome_cells() -> impl Iterator<Item = (i32, i32, u8)> {
    CELLS
        .iter()
        .map(|(&(row, column), biome)| (column, row, biome.id()))
}

/// Per terrain sample (row-major), whether a painted height must leave it alone: under or
/// beside structures that stand on authored heights, at water, on the flat Outpost and in the
/// cave and shrine. Trees, loose rocks and dressing follow the ground, so they do not lock it.
#[must_use]
pub fn locked_cells(scenery: &Scenery) -> Vec<bool> {
    let columns = i32::from(terrain::COLUMNS);
    let rows = i32::from(terrain::ROWS);
    let position = |column: i32, row: i32| {
        [
            terrain::ORIGIN[0] + column * terrain::STEP,
            terrain::ORIGIN[1] + row * terrain::STEP,
        ]
    };
    let mut locked = Vec::with_capacity(usize::from(terrain::COLUMNS) * usize::from(terrain::ROWS));
    for row in 0..rows {
        for column in 0..columns {
            let [x, z] = position(column, row);
            locked.push(
                layout::REGION.contains(x, z)
                    || brook::wet_ground(x, z)
                    || shrine::flattens(x, z)
                    || cave::GREYFANG.flattens(x, z)
                    || scenery.water_distance([x, z]) < WATER_LOCK_UNITS,
            );
        }
    }
    // The base colliders: the Outpost layout's own overrides stand on already-locked ground.
    let definition = greyhaven_vale::base_definition();
    for collider in definition.colliders() {
        if ids::TREES.contains(&collider.id)
            || ids::LOOSE_ROCKS.contains(&collider.id)
            || is_dressing(collider.id)
            || collider.half_extents[0].min(collider.half_extents[2]) > GROUND_SLAB_UNITS
        {
            continue;
        }
        let reach = |axis: usize, origin: i32, count: i32| {
            let index = |value: i32| (value - origin).div_euclid(terrain::STEP);
            let side = collider.half_extents[axis * 2] + LOCK_MARGIN_UNITS;
            let centre = collider.position[axis * 2];
            index(centre - side).max(0)..(index(centre + side) + 2).min(count)
        };
        for row in reach(1, terrain::ORIGIN[1], rows) {
            for column in reach(0, terrain::ORIGIN[0], columns) {
                locked[usize::try_from(row * columns + column).expect("in the grid")] = true;
            }
        }
    }
    locked
}

#[cfg(test)]
mod tests {
    use mmorpg_core::greyhaven_vale::{paint, terrain};

    use super::{locked_cells, parse};
    use crate::greyhaven_vale_scenery;

    #[test]
    fn parses_cells_and_rejects_what_cannot_be_painted() {
        assert_eq!(
            parse(r#"{"version":1,"cells":[[3,2,11]]}"#).unwrap().len(),
            1
        );
        for bad in [
            r#"{"version":2,"cells":[]}"#,
            r#"{"version":1,"cells":[[-1,0,11]]}"#,
            r#"{"version":1,"cells":[[0,0,5]]}"#,
            r#"{"version":1,"cells":[[0,0,11],[0,0,12]]}"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn committed_height_paint_leaves_locked_ground_alone() {
        let locked = locked_cells(&greyhaven_vale_scenery());
        // The Outpost, the first thing a lock must protect, is locked at its centre.
        let centre = (2_000 - terrain::ORIGIN[1]) / terrain::STEP * i32::from(terrain::COLUMNS)
            - terrain::ORIGIN[0] / terrain::STEP;
        assert!(locked[usize::try_from(centre).unwrap()]);
        for (column, row, delta) in paint::height_cells() {
            let index = row * i32::from(terrain::COLUMNS) + column;
            assert!(
                !locked[usize::try_from(index).unwrap()],
                "[{column}, {row}] moved by {delta}"
            );
        }
    }
}
