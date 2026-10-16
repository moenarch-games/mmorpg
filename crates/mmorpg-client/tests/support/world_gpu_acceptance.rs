//! Actual adapter consumption and reviewable matched-view evidence for #182.
use super::offscreen_frame_pixels;
use crate::{
    camera::{CameraView, OrbitCamera},
    presentation::Presentation,
    world::WorldScene,
};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale};
use std::{path::Path, sync::Arc, time::Instant};

fn save(name: &str, pixels: &[u8]) {
    if let Some(directory) = std::env::var_os("MMORPG_SMOKE_FRAME_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        let mut bytes = b"P6\n640 360\n255\n".to_vec();
        for pixel in pixels.chunks_exact(4) {
            bytes.extend_from_slice(&pixel[..3]);
        }
        std::fs::write(
            Path::new(&directory).join(format!("parity-{name}.ppm")),
            bytes,
        )
        .unwrap();
    }
}
fn changed(a: &[u8], b: &[u8], label: &str) {
    assert_eq!(a.len(), 640 * 360 * 4);
    assert_eq!(a.len(), b.len());
    assert_ne!(a, b, "actual native GPU ignored submitted {label}");
}

#[test]
#[ignore = "requires GPU; run explicitly for #182 native presentation acceptance"]
fn submitted_frame_mesh_material_transform_and_environment_change_actual_pixels() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let world = WorldScene::new(&mmorpg_scenery::greyhaven_vale_scenery());
        let view = CameraView {
            eye: [0.0, 12.0, 28.0],
            target: [0.0, 2.0, 0.0],
        };
        let frame = world.frame(0.0, true, view.eye);
        let before = offscreen_frame_pixels(&frame, &[], view).await.unwrap();
        save("consumed-before", &before);
        let mut material = frame.clone();
        for draw in &mut material.draws {
            draw.material.color = [1.0, 0.0, 0.0];
            draw.material.unlit = true;
            draw.material.emissive = [0.0; 3];
        }
        let after = offscreen_frame_pixels(&material, &[], view).await.unwrap();
        changed(&before, &after, "material");
        save("consumed-material", &after);
        let mut transform = frame.clone();
        for draw in &mut transform.draws {
            draw.model.elements[12] += 5000.0;
        }
        let after = offscreen_frame_pixels(&transform, &[], view).await.unwrap();
        changed(&before, &after, "model transforms");
        save("consumed-transform", &after);
        let mut geometry = frame.clone();
        for draw in &mut geometry.draws {
            draw.mesh = Arc::new(three_d_core::Mesh::unit_cube());
            draw.resource_key = format!("acceptance-cube:{}", draw.id);
        }
        let after = offscreen_frame_pixels(&geometry, &[], view).await.unwrap();
        changed(&before, &after, "mesh geometry");
        save("consumed-mesh", &after);
        let night = world.frame(600.0, true, view.eye);
        let after = offscreen_frame_pixels(&night, &[], view).await.unwrap();
        changed(&before, &after, "day/night environment");
        save("consumed-night", &after);
        let mut lighting = frame.clone();
        lighting.environment.sun.intensity = 0.0;
        lighting.environment.sky.intensity = 0.0;
        let after = offscreen_frame_pixels(&lighting, &[], view).await.unwrap();
        changed(&before, &after, "sun/sky lighting");
        let mut emissive = frame.clone();
        emissive.environment.sun.intensity = 0.0;
        emissive.environment.sky.intensity = 0.0;
        for draw in &mut emissive.draws {
            draw.material.emissive = [0.0; 3];
        }
        let dark = offscreen_frame_pixels(&emissive, &[], view).await.unwrap();
        for draw in &mut emissive.draws {
            draw.material.emissive = [1.0, 0.7, 0.0];
            draw.material.unlit = false;
        }
        let glowing = offscreen_frame_pixels(&emissive, &[], view).await.unwrap();
        changed(&dark, &glowing, "emissive material");
        let mut vertex_colors = frame.clone();
        for draw in &mut vertex_colors.draws {
            if draw.mesh.attributes().colors.is_some() {
                let mut attributes = draw.mesh.attributes().clone();
                attributes.colors = Some(vec![
                    three_d_core::Color3::new(1.0, 0.0, 1.0);
                    draw.mesh.vertices().len()
                ]);
                draw.mesh = Arc::new(
                    three_d_core::Mesh::with_attributes(
                        draw.mesh.vertices().to_vec(),
                        draw.mesh.indices().to_vec(),
                        attributes,
                    )
                    .unwrap(),
                );
                draw.resource_key = format!("acceptance-colors:{}", draw.id);
            }
        }
        let colored = offscreen_frame_pixels(&vertex_colors, &[], view)
            .await
            .unwrap();
        changed(&before, &colored, "mesh vertex colors");
        let mut fog = frame.clone();
        fog.environment.fog.near = 0.0;
        fog.environment.fog.far = 1.0;
        fog.environment.fog.color = [1.0, 0.0, 1.0];
        let after = offscreen_frame_pixels(&fog, &[], view).await.unwrap();
        changed(&before, &after, "fog");
        let mut opacity = frame.clone();
        for draw in &mut opacity.draws {
            draw.material.opacity = 0.0;
        }
        let after = offscreen_frame_pixels(&opacity, &[], view).await.unwrap();
        changed(&before, &after, "opacity");
    });
}

