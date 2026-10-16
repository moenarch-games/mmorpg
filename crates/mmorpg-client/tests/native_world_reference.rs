#[path = "support/world_reference.rs"]
mod reference;

#[test]
fn pages_reference_inventory_covers_current_world_scope() {
    let (cases, output) = reference::shared();
    assert_eq!(output["scenes"].as_array().unwrap().len(), cases.len());
    assert!(!output["resources"].as_object().unwrap().is_empty());
    for kind in mmorpg_scenery::PropKind::ALL {
        let name = format!("prop:{}", kind.name());
        let scene = output["scenes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|scene| scene["name"] == name)
            .unwrap();
        let empty_name = format!("empty:{}", kind.name());
        let empty = output["scenes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|scene| scene["name"] == empty_name)
            .unwrap();
        assert_ne!(
            scene["draws"], empty["draws"],
            "prop has no contribution: {name}"
        );
        assert!(
            scene["draws"].as_array().unwrap().iter().any(|draw| {
                let id = draw["id"].as_str().unwrap();
                !id.starts_with("static-all") && !id.starts_with("water-")
            }),
            "no reference geometry for {name}"
        );
    }
}
