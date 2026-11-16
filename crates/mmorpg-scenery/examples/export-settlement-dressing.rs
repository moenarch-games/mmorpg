//! Offline: the region's walk-in cottages (#818, #843) → committed world dressing, built from the
//! Outpost's house composition (`content/greyhaven/authoring/outpost-buildings.json`) in
//! asset-tooling's Outpost kit, with its doorway left open, and furnished from its
//! `settlement.props` furniture at `greyhaven_vale::cottages::FURNITURE`. The zone's collision is
//! `greyhaven_vale::cottages`; runtime clients read only the result.
//!
//! `cargo run -p mmorpg-scenery --example export-settlement-dressing > content/greyhaven/dressing/settlements.json`
use mmorpg_core::{greyhaven_vale::cottages, trig::YAW_QUARTER_TURN};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Module {
    id: String,
    asset: String,
    position: [i32; 3],
    quarters: u8,
}

#[derive(Deserialize)]
struct Compositions {
    templates: BTreeMap<String, Vec<Module>>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let compositions: Compositions = serde_json::from_str(include_str!(
        "../../../content/greyhaven/authoring/outpost-buildings.json"
    ))?;
    let house = compositions
        .templates
        .get("house")
        .ok_or("no house composition")?;
    let mut placements = Vec::new();
    for cottage in cottages::cottages() {
        let wear = (cottage.house.wrapping_mul(0x9E37_79B1) >> 24) as u8;
        let yaw = |quarters: u8| u16::from((quarters + cottage.quarters) % 4) * YAW_QUARTER_TURN;
        // The doorway stays open: the zone has no door there.
        for module in house
            .iter()
            .filter(|module| module.asset != "outpost.kit.door")
        {
            let [x, y, z] = module.position;
            let [px, pz] = cottage.spot([x, z]);
            placements.push(json!({
                "id": format!("settlement-{}-{}", cottage.house, module.id),
                "asset": module.asset,
                "position": [px, cottage.floor + y, pz],
                "yaw": yaw(module.quarters),
                "scale": [1000, 1000, 1000],
                "fallbackCollider": cottage.house,
                "wear": wear
            }));
        }
        for piece in &cottages::FURNITURE {
            let [px, pz] = cottage.spot(piece.position);
            let name = piece.asset.rsplit('.').next().unwrap_or(piece.asset);
            placements.push(json!({
                "id": format!("settlement-{}-{name}", cottage.house),
                "asset": piece.asset,
                "position": [px, cottage.floor, pz],
                "yaw": yaw(piece.quarters),
                "scale": [1000, 1000, 1000]
            }));
        }
    }
    println!("{{\n  \"version\": 1,\n  \"area\": \"settlements\",\n  \"placements\": [");
    for (index, placement) in placements.iter().enumerate() {
        let comma = if index + 1 == placements.len() {
            ""
        } else {
            ","
        };
        println!("    {}{comma}", serde_json::to_string(placement)?);
    }
    println!("  ]\n}}");
    Ok(())
}
