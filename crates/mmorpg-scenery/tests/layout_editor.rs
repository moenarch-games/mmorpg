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
        {"id": 103, "position": [-2450, 3600]},
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
    for id in [103, 60000] {
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
    // A decorative move cannot change gameplay content identity.
    let collision_identity = content.revision();
    document["decoration"][0]["position"] = json!([-1900, 1000]);
    let decorative_edit = document.to_string();
    assert_eq!(
        collision_identity,
        greyhaven_vale::content_with_definition(layout::compile(&decorative_edit).unwrap())
            .unwrap()
            .revision()
    );
    assert_ne!(scenery, compose(&decorative_edit, &definition).unwrap());
}

#[test]
fn invalid_overrides_fail_closed_before_publication() {
    for edit in [
        json!([{"id": 103, "position": [0, 2000]}]), // spawn plaza / road
        json!([{"id": 103, "position": [-1000, 3800]}]), // another house
        json!([{"id": 103, "position": [i32::MAX, 3800]}]),
        json!([{"id": 149, "position": [1000, 1000], "removed": true}]), // inn door locked
        json!([{"id": 103, "position": [-2400, 3600]}, {"id": 103, "position": [-2400, 3600]}]),
    ] {
        let mut document = source();
        document["collision"] = edit;
        assert!(layout::compile(&document.to_string()).is_err());
    }
    let mut document = source();
    document["generator"] = json!("greyhaven-unknown");
    assert!(layout::compile(&document.to_string()).is_err());
    let mut document = source();
    document["decoration"][0]["position"] = json!([0, 2000]);
    let definition = layout::compile(&document.to_string()).unwrap();
    assert!(compose(&document.to_string(), &definition).is_err());
}
