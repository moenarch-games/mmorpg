//! World-only differential; unit presentation remains a separate required binary.
#[path = "support/draw_comparison.rs"]
mod comparison;
#[path = "support/world_reference.rs"]
mod reference;
use comparison::{close, compare_draws};
use mmorpg_client::world::WorldScene;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
fn rgb(value: &Value) -> [f32; 3] {
    let text = value.as_str().unwrap();
    let color = u32::from_str_radix(&text[1..], 16).unwrap();
    [
        ((color >> 16) & 255) as f32 / 255.0,
        ((color >> 8) & 255) as f32 / 255.0,
        (color & 255) as f32 / 255.0,
    ]
}
#[test]
fn every_world_prop_region_and_day_phase_matches_pages_submitted_content() {
    let (cases, output) = reference::shared();
    for (request, expected) in cases.iter().zip(output["scenes"].as_array().unwrap()) {
        if request["kind"] != "world" {
            continue;
        }
        let name = request["name"].as_str().unwrap();
        eprintln!("world reference {name}");
        let world = WorldScene::new(&mmorpg_scenery::greyhaven_vale_scenery());
        let eye = std::array::from_fn(|i| request["eye"][i].as_f64().unwrap() as f32);
        let frame = world.frame(
            request["seconds"].as_f64().unwrap(),
            request["animate"].as_bool().unwrap(),
            eye,
            None,
        );
        let draws: Vec<_> = frame.all_draws().cloned().collect();
        compare_draws(&draws, &expected["draws"], &output["resources"]);
        // Both clients light with render-engine (#529; the browser through its three.js port, #600)
        // and fog alike: the same palette, the same aerial perspective (#526,
        // `FOG_VISIBILITY_METRES` in web/src/world/lighting.ts).
        let fog = &frame.lighting.fog;
        let want = &expected["environment"]["fog"];
        for (a, b) in fog.color.iter().zip(rgb(&want["color"])) {
            close(*a, f64::from(b), name);
        }
        close(fog.visibility, 3200.0, name);
        let next = world.frame(
            request["seconds"].as_f64().unwrap() + 0.25,
            request["animate"].as_bool().unwrap(),
            eye,
            None,
        );
        let mut resources = BTreeMap::new();
        for draw in frame.all_draws().chain(next.all_draws()) {
            if let Some(previous) = resources.insert(&draw.resource_key, &draw.mesh) {
                assert!(
                    Arc::ptr_eq(previous, &draw.mesh),
                    "immutable geometry copied for {}",
                    draw.resource_key
                );
            }
        }
    }
}
