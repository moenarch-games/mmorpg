//! Development-only input for `content/greyhaven/authoring/basin_rim.py` (#1195): what the
//! village basin's rock band is fitted over, as JSON on stdout. Coordinates are in units (100
//! per metre).
//!
//! - `lattice`: the zone terrain's samples (4 m) round the basin, which the band must hide
//!   wherever the terrain steps from the floor to the rim;
//! - `rock`: [`basin::rock_q10`] on a 1 m grid, whose smooth level at [`basin::STEP_Q10`] is the
//!   band's outline;
//! - `lanes`: the six climbs' lanes, which stay terrain and keep clear of the band;
//! - `roads`: the zone's road centre lines crossing the box, for review;
//! - `units`: where creatures and NPCs stand in the box, which the band keeps clear of;
//! - `softShoulder`: the soft southern shoulder's boxes, where the band leaves the slope bare.
//!
//! `cargo run -p mmorpg-scenery --example export-basin-context > artifacts/basin/context.json`
use mmorpg_core::greyhaven_vale::{self, basin, terrain};
use serde_json::json;

/// The box round the basin: x and z, min and max (units), on the terrain lattice.
const MIN: [i32; 2] = [-8_000, -9_200];
const MAX: [i32; 2] = [8_000, 12_000];
const FINE: i32 = 100;

fn main() {
    let grid = greyhaven_vale::terrain();
    let step = grid.step;
    let first = [
        (MIN[0] - grid.origin[0]) / step,
        (MIN[1] - grid.origin[1]) / step,
    ];
    let count = [(MAX[0] - MIN[0]) / step + 1, (MAX[1] - MIN[1]) / step + 1];
    let heights: Vec<i32> = (0..count[1])
        .flat_map(|row| (0..count[0]).map(move |column| (column, row)))
        .map(|(column, row)| {
            let index = (first[1] + row) * i32::from(grid.columns) + first[0] + column;
            grid.heights[usize::try_from(index).expect("the box lies on the grid")]
        })
        .collect();
    let fine = [(MAX[0] - MIN[0]) / FINE + 1, (MAX[1] - MIN[1]) / FINE + 1];
    let rock: Vec<i64> = (0..fine[1])
        .flat_map(|row| (0..fine[0]).map(move |column| (column, row)))
        .map(|(column, row)| basin::rock_q10(MIN[0] + column * FINE, MIN[1] + row * FINE))
        .collect();
    let lanes: Vec<_> = basin::CLIMB
        .iter()
        .chain(&basin::STEPS)
        .chain(&basin::LAKESHORE)
        .chain(&basin::WOLFRUN)
        .chain(&basin::HOLLOW)
        .chain(&basin::SOUTH)
        .map(|lane| {
            json!({
                "min": lane.min,
                "max": lane.max,
                "heights": lane.heights,
                "along": if lane.along == basin::Along::X { "x" } else { "z" },
            })
        })
        .collect();
    let inside =
        |[x, z]: [i32; 2]| (MIN[0]..=MAX[0]).contains(&x) && (MIN[1]..=MAX[1]).contains(&z);
    let roads: Vec<_> = greyhaven_vale::roads()
        .iter()
        .filter(|road| road.points.iter().any(|&p| inside(p)))
        .map(|road| json!({"name": road.name, "points": road.points}))
        .collect();
    let units: Vec<_> = greyhaven_vale::standing_points()
        .into_iter()
        .filter(|&point| inside(point))
        .collect();
    assert_eq!(step, terrain::STEP);
    println!(
        "{}",
        json!({
            "lattice": {"origin": MIN, "step": step, "columns": count[0], "rows": count[1], "heights": heights},
            "rock": {"origin": MIN, "step": FINE, "columns": fine[0], "rows": fine[1], "values": rock, "level": basin::STEP_Q10},
            "terrace": basin::TERRACE_Y,
            "lanes": lanes,
            "roads": roads,
            "units": units,
            "softShoulder": basin::SOFT_SHOULDER,
        })
    );
}
