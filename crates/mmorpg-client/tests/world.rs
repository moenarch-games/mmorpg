//! Static world geometry: terrain and water meshes plus prop blockouts that
//! reuse the server's collision boxes exactly.
use mmorpg_client::world::{TERRAIN_STEP_UNITS, WorldScene};
use mmorpg_core::greyhaven_vale_definition;
use mmorpg_scenery::{PropKind, TERRAIN_BOUNDS, greyhaven_vale_scenery};

#[test]
fn terrain_and_water_are_valid_indexed_meshes() {
    let scenery = greyhaven_vale_scenery();
    let world = WorldScene::new(&scenery);
    let side = |axis: usize| {
        usize::try_from(
            (TERRAIN_BOUNDS.max[axis] - TERRAIN_BOUNDS.min[axis]) / TERRAIN_STEP_UNITS + 1,
        )
        .unwrap()
    };
    assert_eq!(world.terrain.vertices.len(), side(0) * side(1));
    assert_eq!(
        world.terrain.indices.len(),
        (side(0) - 1) * (side(1) - 1) * 6
    );
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
    let origin =
        |axis: usize| usize::try_from(-TERRAIN_BOUNDS.min[axis] / TERRAIN_STEP_UNITS).unwrap();
    let centre = &world.terrain.vertices[side(0) * origin(1) + origin(0)];
    assert_eq!(centre.position[0], 0.0);
    assert_eq!(centre.position[2], 0.0);
    assert_eq!(centre.position[1], scenery.height_at(0, 0) as f32 / 100.0);
    // Every water body (Stillwater Lake, the wetlands' lake and pools) draws
    // one fan at its own surface, floating above its flat bed.
    assert!(scenery.water.len() > 1);
    assert_eq!(world.water.vertices.len() % scenery.water.len(), 0);
    let per_body = world.water.vertices.len() / scenery.water.len();
    for (water, fan) in scenery
        .water
        .iter()
        .zip(world.water.vertices.chunks(per_body))
    {
        assert_eq!(fan[0].position[0], water.centre[0] as f32 / 100.0);
        assert_eq!(fan[0].position[2], water.centre[1] as f32 / 100.0);
        assert!(
            fan.iter()
                .all(|vertex| vertex.position[1] == water.surface as f32 / 100.0)
        );
    }
}

#[test]
fn every_structure_renders_its_exact_collision_box() {
    let definition = greyhaven_vale_definition();
    let scenery = greyhaven_vale_scenery();
    let world = WorldScene::new(&scenery);
    assert!(world.props.len() >= scenery.props.len());
    // The Sunken Shrine is drawn from its asset-tooling kit (#431), not as boxes, and solid
    // dressing (#577) as its own lamps, barrels and rocks inside its collider.
    for prop in scenery.props.iter().filter(|prop| {
        prop.collider
            .is_some_and(|id| !mmorpg_scenery::is_dressing(id))
            && prop.kind != PropKind::ShrineKit
    }) {
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
