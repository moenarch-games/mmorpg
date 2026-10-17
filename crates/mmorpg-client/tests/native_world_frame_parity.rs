//! World-only differential; unit presentation remains a separate required binary.
#[path = "support/draw_comparison.rs"]
mod comparison;
#[path = "support/world_reference.rs"]
mod reference;
use comparison::{close, compare_draws, vector};
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
        );
        compare_draws(&frame.draws, &expected["draws"], &output["resources"]);
        let env = &frame.environment;
        let want = &expected["environment"];
        for (a, b) in env.background.iter().zip(rgb(&want["background"])) {
            close(*a, f64::from(b), name);
        }
        for (a, b) in env.fog.color.iter().zip(rgb(&want["fog"]["color"])) {
            close(*a, f64::from(b), name);
        }
        close(env.fog.near, want["fog"]["near"].as_f64().unwrap(), name);
        close(env.fog.far, want["fog"]["far"].as_f64().unwrap(), name);
        vector(&env.sun.direction, &want["sun"]["direction"], name);
        for (a, b) in env.sun.color.iter().zip(rgb(&want["sun"]["color"])) {
            close(*a, f64::from(b), name);
        }
        close(
            env.sun.intensity,
            want["sun"]["intensity"].as_f64().unwrap(),
            name,
        );
        for (a, b) in env.sky.sky_color.iter().zip(rgb(&want["sky"]["skyColor"])) {
            close(*a, f64::from(b), name);
        }
        for (a, b) in env
            .sky
            .ground_color
            .iter()
            .zip(rgb(&want["sky"]["groundColor"]))
        {
            close(*a, f64::from(b), name);
        }
        close(
            env.sky.intensity,
            want["sky"]["intensity"].as_f64().unwrap(),
            name,
        );
        for (color, key) in [
            (&env.sky_stops.zenith, "zenith"),
            (&env.sky_stops.upper, "upper"),
            (&env.sky_stops.horizon, "horizon"),
            (&env.sky_stops.ground, "ground"),
        ] {
            for (a, b) in color.iter().zip(rgb(&expected["sky_stops"][key])) {
                close(*a, f64::from(b), name);
            }
        }
        let next = world.frame(
            request["seconds"].as_f64().unwrap() + 0.25,
            request["animate"].as_bool().unwrap(),
            eye,
        );
        let mut resources = BTreeMap::new();
        for draw in frame.draws.iter().chain(&next.draws) {
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
