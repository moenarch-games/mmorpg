//! Terrain height painted in an image editor (`docs/WORLD_LAYOUT_EDITOR.md`): whole-grid offsets
//! over the generated ground. `mmorpg-world-studio import` writes the committed file; an
//! empty list leaves the generated terrain untouched.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use super::terrain;

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../content/greyhaven/paint/height.json"
));
/// One step of the painted layer's grey value, in units (10 cm).
pub const HEIGHT_STEP_UNITS: i32 = 10;
/// The greatest painted offset either way, in units: the layer's 8-bit range.
pub const MAX_HEIGHT_DELTA_UNITS: i32 = 127 * HEIGHT_STEP_UNITS;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeightPaint {
    version: u32,
    /// `[column, row, offset in units]`, ordered by row, then column.
    cells: Vec<[i32; 3]>,
}

fn parse(source: &str) -> Result<BTreeMap<(i32, i32), i32>, String> {
    let paint: HeightPaint = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if paint.version != 1 {
        return Err(format!("unknown painted height version {}", paint.version));
    }
    let mut cells = BTreeMap::new();
    for [column, row, delta] in paint.cells {
        if !(0..i32::from(terrain::COLUMNS)).contains(&column)
            || !(0..i32::from(terrain::ROWS)).contains(&row)
        {
            return Err(format!("painted cell [{column}, {row}] is off the terrain"));
        }
        if delta == 0 || delta.abs() > MAX_HEIGHT_DELTA_UNITS || delta % HEIGHT_STEP_UNITS != 0 {
            return Err(format!("painted cell [{column}, {row}] has offset {delta}"));
        }
        if cells.insert((row, column), delta).is_some() {
            return Err(format!("painted cell [{column}, {row}] is repeated"));
        }
    }
    Ok(cells)
}

static CELLS: LazyLock<BTreeMap<(i32, i32), i32>> =
    LazyLock::new(|| parse(SOURCE).expect("committed painted height is valid"));

/// The painted offset of one terrain sample, in units.
#[must_use]
pub fn height_delta(column: i32, row: i32) -> i32 {
    CELLS.get(&(row, column)).copied().unwrap_or(0)
}

/// Every painted sample as `(column, row, offset in units)`, ordered by row, then column.
pub fn height_cells() -> impl Iterator<Item = (i32, i32, i32)> {
    CELLS
        .iter()
        .map(|(&(row, column), &delta)| (column, row, delta))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_cells_and_rejects_what_the_grid_cannot_hold() {
        let cells = parse(r#"{"version":1,"cells":[[3,2,-40]]}"#).unwrap();
        assert_eq!(cells[&(2, 3)], -40);
        for bad in [
            r#"{"version":2,"cells":[]}"#,
            r#"{"version":1,"cells":[[-1,0,10]]}"#,
            r#"{"version":1,"cells":[[0,0,0]]}"#,
            r#"{"version":1,"cells":[[0,0,15]]}"#,
            r#"{"version":1,"cells":[[0,0,10],[0,0,20]]}"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
