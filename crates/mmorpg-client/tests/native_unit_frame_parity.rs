//! Required unit/model differential; never stubbed or ignored by world-only validation.
//! Owner decision Atlas 6.3.6 moves native units to Quaternius models before Pages; this
//! differential is paused until the browser adopts the same models, then compares them again.
#[path = "support/draw_comparison.rs"]
mod comparison;
#[path = "support/draw_identity.rs"]
mod identity;
#[path = "support/world_reference.rs"]
mod reference;
use comparison::compare_draws;
use mmorpg_client::{camera::OrbitCamera, presentation::Presentation};
use mmorpg_core::{
    AbilityId, AuraKind, AuraView, CastView, CreatureId, EntityFlags, EntityKind, EntityRef,
    EntitySnapshot, NpcId, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};
use serde_json::Value;
use std::{sync::Arc, time::Instant};

#[test]
#[ignore = "Atlas 6.3.6: native units use Quaternius models ahead of Pages; re-enable with the browser slice"]
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
        eprintln!(
            "unit reference {} sampleSeconds={} delta={} animate={}",
            request["name"], request["sampleSeconds"], request["delta"], request["animate"]
        );
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
