//! #182: observe the same application frames the native GPU adapter consumes.
#[path = "support/world_reference.rs"]
mod reference;
use mmorpg_client::{
    camera::OrbitCamera,
    presentation::Presentation,
    world::{SceneDraw, WorldScene},
};
use mmorpg_core::{
    AbilityId, AuraKind, AuraView, CastView, CreatureId, EntityFlags, EntityKind, EntityRef,
    EntitySnapshot, NpcId, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc, time::Instant};

fn close(actual: f32, expected: f64, label: &str) {
    // TRS composition and f64→f32 conversion, rather than a visual/performance budget.
    let tolerance = f32::EPSILON as f64 * 32.0 * expected.abs().max(1.0);
    assert!(
        (actual as f64 - expected).abs() <= tolerance,
        "{label}: {actual} != {expected}"
    );
}
fn vector(actual: &[f32], expected: &Value, label: &str) {
    let values = expected.as_array().unwrap();
    assert_eq!(actual.len(), values.len(), "{label}");
    for (a, b) in actual.iter().zip(values) {
        close(*a, b.as_f64().unwrap(), label);
    }
}
fn rgb(value: &Value) -> [f32; 3] {
    let text = value.as_str().unwrap();
    let color = u32::from_str_radix(&text[1..], 16).unwrap();
    [
        ((color >> 16) & 255) as f32 / 255.0,
        ((color >> 8) & 255) as f32 / 255.0,
        (color & 255) as f32 / 255.0,
    ]
}
fn digest(mesh: &three_d_core::Mesh) -> String {
    let mut hash = 14695981039346656037_u64;
    let mut bytes = |values: &[u8]| {
        for byte in values {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(1099511628211);
        }
    };
    let positions: Vec<[f32; 3]> = mesh.vertices().iter().map(|v| [v.x, v.y, v.z]).collect();
    let normals = mesh
        .attributes()
        .normals
        .as_ref()
        .map(|values| values.iter().map(|v| [v.x, v.y, v.z]).collect::<Vec<_>>());
    let colors = mesh
        .attributes()
        .colors
        .as_ref()
        .map(|values| values.iter().map(|v| [v.r, v.g, v.b]).collect::<Vec<_>>());
    for (name, values) in [
        ("positions", Some(positions.as_slice())),
        ("normals", normals.as_deref()),
        ("colors", colors.as_deref()),
    ] {
        bytes(name.as_bytes());
        bytes(&[u8::from(values.is_some())]);
        if let Some(values) = values {
            bytes(&u32::try_from(values.len()).unwrap().to_le_bytes());
            for v in values {
                for n in v {
                    bytes(&n.to_le_bytes());
                }
            }
        }
    }
    bytes(&u32::try_from(mesh.indices().len()).unwrap().to_le_bytes());
    for index in mesh.indices() {
        bytes(&index.to_le_bytes());
    }
    format!("{hash:016x}")
}
fn compare_draws(actual: &[SceneDraw], expected: &Value, resources: &Value) {
    let by_id: BTreeMap<_, _> = actual.iter().map(|draw| (draw.id.as_str(), draw)).collect();
    assert_eq!(by_id.len(), actual.len(), "duplicate actual draw IDs");
    for draw in expected.as_array().unwrap() {
        let id = draw["id"].as_str().unwrap();
        let actual = by_id
            .get(id)
            .unwrap_or_else(|| panic!("missing actual submitted draw {id}"));
        vector(&actual.model.elements, &draw["model"], id);
        let material = &draw["material"];
        vector(&actual.material.color, &material["color"], id);
        vector(&actual.material.emissive, &material["emissive"], id);
        close(
            actual.material.opacity,
            material["opacity"].as_f64().unwrap(),
            id,
        );
        assert_eq!(
            actual.material.unlit,
            material["unlit"].as_bool().unwrap(),
            "{id}"
        );
        assert_eq!(
            actual.material.double_sided,
            material["double_sided"].as_bool().unwrap(),
            "{id}"
        );
        let resource = &resources[draw["reference_resource"].as_str().unwrap()];
        assert!(!actual.mesh.indices().is_empty(), "{id}: empty geometry");
        for axis in 0..3 {
            let values = actual.mesh.vertices().iter().map(|v| [v.x, v.y, v.z][axis]);
            let (low, high) = values.fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), v| {
                (low.min(v), high.max(v))
            });
            vector(&[low, high], &resource["bounds"][axis], id);
        }
        // Asset topology is an authored contract; procedural tessellation may legitimately differ.
        if draw["source_key"]
            .as_str()
            .is_some_and(|key| key.starts_with("asset-tooling:"))
        {
            assert_eq!(
                digest(&actual.mesh),
                resource["digest"].as_str().unwrap(),
                "normalized asset {id}"
            );
            assert!(
                actual
                    .resource_key
                    .contains(draw["source_key"].as_str().unwrap()),
                "asset identity lost: {id}"
            );
        }
    }
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
        let mut scenery = mmorpg_scenery::greyhaven_vale_scenery();
        if let Some(kind) = request["propKind"].as_str() {
            scenery.props.retain(|p| p.kind.name() == kind);
        }
        let world = WorldScene::new(&scenery);
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

