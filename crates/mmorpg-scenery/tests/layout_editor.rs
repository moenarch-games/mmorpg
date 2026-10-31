use mmorpg_core::greyhaven_vale::{self, layout};
use mmorpg_scenery::layout::compose;
use serde_json::{Value, json};

fn source() -> Value {
    serde_json::from_str(layout::SOURCE).unwrap()
}

#[test]
fn saved_overrides_reopen_with_matching_authoritative_and_visual_placements() {
    let mut document = source();
    // Move an existing house, suppress another, add a house from the same prefab.
    document["collision"] = json!([
        {"id": 106, "position": [2350, 3600]},
        {"id": 104, "position": [-1100, 3800], "removed": true},
        {"id": 60000, "position": [-2300, 2800]}
    ]);
    document["decoration"][0]["position"] = json!([-1850, 1000]);
    let saved = serde_json::to_string_pretty(&document).unwrap();
    let definition = layout::compile(&saved).unwrap();
    let content = greyhaven_vale::content_with_definition(definition.clone()).unwrap();
    let scenery = compose(&saved, &definition).unwrap();
    assert_eq!(definition, layout::compile(&saved).unwrap());
    assert_eq!(scenery, compose(&saved, &definition).unwrap());
    assert_eq!(
        content.revision(),
        greyhaven_vale::content_with_definition(layout::compile(&saved).unwrap())
            .unwrap()
            .revision()
    );
    assert!(
        !definition
            .colliders()
            .iter()
            .any(|collider| collider.id == 104)
    );
    assert!(!scenery.props.iter().any(|prop| prop.collider == Some(104)));
    for id in [106, 60000] {
        let collider = definition
            .colliders()
            .iter()
            .find(|collider| collider.id == id)
            .unwrap();
        let prop = scenery
            .props
            .iter()
            .find(|prop| prop.collider == Some(id))
            .unwrap();
        assert_eq!(
            [prop.position[0], prop.position[2]],
            [collider.position[0], collider.position[2]]
        );
        assert_eq!(prop.half_extents, collider.half_extents);
    }
    // The market square follows house 106 (#577).
    let market = |definition: &mmorpg_core::ZoneDefinition| -> Vec<[i32; 3]> {
        definition
            .colliders()
            .iter()
            .filter(|collider| greyhaven_vale::ids::MARKET.contains(&collider.id))
            .map(|collider| collider.position)
            .collect()
    };
    let base = market(&greyhaven_vale::base_definition());
    let moved = market(&definition);
    assert_eq!(base.len(), moved.len());
    for (before, after) in base.iter().zip(&moved) {
        assert_eq!([after[0] - before[0], after[2] - before[2]], [-50, 0]);
    }
    // A lamp collides where the layout stands it (#577), so moving one changes gameplay
    // content identity; moving flowers would not.
    let collision_identity = content.revision();
    document["decoration"][0]["position"] = json!([-1900, 1000]);
    let lamp_edit = document.to_string();
    let edited = layout::compile(&lamp_edit).unwrap();
    assert_ne!(
        collision_identity,
        greyhaven_vale::content_with_definition(edited.clone())
            .unwrap()
            .revision()
    );
    let lamp = edited
        .colliders()
        .iter()
        .find(|collider| collider.id == *greyhaven_vale::ids::LAYOUT_LAMPS.start())
        .unwrap();
    assert_eq!([lamp.position[0], lamp.position[2]], [-1900, 1000]);
    let scenery = compose(&lamp_edit, &edited).unwrap();
    let drawn = scenery
        .props
        .iter()
        .find(|prop| prop.collider == Some(lamp.id))
        .unwrap();
    assert_eq!(drawn.kind, mmorpg_scenery::PropKind::Lamp);
    assert_eq!([drawn.position[0], drawn.position[2]], [-1900, 1000]);
    let mut flowers = source();
    flowers["decoration"].as_array_mut().unwrap().push(
        json!({"id": "outpost-flowers-1", "kind": "flowers", "position": [-2600, 1200], "yaw": 0}),
    );
    let flowers = flowers.to_string();
    let base_identity = greyhaven_vale::content().revision();
    let definition = layout::compile(&flowers).unwrap();
    assert_eq!(
        base_identity,
        greyhaven_vale::content_with_definition(definition.clone())
            .unwrap()
            .revision()
    );
    assert!(
        compose(&flowers, &definition)
            .unwrap()
            .props
            .iter()
            .any(|prop| prop.kind == mmorpg_scenery::PropKind::Flowers
                && [prop.position[0], prop.position[2]] == [-2600, 1200])
    );
}

#[test]
fn invalid_overrides_fail_closed_before_publication() {
    for edit in [
        json!([{"id": 106, "position": [0, 2000]}]), // spawn plaza / road
        json!([{"id": 106, "position": [1200, 3800]}]), // another house
        json!([{"id": 106, "position": [i32::MAX, 3800]}]),
        json!([{"id": 149, "position": [1000, 1000], "removed": true}]), // inn door locked
        json!([{"id": 106, "position": [2400, 3600]}, {"id": 106, "position": [2400, 3600]}]),
        json!([{"id": 103, "position": [-2450, 3600]}]), // Elden's walk-in house (#158) is locked
    ] {
        let mut document = source();
        document["collision"] = edit;
        assert!(layout::compile(&document.to_string()).is_err());
    }
    let mut document = source();
    document["generator"] = json!("greyhaven-unknown");
    assert!(layout::compile(&document.to_string()).is_err());
    // A decoration on the spawn plaza, or one of an unknown prefab, fails to compile.
    let mut document = source();
    document["decoration"][0]["position"] = json!([0, 2000]);
    assert!(layout::compile(&document.to_string()).is_err());
    let mut document = source();
    document["decoration"][0]["kind"] = json!("statue");
    assert!(layout::compile(&document.to_string()).is_err());
}
