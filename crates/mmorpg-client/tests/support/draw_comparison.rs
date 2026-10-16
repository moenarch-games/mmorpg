use mmorpg_client::world::SceneDraw;
use serde_json::Value;
pub fn close(actual: f32, expected: f64, label: &str) {
    // TRS composition and f64→f32 conversion, rather than a visual/performance budget.
    let tolerance = f32::EPSILON as f64 * 32.0 * expected.abs().max(1.0);
    assert!(
        (actual as f64 - expected).abs() <= tolerance,
        "{label}: {actual} != {expected}"
    );
}
pub fn vector(actual: &[f32], expected: &Value, label: &str) {
    let values = expected.as_array().unwrap();
    assert_eq!(actual.len(), values.len(), "{label}");
    for (a, b) in actual.iter().zip(values) {
        close(*a, b.as_f64().unwrap(), label);
    }
}
fn digest(mesh: &three_d_core::Mesh) -> String {
    let mut hash = 14695981039346656037_u64;
    let mut bytes = |values: &[u8]| {
        for byte in values {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(1099511628211);
        }
    };
    let positions: Vec<[f32; 3]> = mesh.vertices().iter().map(|v| [v.x, v.y, v.z]).collect();
    let normals = mesh
        .attributes()
        .normals
        .as_ref()
        .map(|values| values.iter().map(|v| [v.x, v.y, v.z]).collect::<Vec<_>>());
    let colors = mesh
        .attributes()
        .colors
        .as_ref()
        .map(|values| values.iter().map(|v| [v.r, v.g, v.b]).collect::<Vec<_>>());
    for (name, values) in [
        ("positions", Some(positions.as_slice())),
        ("normals", normals.as_deref()),
        ("colors", colors.as_deref()),
    ] {
        bytes(name.as_bytes());
        bytes(&[u8::from(values.is_some())]);
        if let Some(values) = values {
            bytes(&u32::try_from(values.len()).unwrap().to_le_bytes());
            for v in values {
                for n in v {
                    bytes(&n.to_le_bytes());
                }
            }
        }
    }
    bytes(&u32::try_from(mesh.indices().len()).unwrap().to_le_bytes());
    for index in mesh.indices() {
        bytes(&index.to_le_bytes());
    }
    format!("{hash:016x}")
}
pub fn compare_draws(actual: &[SceneDraw], expected: &Value, resources: &Value) {
    let by_id =
        super::identity::unique_index(actual.iter().map(|draw| (draw.id.as_str(), draw)), "actual");
    let expected_by_id = super::identity::unique_index(
        expected
            .as_array()
            .unwrap()
            .iter()
            .map(|draw| (draw["id"].as_str().unwrap(), draw)),
        "expected",
    );
    for (canonical, (id, draw)) in expected_by_id {
        let actual = by_id
            .get(&canonical)
            .unwrap_or_else(|| panic!("missing actual submitted draw {id} (canonical {canonical})"))
            .1;
        vector(&actual.model.elements, &draw["model"], id);
        let material = &draw["material"];
        vector(&actual.material.color, &material["color"], id);
        vector(&actual.material.emissive, &material["emissive"], id);
        close(
            actual.material.opacity,
            material["opacity"].as_f64().unwrap(),
            id,
        );
        assert_eq!(
            actual.material.unlit,
            material["unlit"].as_bool().unwrap(),
            "{id}"
        );
        assert_eq!(
            actual.material.double_sided,
            material["double_sided"].as_bool().unwrap(),
            "{id}"
        );
        let resource = &resources[draw["reference_resource"].as_str().unwrap()];
        assert!(!actual.mesh.indices().is_empty(), "{id}: empty geometry");
        for axis in 0..3 {
            let values = actual.mesh.vertices().iter().map(|v| [v.x, v.y, v.z][axis]);
            let (low, high) = values.fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), v| {
                (low.min(v), high.max(v))
            });
            vector(&[low, high], &resource["bounds"][axis], id);
        }
        // Asset topology is an authored contract; procedural tessellation may legitimately differ.
        if draw["source_key"]
            .as_str()
            .is_some_and(|key| key.starts_with("asset-tooling:"))
        {
            assert_eq!(
                digest(&actual.mesh),
                resource["digest"].as_str().unwrap(),
                "normalized asset {id}"
            );
            assert!(
                actual
                    .resource_key
                    .contains(draw["source_key"].as_str().unwrap()),
                "asset identity lost: {id}"
            );
        }
    }
}
