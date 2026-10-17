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
