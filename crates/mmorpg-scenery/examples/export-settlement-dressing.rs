//! Offline: the region's walk-in cottages (#818, #843) and farmhouses (#886) → committed world
//! dressing, built from the Outpost's house and farmhouse compositions
//! (`content/greyhaven/authoring/outpost-buildings.json`) in asset-tooling's Outpost kit, with
//! their doorways left open, and furnished from its `settlement.props` furniture at
//! `greyhaven_vale::cottages::Plan::furniture`. The zone's collision is
//! `greyhaven_vale::cottages`; runtime clients read only the result.
//!
//! `cargo run -p mmorpg-scenery --example export-settlement-dressing > content/greyhaven/dressing/settlements.json`
use mmorpg_core::{
    greyhaven_vale::{self, cottages, farmland, settlements, village_props},
    trig::YAW_QUARTER_TURN,
};
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
    let mut placements = Vec::new();
    for cottage in cottages::cottages() {
        let template = cottage.plan.template();
        let house = compositions
            .templates
            .get(template)
            .ok_or(format!("no {template} composition"))?;
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
        for (index, piece) in cottage.plan.furniture().iter().enumerate() {
            let [px, pz] = cottage.spot(piece.position);
            let name = piece.asset.rsplit('.').next().unwrap_or(piece.asset);
            // A farmhouse has two benches.
            let id = match cottage.plan {
                cottages::Plan::Cottage => format!("settlement-{}-{name}", cottage.house),
                cottages::Plan::Farmhouse => format!("settlement-{}-{name}-{index}", cottage.house),
            };
            placements.push(json!({
                "id": id,
                "asset": piece.asset,
                "position": [px, cottage.floor, pz],
                "yaw": yaw(piece.quarters),
                "scale": [1000, 1000, 1000]
            }));
        }
    }
    // The settlements' and hamlets' wells, barrels, crates, carts and ring stones (#851): the
    // props stand in over their structures' boxes, which keep colliding, scaled to them.
    let terrain = greyhaven_vale::terrain();
    let structures = (*greyhaven_vale::ids::SETTLEMENTS.start()..)
        .zip(settlements::SETTLEMENTS.iter().flat_map(|s| s.structures))
        .map(|(id, &(kind, centre, _, height))| {
            use settlements::Building as B;
            let kind = match kind {
                B::Well => "well",
                B::Barrel => "barrel",
                B::Crate => "crate",
                B::Cart => "cart",
                B::Stone => "standing-stone",
                _ => "",
            };
            (id, kind, centre, height)
        });
    let wells = (*greyhaven_vale::ids::FARMLAND_BUILDINGS.start()..)
        .zip(farmland::BUILDINGS)
        .filter(|(_, (kind, ..))| *kind == farmland::Building::Well)
        .map(|(id, &(_, min, max, height))| {
            let centre = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2];
            (id, "well", centre, height)
        });
    for (id, kind, [x, z], height) in structures.chain(wells) {
        // Quarter turns and scale (permille) to the box: the cart's length lies along its own Z,
        // the box's along X; a crate box is larger than the prop, a stone smaller.
        let (quarters, scale) = match kind {
            "well" | "barrel" => (0_u16, [1000, 1000, 1000]),
            "crate" => (0, [1390, 1430, 1390]),
            "cart" => (1, [900, 1000, 900]),
            "standing-stone" => (0, [490, height * 1000 / 240, 490]),
            _ => continue,
        };
        placements.push(json!({
            "id": format!("settlement-{id}-{kind}"),
            "asset": format!("settlement.props.{kind}"),
            "position": [x, terrain.height_at(x, z), z],
            "yaw": quarters * YAW_QUARTER_TURN,
            "scale": scale,
            "fallbackCollider": id
        }));
    }
    // The villages' clutter (#851), solid as greyhaven_vale::village_props.
    // The second batch (#891) as well, on its floors where it has one.
    let batches = [
        ("village", &village_props::PROPS[..]),
        ("settlement-prop", &village_props::SETTLEMENT_PROPS[..]),
    ];
    for (prefix, props) in batches {
        for (index, prop) in props.iter().enumerate() {
            placements.push(json!({
                "id": format!("{prefix}-{index}-{}", prop.name),
                "asset": format!("settlement.props.{}", prop.name),
                "position": [prop.position[0], prop.ground(), prop.position[1]],
                "yaw": u16::from(prop.quarters) * YAW_QUARTER_TURN,
                "scale": [1000, 1000, 1000]
            }));
        }
    }
    println!(
        "{}",
        mmorpg_scenery::layout::dressing_json("settlements", &placements)?
    );
    Ok(())
}