#[test]
#[ignore = "requires GPU and independent paired-image review; run explicitly for #182"]
fn current_greyhaven_regions_and_six_player_looks_render_on_the_real_native_gpu() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let world = WorldScene::new(&mmorpg_scenery::greyhaven_vale_scenery());
        for (name, eye, target) in [
            ("outpost", [5.0, 8.5, 31.0], [-14.0, 3.0, 4.0]),
            ("woods", [-95.0, 12.0, -20.0], [-85.0, 1.0, -40.0]),
            ("farm", [90.0, 18.0, 20.0], [90.0, 1.0, 0.0]),
            ("lake", [0.0, 15.0, 125.0], [0.0, 0.0, 100.0]),
            ("hollow", [0.0, 15.0, -150.0], [0.0, 2.0, -175.0]),
        ] {
            for (phase, seconds) in [("day", 0.0), ("night", 600.0)] {
                let view = CameraView { eye, target };
                let frame = world.frame(seconds, true, eye);
                let pixels = offscreen_frame_pixels(&frame, &[], view).await.unwrap();
                save(&format!("{name}-{phase}"), &pixels);
            }
        }
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        for kind in mmorpg_scenery::PropKind::ALL {
            let prop = scenery.props.iter().find(|prop| prop.kind == kind).unwrap();
            let x = prop.position[0] as f32 / 100.0;
            let z = prop.position[2] as f32 / 100.0;
            let base = scenery.height_at(prop.position[0], prop.position[2]) as f32 / 100.0;
            let half = prop.half_extents.map(|v| v as f32 / 100.0);
            let distance = half.into_iter().fold(2.0_f32, f32::max) * 3.0 + 4.0;
            let view = CameraView {
                eye: [x + distance, base + half[1] + distance * 0.7, z + distance],
                target: [x, base + half[1], z],
            };
            let frame = world.frame(0.0, false, view.eye);
            let pixels = offscreen_frame_pixels(&frame, &[], view).await.unwrap();
            save(&format!("prop-{}", kind.name()), &pixels);
        }
        let now = Instant::now();
        let mut renders = Vec::new();
        for class in 0..3 {
            for sex in 0..2 {
                let content = greyhaven_vale::content();
                let mut zone =
                    ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
                zone.add_player(1).unwrap();
                zone.apply_command(1, 1, ZoneCommand::ChooseClass { class, sex })
                    .unwrap();
                zone.advance_tick().unwrap();
                let projection = zone.snapshot_for_player(1).unwrap();
                let mut presentation =
                    Presentation::new(1, mmorpg_scenery::greyhaven_vale_scenery(), content, now)
                        .unwrap();
                presentation.push(projection, now).unwrap();
                let view = OrbitCamera::default().view(presentation.camera_target(now));
                let frame = world.frame(0.0, false, view.eye);
                let units = presentation.unit_draws(now, view);
                let pixels = offscreen_frame_pixels(&frame, &units, view).await.unwrap();
                save(&format!("class-{class}-sex-{sex}"), &pixels);
                renders.push(pixels);
            }
        }
        for (i, pixels) in renders.iter().enumerate() {
            for other in &renders[i + 1..] {
                changed(pixels, other, "class/sex appearance");
            }
        }
    });
}

