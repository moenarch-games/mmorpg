//! Hosts must not depend on presentation data. The committed lockfile is the
//! resolved workspace graph, so reachability over it is a mechanical check
//! that no host-side crate links `mmorpg-scenery`, directly or transitively.
use std::collections::{BTreeMap, BTreeSet};

const LOCKFILE: &str = include_str!("../../../Cargo.lock");
const PRESENTATION: &str = "mmorpg-scenery";

/// Package name → names of its dependencies, from `[[package]]` entries.
fn dependency_graph(lockfile: &'static str) -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut graph = BTreeMap::new();
    for package in lockfile.split("[[package]]").skip(1) {
        let Some(name) = package
            .lines()
            .find_map(|line| line.strip_prefix("name = \""))
            .and_then(|rest| rest.strip_suffix('"'))
        else {
            continue;
        };
        let dependencies = package
            .split_once("dependencies = [")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(list, _)| {
                list.lines()
                    .filter_map(|line| line.trim().strip_prefix('"'))
                    .filter_map(|entry| entry.split(['"', ' ']).next())
                    .collect()
            })
            .unwrap_or_default();
        graph.insert(name, dependencies);
    }
    graph
}

fn reachable(
    graph: &BTreeMap<&'static str, Vec<&'static str>>,
    root: &str,
) -> BTreeSet<&'static str> {
    let mut seen = BTreeSet::new();
    let mut pending: Vec<&str> = graph.get(root).cloned().unwrap_or_default();
    while let Some(name) = pending.pop() {
        if seen.insert(name) {
            pending.extend(graph.get(name).into_iter().flatten());
        }
    }
    seen
}

#[test]
fn hosts_never_link_presentation_scenery() {
    let graph = dependency_graph(LOCKFILE);
    // Parsing sanity: scenery reads its authored JSON through Serde and lists its kinds
    // with strum (#542); gameplay still points downward to core/physics, and hosts must
    // never reach scenery.
    assert_eq!(
        graph.get(PRESENTATION),
        Some(&vec!["mmorpg-core", "serde", "serde_json", "strum"])
    );
    assert!(reachable(&graph, PRESENTATION).contains("physics-engine"));
    let host = reachable(&graph, "mmorpg-game-server");
    assert!(host.contains("game-server") && host.contains("physics-engine"));
    // `mmorpg-wasm` is absent on purpose: it is also the browser's
    // presentation adapter and exports scenery, while its local zone host
    // never reads it (checked in mmorpg-wasm/src/scenery.rs).
    for host_side in [
        "mmorpg-game-server",
        "mmorpg-core",
        "mmorpg-protocol",
        "mmorpg-control-plane",
        "mmorpg-scenarios",
    ] {
        assert!(
            graph.contains_key(host_side),
            "{host_side} is a workspace crate"
        );
        assert!(
            !reachable(&graph, host_side).contains(PRESENTATION),
            "{host_side} must not depend on {PRESENTATION}"
        );
        assert!(
            !reachable(&graph, host_side).contains(CLIENT_MODEL),
            "{host_side} must not depend on {CLIENT_MODEL}"
        );
    }
}

const CLIENT_MODEL: &str = "mmorpg-client-model";

/// The shared client rules (#543) are pure: core gameplay types in, views and
/// commands out, with no I/O, rendering or presentation data. `serde_json` reads
/// the committed world-map export from a string. `mmorpg-protocol` is its
/// test-only wire codec (the lockfile lists dev-dependencies too).
#[test]
fn the_client_model_holds_only_client_rules_over_core() {
    let graph = dependency_graph(LOCKFILE);
    assert_eq!(
        graph.get(CLIENT_MODEL),
        Some(&vec![
            "mmorpg-core",
            "mmorpg-protocol",
            "serde_json",
            "strum"
        ])
    );
    const MANIFEST: &str = include_str!("../../mmorpg-client-model/Cargo.toml");
    let (normal, dev) = MANIFEST
        .split_once("[dev-dependencies]")
        .expect("the client model declares its test-only codec");
    assert!(!normal.contains("mmorpg-protocol") && dev.contains("mmorpg-protocol"));
    let reached = reachable(&graph, CLIENT_MODEL);
    for forbidden in [PRESENTATION, "mmorpg-presentation", "game-server", "tokio"] {
        assert!(
            !reached.contains(forbidden),
            "{CLIENT_MODEL} must not depend on {forbidden}"
        );
    }
}

/// Physics Engine is a normal dependency of core, shared by native/WASM production;
/// Rapier is optional and enabled only by the independent A/B diagnostic feature.
#[test]
fn production_builds_use_the_foundation_owned_physics_backend() {
    const FOUNDATION: &str = include_str!("../../sim-foundation/Cargo.toml");
    let declaration = FOUNDATION
        .lines()
        .find(|line| line.starts_with("physics-engine"))
        .expect("sim-foundation declares physics-engine");
    assert_eq!(declaration, "physics-engine.workspace = true");
    assert!(FOUNDATION.contains("rapier3d = { workspace = true, optional = true }"));
    assert!(FOUNDATION.contains("physics-ab = [\"dep:rapier3d\", \"dep:web-time\"]"));
    assert!(
        !FOUNDATION.contains("default = ["),
        "physics engine must not need an opt-in feature"
    );
    for (name, manifest) in [
        ("mmorpg-game-server", include_str!("../Cargo.toml")),
        ("mmorpg-wasm", include_str!("../../mmorpg-wasm/Cargo.toml")),
        (
            "mmorpg-protocol",
            include_str!("../../mmorpg-protocol/Cargo.toml"),
        ),
        (
            "mmorpg-scenarios",
            include_str!("../../mmorpg-scenarios/Cargo.toml"),
        ),
        (
            "mmorpg-client",
            include_str!("../../mmorpg-client/Cargo.toml"),
        ),
        (
            "mmorpg-scenery",
            include_str!("../../mmorpg-scenery/Cargo.toml"),
        ),
        (
            "mmorpg-presentation",
            include_str!("../../mmorpg-presentation/Cargo.toml"),
        ),
        (
            "mmorpg-client-model",
            include_str!("../../mmorpg-client-model/Cargo.toml"),
        ),
        (
            "mmorpg-control-plane",
            include_str!("../../mmorpg-control-plane/Cargo.toml"),
        ),
        (
            "mmorpg-characters",
            include_str!("../../mmorpg-characters/Cargo.toml"),
        ),
    ] {
        assert!(
            !manifest.contains("physics-ab") && !manifest.contains("physics-engine"),
            "{name} must leave backend selection to the foundation"
        );
    }
    let graph = dependency_graph(LOCKFILE);
    let users: Vec<_> = graph
        .iter()
        .filter(|(_, dependencies)| dependencies.contains(&"physics-engine"))
        .map(|(name, _)| *name)
        .collect();
    // Core links it only for the authored-backend A/B test.
    assert_eq!(users, ["mmorpg-core", "sim-foundation"]);
}

#[test]
fn a_transitive_presentation_dependency_is_detected() {
    let graph = dependency_graph(
        r#"
[[package]]
name = "mmorpg-game-server"
version = "0.1.0"
dependencies = [
 "mmorpg-protocol",
 "tokio 1.53.1",
]

[[package]]
name = "mmorpg-protocol"
version = "0.1.0"
dependencies = [
 "mmorpg-scenery",
]

[[package]]
name = "mmorpg-scenery"
version = "0.1.0"
"#,
    );
    let host = reachable(&graph, "mmorpg-game-server");
    assert!(host.contains(PRESENTATION) && host.contains("tokio"));
}
