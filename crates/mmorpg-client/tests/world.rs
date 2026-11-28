//! Static world prop blockouts that reuse the server's collision boxes exactly.
use mmorpg_client::world::WorldScene;
use mmorpg_core::greyhaven_vale_definition;
use mmorpg_presentation::geometry::position_f32;
use mmorpg_scenery::{PropKind, greyhaven_vale_scenery};

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
        let position = position_f32(collider.position);
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