#[test]
fn native_unit_draws_match_current_pages_models_and_appearance_inputs() {
    let (cases, output) = reference::shared();
    let now = Instant::now();
    for (request, expected) in cases.iter().zip(output["scenes"].as_array().unwrap()) {
        if request["kind"] != "units" {
            continue;
        }
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let class = match request["classId"].as_str().unwrap() {
            "warden" => 0,
            "ranger" => 1,
            "arcanist" => 2,
            _ => panic!("bad fixture class"),
        };
        let sex = u8::from(request["sex"] == "male");
        zone.apply_command(1, 1, ZoneCommand::ChooseClass { class, sex })
            .unwrap();
        zone.advance_tick().unwrap();
        let mut projection = zone.snapshot_for_player(1).unwrap();
        projection.entities = request["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entity| {
                let flags = &entity["flags"];
                EntitySnapshot {
                    kind: match entity["kind"].as_str().unwrap() {
                        "player" => EntityKind::Player,
                        "creature" => EntityKind::Creature,
                        "npc" => EntityKind::Npc,
                        _ => panic!("bad fixture kind"),
                    },
                    id: entity["entityId"].as_u64().unwrap().try_into().unwrap(),
                    appearance: entity["appearance"].as_u64().unwrap().try_into().unwrap(),
                    position: std::array::from_fn(|i| {
                        entity["position"][i].as_i64().unwrap().try_into().unwrap()
                    }),
                    velocity: std::array::from_fn(|i| {
                        entity["velocity"][i].as_i64().unwrap().try_into().unwrap()
                    }),
                    facing: entity["facing"].as_u64().unwrap().try_into().unwrap(),
                    level: entity["level"].as_u64().unwrap().try_into().unwrap(),
                    health_percent: entity["healthPercent"]
                        .as_u64()
                        .unwrap()
                        .try_into()
                        .unwrap(),
                    flags: EntityFlags {
                        dead: flags["dead"].as_bool().unwrap(),
                        hostile: flags["hostile"].as_bool().unwrap(),
                        attackable: flags["attackable"].as_bool().unwrap(),
                        tapped_by_other: flags["tappedByOther"].as_bool().unwrap(),
                        ..EntityFlags::default()
                    },
                }
            })
            .collect();
        projection.viewer.target = entity_ref(&request["target"]);
        projection.viewer.cast = cast(&request["cast"]);
        if let Some(events) = request["events"].as_array() {
            projection.events = events
                .iter()
                .map(|event| match event["kind"].as_str().unwrap() {
                    "ability-used" => ZoneEvent::AbilityUsed {
                        source: entity_ref(&event["source"]).unwrap(),
                        target: entity_ref(&event["target"]),
                        ability: AbilityId::new(
                            event["ability"].as_u64().unwrap().try_into().unwrap(),
                        ),
                    },
                    "interrupted" => ZoneEvent::Interrupted {
                        source: entity_ref(&event["source"]),
                        target: entity_ref(&event["target"]).unwrap(),
                        ability: AbilityId::new(
                            event["ability"].as_u64().unwrap().try_into().unwrap(),
                        ),
                    },
                    _ => panic!("unexpected fixture event"),
                })
                .collect();
        }
        if request["name"] == "barrier-aura" {
            projection.auras.push(AuraView {
                ability: AbilityId::new(11),
                kind: AuraKind::Absorb,
                remaining: 60,
                amount: 20,
            });
        }
        let mut presentation =
            Presentation::new(1, mmorpg_scenery::greyhaven_vale_scenery(), content, now).unwrap();
        presentation
            .set_headwear(request["headwear"].as_str())
            .unwrap();
        presentation.set_reduced_motion(!request["animate"].as_bool().unwrap());
        if request["previousCast"].is_object() {
            let mut previous = projection.clone();
            previous.events.clear();
            previous.viewer.cast = cast(&request["previousCast"]);
            presentation.push(previous, now).unwrap();
            projection.tick += 1;
        }
        presentation.push(projection, now).unwrap();
        let view = OrbitCamera::default().view(presentation.camera_target(now));
        compare_draws(
            &presentation.unit_draws(
                now + std::time::Duration::from_secs_f64(
                    request["sampleSeconds"].as_f64().unwrap_or(0.0),
                ),
                view,
            ),
            &expected["draws"],
            &output["resources"],
        );
        assert!(presentation.set_headwear(Some("not-a-real-hat")).is_err());
    }
}

fn entity_ref(value: &Value) -> Option<EntityRef> {
    if value.is_null() {
        return None;
    }
    let id = value["id"].as_u64().unwrap().try_into().unwrap();
    Some(match value["kind"].as_str().unwrap() {
        "player" => EntityRef::Player(id),
        "creature" => EntityRef::Creature(CreatureId::new(id)),
        "npc" => EntityRef::Npc(NpcId::new(id)),
        _ => panic!("bad fixture reference"),
    })
}
fn cast(value: &Value) -> Option<CastView> {
    if value.is_null() {
        return None;
    }
    Some(CastView {
        ability: AbilityId::new(value["ability"].as_u64().unwrap().try_into().unwrap()),
        elapsed: value["elapsed"].as_u64().unwrap().try_into().unwrap(),
        total: value["total"].as_u64().unwrap().try_into().unwrap(),
        channel: value["channel"].as_bool().unwrap(),
    })
}
