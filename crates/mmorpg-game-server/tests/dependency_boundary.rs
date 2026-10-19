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
    // Parsing sanity: scenery reads its authored JSON through Serde; gameplay still
    // points downward to core/physics, and hosts must never reach scenery.
    assert_eq!(
        graph.get(PRESENTATION),
        Some(&vec!["mmorpg-core", "serde", "serde_json"])
    );
    assert!(reachable(&graph, PRESENTATION).contains("rapier3d"));
    let host = reachable(&graph, "mmorpg-game-server");
    assert!(host.contains("game-server") && host.contains("rapier3d"));
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
    }
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
