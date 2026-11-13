//! Offline layout + market art data → committed world dressing (#683).
use mmorpg_core::greyhaven_vale;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Market {
    anchor_house: u32,
    placements: Vec<Placement>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Placement {
    id: String,
    asset: String,
    position: [i32; 3],
    yaw: u16,
    scale: [u16; 3],
    #[serde(rename = "relativeToHouse", skip_serializing)]
    relative_to_house: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(path)?,
        None => greyhaven_vale::layout::SOURCE.to_owned(),
    };
    let definition = greyhaven_vale::layout::compile(&source)?;
    let scenery = mmorpg_scenery::layout::compose(&source, &definition)?;
    let market: Market = serde_json::from_str(include_str!(
        "../../../content/greyhaven/authoring/market-props.json"
    ))?;
    let anchor = definition
        .colliders()
        .iter()
        .find(|c| c.id == market.anchor_house);
    let mut ids = BTreeSet::new();
    let mut placed = Vec::new();
    if let Some(anchor) = anchor {
        for mut p in market.placements {
            if !ids.insert(p.id.clone())
                || !p.asset.starts_with("settlement.props.")
                || p.scale.contains(&0)
            {
                return Err(format!("{}: invalid market placement", p.id).into());
            }
            if p.relative_to_house {
                p.position[0] += anchor.position[0];
                p.position[2] += anchor.position[2];
            }
            let [x, _, z] = p.position;
            p.position[1] += scenery.surface_at(x, z);
            placed.push(p);
        }
    }
    println!("{{\n  \"version\": 1,\n  \"area\": \"outpost-market\",\n  \"placements\": [");
    for (index, p) in placed.iter().enumerate() {
        println!(
            "    {}{}",
            serde_json::to_string(p)?,
            if index + 1 == placed.len() { "" } else { "," }
        );
    }
    println!("  ]\n}}");
    Ok(())
}
