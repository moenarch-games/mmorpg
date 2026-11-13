//! Offline Outpost layout + composition data → committed world dressing (#683).
//! Runtime clients read only the committed result; hosts read only existing core collision.
use mmorpg_core::greyhaven_vale;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Module {
    id: String,
    asset: String,
    position: [i32; 3],
    quarters: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Building {
    collider: u32,
    template: String,
    half: [i32; 2],
    quarters: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Compositions {
    templates: BTreeMap<String, Vec<Module>>,
    buildings: Vec<Building>,
}

fn turn([x, z]: [i32; 2], quarters: u8) -> [i32; 2] {
    match quarters % 4 {
        0 => [x, z],
        1 => [z, -x],
        2 => [-x, -z],
        _ => [-z, x],
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(path)?,
        None => greyhaven_vale::layout::SOURCE.to_owned(),
    };
    let definition = greyhaven_vale::layout::compile(&source)?;
    let scenery = mmorpg_scenery::layout::compose(&source, &definition)?;
    let mut compositions: Compositions = serde_json::from_str(include_str!(
        "../../../content/greyhaven/authoring/outpost-buildings.json"
    ))?;
    // Authored cottages use the existing layout's house prefab, outside the runtime renderer.
    compositions
        .buildings
        .extend(scenery.props.iter().filter_map(|prop| {
            let collider = prop.collider.filter(|id| (60_000..65_000).contains(id))?;
            Some(Building {
                collider,
                template: "house".to_owned(),
                half: [400, 300],
                quarters: if prop.position[2] >= 2800 { 2 } else { 0 },
            })
        }));
    let mut placements = Vec::new();
    for building in compositions.buildings {
        let Some(prop) = scenery
            .props
            .iter()
            .find(|p| p.collider == Some(building.collider))
        else {
            // Layout tombstones suppress base houses.
            continue;
        };
        let [hx, _, hz] = prop.half_extents;
        if [hx, hz] != turn(building.half, building.quarters).map(i32::abs) {
            return Err(format!(
                "{}: composition footprint differs from collision",
                building.collider
            )
            .into());
        }
        let [cx, cy, cz] = prop.position;
        let cy = cy + scenery.height_at(cx, cz);
        let modules = compositions
            .templates
            .get(&building.template)
            .ok_or("unknown Outpost composition")?;
        let wear = (building.collider.wrapping_mul(0x9E37_79B1) >> 24) as u8;
        for module in modules {
            let [x, y, z] = module.position;
            let [dx, dz] = turn([x, z], building.quarters);
            placements.push(json!({
                "id": format!("outpost-{}-{}", building.collider, module.id),
                "asset": module.asset,
                "position": [cx + dx, cy + y, cz + dz],
                "yaw": u16::from((module.quarters + building.quarters) % 4) * mmorpg_core::trig::YAW_QUARTER_TURN,
                "scale": [1000, 1000, 1000],
                "fallbackCollider": building.collider,
                "wear": wear
            }));
        }
    }
    placements.push(json!({
        "id": "outpost-authored-settlement", "asset": "greyhaven.fort.settlement",
        "position": [0, 0, 0], "yaw": 0, "scale": [1000, 1000, 1000]
    }));
    println!("{{\n  \"version\": 1,\n  \"area\": \"outpost\",\n  \"placements\": [");
    for (index, placement) in placements.iter().enumerate() {
        println!(
            "    {}{}",
            serde_json::to_string(placement)?,
            if index + 1 == placements.len() {
                ""
            } else {
                ","
            }
        );
    }
    println!("  ]\n}}");
    Ok(())
}
