//! Static world geometry: terrain and water meshes plus prop blockouts that
//! reuse the server's collision boxes exactly.
use mmorpg_client::world::{TERRAIN_STEP_UNITS, WorldScene};
use mmorpg_core::greyhaven_vale_definition;
use mmorpg_scenery::{TERRAIN_EXTENT_UNITS, greyhaven_vale_scenery};

#[test]
fn terrain_and_water_are_valid_indexed_meshes() {
    let scenery = greyhaven_vale_scenery();
    let world = WorldScene::new(&scenery);
    let side = usize::try_from(2 * TERRAIN_EXTENT_UNITS / TERRAIN_STEP_UNITS + 1).unwrap();
    assert_eq!(world.terrain.vertices.len(), side * side);
    assert_eq!(world.terrain.indices.len(), (side - 1) * (side - 1) * 6);
    for mesh in [&world.terrain, &world.water] {
        assert!(!mesh.indices.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);
        let count = u32::try_from(mesh.vertices.len()).unwrap();
        assert!(mesh.indices.iter().all(|&index| index < count));
        for vertex in &mesh.vertices {
            let length = vertex.normal.iter().map(|axis| axis * axis).sum::<f32>();
            assert!((length - 1.0).abs() < 1e-4);
            assert!(
                vertex
                    .color
                    .iter()
                    .all(|channel| (0.0..=1.0).contains(channel))
            );
        }
    }
    // Terrain vertices carry the shared relief, in metres.
    let centre = &world.terrain.vertices[side * (side / 2) + side / 2];
    assert_eq!(centre.position[0], 0.0);
    assert_eq!(centre.position[2], 0.0);
    assert_eq!(centre.position[1], scenery.height_at(0, 0) as f32 / 100.0);
    // The lake surface floats above its flat bed.
    let lake = scenery.water[0];
    assert!(
        world
            .water
            .vertices
            .iter()
            .all(|vertex| vertex.position[1] == lake.surface as f32 / 100.0)
    );
}

#[test]
fn every_structure_renders_its_exact_collision_box() {
    let definition = greyhaven_vale_definition();
    let scenery = greyhaven_vale_scenery();
    let world = WorldScene::new(&scenery);
    assert!(world.props.len() >= scenery.props.len());
    for prop in scenery.props.iter().filter(|prop| prop.collider.is_some()) {
        // The inn's footprint is a presentation anchor without a collider (#158).
        let Some(collider) = definition
            .colliders()
            .iter()
            .find(|collider| Some(collider.id) == prop.collider)
        else {
            continue;
        };
        let position = collider.position.map(|value| value as f32 / 100.0);
        let size = collider.half_extents.map(|value| value as f32 / 50.0);
        assert!(
            world
                .props
                .iter()
                // Hillside structures (#159) sum their footing and half height in f32.
                .any(|item| item
                    .position
                    .iter()
                    .zip(position)
                    .all(|(a, b)| (a - b).abs() < 1.0e-4)
                    && item.size == size
                    && item.yaw == 0.0),
            "collider {} has no exact box",
            collider.id
        );
    }
}