#[path = "world_reference.rs"]
mod reference;

#[test]
#[ignore = "requires GPU and independent paired-image review of all current unit families"]
fn current_creature_and_npc_model_images_reach_native_gpu() {
    use mmorpg_core::{EntityFlags, EntityKind, EntitySnapshot};
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (cases, _) = reference::shared();
        let now = Instant::now();
        let world = WorldScene::new(&mmorpg_scenery::greyhaven_vale_scenery());
        for request in cases {
            let name = request["name"].as_str().unwrap();
            if !(name.starts_with("creature:") || name.starts_with("npc:")) {
                continue;
            }
            let content = greyhaven_vale::content();
            let mut zone =
                ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
            zone.add_player(1).unwrap();
            zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
                .unwrap();
            zone.advance_tick().unwrap();
            let mut projection = zone.snapshot_for_player(1).unwrap();
            projection.entities = request["entities"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entity| EntitySnapshot {
                    kind: match entity["kind"].as_str().unwrap() {
                        "player" => EntityKind::Player,
                        "creature" => EntityKind::Creature,
                        "npc" => EntityKind::Npc,
                        _ => panic!("invalid fixture kind"),
                    },
                    id: entity["entityId"].as_u64().unwrap().try_into().unwrap(),
                    appearance: entity["appearance"].as_u64().unwrap().try_into().unwrap(),
                    position: std::array::from_fn(|i| {
                        entity["position"][i].as_i64().unwrap().try_into().unwrap()
                    }),
                    velocity: [0; 3],
                    facing: 0,
                    level: entity["level"].as_u64().unwrap().try_into().unwrap(),
                    health_percent: entity["healthPercent"]
                        .as_u64()
                        .unwrap()
                        .try_into()
                        .unwrap(),
                    flags: EntityFlags {
                        dead: entity["flags"]["dead"].as_bool().unwrap(),
                        hostile: entity["flags"]["hostile"].as_bool().unwrap(),
                        tapped_by_other: entity["flags"]["tappedByOther"].as_bool().unwrap(),
                        attackable: entity["flags"]["attackable"].as_bool().unwrap(),
                        ..EntityFlags::default()
                    },
                })
                .collect();
            if request["target"]["kind"] == "creature" {
                projection.viewer.target = Some(mmorpg_core::EntityRef::Creature(
                    mmorpg_core::CreatureId::new(
                        request["target"]["id"]
                            .as_u64()
                            .unwrap()
                            .try_into()
                            .unwrap(),
                    ),
                ));
            }
            // Gallery separates the subject from the viewer without changing its model identity.
            projection.entities[1].position[0] += 250;
            let mut presentation =
                Presentation::new(1, mmorpg_scenery::greyhaven_vale_scenery(), content, now)
                    .unwrap();
            presentation.set_reduced_motion(true);
            presentation.push(projection, now).unwrap();
            let view = CameraView {
                eye: [5.0, 3.0, 7.0],
                target: [2.5, 0.9, 0.0],
            };
            let frame = world.frame(0.0, false, view.eye);
            let pixels = offscreen_frame_pixels(&frame, &presentation.unit_draws(now, view), view)
                .await
                .unwrap();
            save(&format!("model-{}", name.replace(':', "-")), &pixels);
        }
    });
}
