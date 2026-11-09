//! Greyhaven's terrain grid as JSON on stdout, for `mmorpg-world-studio`, the one
//! consumer: heights, biomes, the samples paint may not move, the
//! paintable palette and the committed paint.
use mmorpg_core::greyhaven_vale::{paint as height_paint, terrain};
use mmorpg_scenery::{greyhaven_vale_scenery, paint};
use serde_json::json;

fn main() {
    let scenery = greyhaven_vale_scenery();
    let (columns, rows) = (i32::from(terrain::COLUMNS), i32::from(terrain::ROWS));
    let samples = || {
        (0..rows).flat_map(move |row| {
            (0..columns).map(move |column| {
                (
                    terrain::ORIGIN[0] + column * terrain::STEP,
                    terrain::ORIGIN[1] + row * terrain::STEP,
                )
            })
        })
    };
    let heights: Vec<i32> = samples().map(|(x, z)| scenery.ground_at(x, z)).collect();
    let biomes: Vec<u8> = samples()
        .map(|(x, z)| scenery.biome_of(x, z).id())
        .collect();
    let locked: Vec<u8> = paint::locked_cells(&scenery)
        .into_iter()
        .map(u8::from)
        .collect();
    let palette: Vec<_> = paint::PAINTABLE
        .iter()
        .map(|(biome, name, color)| json!({"id": biome.id(), "name": name, "color": color}))
        .collect();
    let height_cells: Vec<_> = height_paint::height_cells()
        .map(|(c, r, d)| [c, r, d])
        .collect();
    let biome_cells: Vec<_> = paint::biome_cells()
        .map(|(c, r, id)| [c, r, i32::from(id)])
        .collect();
    println!(
        "{}",
        json!({
            "origin": terrain::ORIGIN,
            "step": terrain::STEP,
            "columns": columns,
            "rows": rows,
            "heightStepUnits": height_paint::HEIGHT_STEP_UNITS,
            "heights": heights,
            "biomes": biomes,
            "roadBiome": mmorpg_scenery::Biome::Road.id(),
            "waterBiomes": [mmorpg_scenery::Biome::LakeBed.id(), mmorpg_scenery::Biome::Shore.id()],
            "locked": locked,
            "palette": palette,
            "paintedHeight": height_cells,
            "paintedBiome": biome_cells,
        })
    );
}
