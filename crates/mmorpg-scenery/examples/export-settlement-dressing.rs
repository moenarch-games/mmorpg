//! Offline: the region's settlement houses (#818) → committed world dressing, built from the
//! Outpost's house composition (`content/greyhaven/authoring/outpost-buildings.json`) in
//! asset-tooling's Outpost kit. Each 8 × 6 m house turns its door to its settlement's yard; its
//! box stays the stand-in while the kit is not drawn. Runtime clients read only the result.
//!
//! `cargo run -p mmorpg-scenery --example export-settlement-dressing > content/greyhaven/dressing/settlements.json`
use mmorpg_core::greyhaven_vale::settlements::{self, Building};
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

/// The house template's footprint half extents, in units.
const HALF: [i32; 2] = [400, 300];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let compositions: Compositions = serde_json::from_str(include_str!(
        "../../../content/greyhaven/authoring/outpost-buildings.json"
    ))?;
    let house = compositions
        .templates
        .get("house")
        .ok_or("no house composition")?;
    let scenery = mmorpg_scenery::greyhaven_vale_scenery();
    let mut placements = Vec::new();
    for prop in &scenery.props {
        let Some(collider) = prop.collider else {
            continue;
        };
        if settlements::building(collider) != Some(Building::House)
            || [prop.half_extents[0], prop.half_extents[2]] != HALF
        {
            continue;
        }
        let [cx, cy, cz] = prop.position;
        let yard = settlements::SETTLEMENTS
            .iter()
            .map(|settlement| settlement.yard)
            .find(|(min, max)| (min[0]..=max[0]).contains(&cx) && (min[1]..=max[1]).contains(&cz))
            .ok_or_else(|| format!("house {collider} stands in no settlement yard"))?;
        // The template's door is on its +Z side: turn it a half turn when the yard lies north.
        let quarters: u8 = if (yard.0[1] + yard.1[1]) / 2 >= cz {
            0
        } else {
            2
        };
        let turn = |[x, z]: [i32; 2]| if quarters == 0 { [x, z] } else { [-x, -z] };
        let wear = (collider.wrapping_mul(0x9E37_79B1) >> 24) as u8;
        for module in house {
            let [x, y, z] = module.position;
            let [dx, dz] = turn([x, z]);
            placements.push(json!({
                "id": format!("settlement-{collider}-{}", module.id),
                "asset": module.asset,
                "position": [cx + dx, cy + y, cz + dz],
                "yaw": u16::from((module.quarters + quarters) % 4) * mmorpg_core::trig::YAW_QUARTER_TURN,
                "scale": [1000, 1000, 1000],
                "fallbackCollider": collider,
                "wear": wear
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
