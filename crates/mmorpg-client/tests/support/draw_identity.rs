use std::collections::BTreeMap;

/// Cache namespaces differ between the WASM export and native scenery value.
/// Keep the complete draw kind/chunk/cull/geometry ordinal/instance ordinal.
pub fn canonical_draw_id(id: &str) -> String {
    let Some(rest) = id.strip_prefix("instances:") else {
        return id.to_owned();
    };
    let Some((chunk, rest)) = rest.split_once(':') else {
        return id.to_owned();
    };
    let Some((cull, rest)) = rest.split_once(':') else {
        return id.to_owned();
    };
    let Some((size, coordinates)) = chunk.split_once('m') else {
        return id.to_owned();
    };
    let Some((x, z)) = coordinates.split_once('_') else {
        return id.to_owned();
    };
    let integer = |value: &str| {
        !value.is_empty()
            && value
                .strip_prefix('-')
                .unwrap_or(value)
                .bytes()
                .all(|c| c.is_ascii_digit())
            && value != "-"
    };
    if size.is_empty()
        || !size.bytes().all(|c| c.is_ascii_digit())
        || !integer(x)
        || !integer(z)
        || !matches!(cull, "near" | "mid" | "far" | "always")
    {
        return id.to_owned();
    }
    let (envelope, native) = if let Some(value) = rest.strip_prefix("scenery:") {
        (value, false)
    } else if let Some(value) = rest.strip_prefix("native-scenery:") {
        (value, true)
    } else {
        return id.to_owned();
    };
    let Some((identity, ordinals)) = envelope.rsplit_once(":instance-geometry:") else {
        return id.to_owned();
    };
    let identity: Vec<_> = identity.split(':').collect();
    let valid_identity = if native {
        identity.len() == 2
    } else {
        identity.len() >= 3
    };
    let valid_hash = identity
        .last()
        .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|c| c.is_ascii_hexdigit()));
    let Some((geometry, instance)) = ordinals.split_once(':') else {
        return id.to_owned();
    };
    let ordinal = |value: &str| !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit());
    if !valid_identity
        || !valid_hash
        || identity.iter().any(|part| part.is_empty())
        || !ordinal(geometry)
        || !ordinal(instance)
    {
        return id.to_owned();
    }
    format!("instances:{chunk}:{cull}:instance-geometry:{geometry}:{instance}")
}

pub fn unique_index<'a, T>(
    values: impl IntoIterator<Item = (&'a str, T)>,
    label: &str,
) -> BTreeMap<String, (&'a str, T)> {
    let mut indexed = BTreeMap::new();
    for (id, value) in values {
        let canonical = canonical_draw_id(id);
        assert!(
            !indexed.contains_key(&canonical),
            "duplicate {label} canonical draw ID {canonical} from {id}"
        );
        indexed.insert(canonical, (id, value));
    }
    indexed
}
