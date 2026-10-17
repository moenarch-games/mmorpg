#[path = "support/world_reference.rs"]
mod reference;

#[test]
fn pages_reference_inventory_covers_current_world_scope() {
    let (cases, output) = reference::shared();
    assert_eq!(output["scenes"].as_array().unwrap().len(), cases.len());
    assert!(!output["resources"].as_object().unwrap().is_empty());
    // Every prop kind's own geometry is checked in mmorpg-presentation's tests;
    // the reference covers the regions and day phases the browser presents.
    for name in ["region:outpost", "region:woods", "day:600", "frozen:600"] {
        assert!(
            output["scenes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|scene| scene["name"] == name),
            "missing {name}"
        );
    }
}

#[test]
fn reference_uses_shared_wasm_relief_at_every_entity_position() {
    let (cases, output) = reference::shared();
    let mut nonflat = false;
    for case in cases {
        if let Some(entities) = case["entities"].as_array() {
            for entity in entities {
                let x: i32 = entity["position"][0].as_i64().unwrap().try_into().unwrap();
                let z: i32 = entity["position"][2].as_i64().unwrap().try_into().unwrap();
                let expected = mmorpg_wasm::scenery::relief_at(x, z);
                assert_eq!(output["reliefSamples"][format!("{x},{z}")], expected);
                nonflat |= expected != 0;
            }
        }
    }
    assert!(
        nonflat,
        "oracle must exercise actual nonflat shared relief, not assume zero"
    );
}
