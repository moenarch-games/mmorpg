//! The documented crate boundaries (AGENTS.md's Layout and Authority), asserted over the
//! resolved workspace graph that `cargo metadata --locked` reports: every edge carries its kind,
//! so a test-only (dev) edge is told apart from one a build links.
//!
//! - Network hosts never link presentation (`mmorpg-scenery`, `mmorpg-presentation`), the
//!   client rules (`mmorpg-client-model`) or the browser's local host.
//! - The browser's local zone host (`mmorpg-local-host`) never links presentation or the client
//!   rules, nor the network host's runtime; `mmorpg-wasm` adds presentation beside it.
//! - The client rules hold only rules over core.
//! - `mmorpg-client` links `mmorpg-game-server` for Host World, which runs a zone host in the
//!   client's process; no other client crate links it.
//! - World Studio links no game crate and no crate links it (it runs `mmorpg-scenery`'s
//!   `export-world-layers` example as a separate program; docs/WORLD_STUDIO.md).
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;

const SCENERY: &str = "mmorpg-scenery";
const PRESENTATION: &str = "mmorpg-presentation";
const CLIENT_MODEL: &str = "mmorpg-client-model";
const LOCAL_HOST: &str = "mmorpg-local-host";
const NETWORK_HOST: &str = "mmorpg-game-server";
const WORLD_STUDIO: &str = "mmorpg-world-studio";

/// Crates a network zone host builds from, and the headless tooling that runs them.
const HOST_SIDE: [&str; 9] = [
    "sim-foundation",
    "sim-movement",
    "sim-combat",
    "mmorpg-core",
    "mmorpg-protocol",
    "mmorpg-control-plane",
    "mmorpg-characters",
    NETWORK_HOST,
    "mmorpg-scenarios",
];

/// How a dependency edge is used.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Kind {
    Normal,
    Build,
    Dev,
}

/// Package name → its dependencies by package name, each with the kinds of its edge. Package
/// names are unique among workspace crates; an external crate resolved at several versions
/// merges its versions' edges, which only widens reachability.
type Graph = BTreeMap<String, BTreeMap<String, BTreeSet<Kind>>>;

fn metadata_graph(metadata: &Value) -> Graph {
    let packages = metadata["packages"].as_array().expect("packages");
    let names: BTreeMap<&str, &str> = packages
        .iter()
        .map(|package| {
            (
                package["id"].as_str().expect("package id"),
                package["name"].as_str().expect("package name"),
            )
        })
        .collect();
    let mut graph = Graph::new();
    for node in metadata["resolve"]["nodes"].as_array().expect("nodes") {
        let name = names[node["id"].as_str().expect("node id")];
        let edges = graph.entry(name.to_owned()).or_default();
        for dependency in node["deps"].as_array().expect("deps") {
            let kinds = dependency["dep_kinds"]
                .as_array()
                .expect("dep kinds")
                .iter()
                .map(|kind| match kind["kind"].as_str() {
                    None => Kind::Normal,
                    Some("build") => Kind::Build,
                    Some("dev") => Kind::Dev,
                    Some(other) => panic!("unknown dependency kind {other}"),
                });
            edges
                .entry(names[dependency["pkg"].as_str().expect("dep id")].to_owned())
                .or_default()
                .extend(kinds);
        }
    }
    graph
}

/// The workspace's resolved graph, read once per test binary.
fn workspace() -> &'static Graph {
    static GRAPH: OnceLock<Graph> = OnceLock::new();
    GRAPH.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args(["metadata", "--format-version", "1", "--locked", "--offline"])
            .arg("--manifest-path")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"))
            .output()
            .expect("cargo metadata runs");
        assert!(
            output.status.success(),
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        metadata_graph(&serde_json::from_slice(&output.stdout).expect("metadata is JSON"))
    })
}

/// Every crate a build of `root` links: its normal and build edges, transitively. Test-only
/// edges are never followed.
fn linked(graph: &Graph, root: &str) -> BTreeSet<String> {
    assert!(graph.contains_key(root), "{root} is in the graph");
    let mut seen = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(name) = pending.pop() {
        for (dependency, kinds) in graph.get(&name).into_iter().flatten() {
            if kinds.iter().any(|kind| *kind != Kind::Dev) && seen.insert(dependency.clone()) {
                pending.push(dependency.clone());
            }
        }
    }
    seen
}

/// The workspace crates `name` depends on directly, with the kinds of each edge.
fn workspace_edges(graph: &Graph, name: &str) -> BTreeMap<String, BTreeSet<Kind>> {
    graph[name]
        .iter()
        .filter(|(dependency, _)| is_workspace_crate(dependency))
        .map(|(dependency, kinds)| (dependency.clone(), kinds.clone()))
        .collect()
}

fn is_workspace_crate(name: &str) -> bool {
    name.starts_with("mmorpg-") || name.starts_with("sim-")
}

fn assert_never_links(graph: &Graph, crates: &[&str], forbidden: &[&str]) {
    for name in crates {
        let reached = linked(graph, name);
        for crate_name in forbidden {
            assert!(
                !reached.contains(*crate_name),
                "{name} must not link {crate_name}"
            );
        }
    }
}

#[test]
fn network_hosts_never_link_presentation_client_rules_or_the_local_host() {
    let graph = workspace();
    // Sanity: scenery is reachable from its clients and itself reaches core's physics.
    assert!(linked(graph, SCENERY).contains("physics-engine"));
    assert!(linked(graph, "mmorpg-client").contains(SCENERY));
    let host = linked(graph, NETWORK_HOST);
    assert!(host.contains("game-server") && host.contains("physics-engine"));
    assert_never_links(
        graph,
        &HOST_SIDE,
        &[
            SCENERY,
            PRESENTATION,
            CLIENT_MODEL,
            LOCAL_HOST,
            "mmorpg-wasm",
        ],
    );
}

/// The local zone host is gameplay: scenery and the rest of presentation never feed back into
/// it. It also stays WASM-compatible, so the network host's runtime is only a parity-test edge.
#[test]
fn the_local_zone_host_never_links_presentation_or_the_network_runtime() {
    let graph = workspace();
    assert_never_links(
        graph,
        &[LOCAL_HOST],
        &[
            SCENERY,
            PRESENTATION,
            CLIENT_MODEL,
            NETWORK_HOST,
            "game-server",
            "tokio",
            "wasm-bindgen",
        ],
    );
    // mmorpg-wasm is where presentation joins it, and links no network runtime either.
    let page = linked(graph, "mmorpg-wasm");
    assert!(page.contains(LOCAL_HOST) && page.contains(SCENERY));
    assert_never_links(
        graph,
        &["mmorpg-wasm"],
        &[NETWORK_HOST, "game-server", "tokio"],
    );
    for name in [LOCAL_HOST, "mmorpg-wasm"] {
        assert_eq!(
            graph[name][NETWORK_HOST],
            BTreeSet::from([Kind::Dev]),
            "{name} reads the network host only as parity evidence"
        );
    }
}

/// The shared client rules (#543) are pure: core gameplay types in, views and commands out,
/// with no I/O, rendering or presentation data. `serde_json` reads the committed world-map
/// export from a string, and `serde` derives the parse of the committed interface theme and
/// logo files. `mmorpg-protocol` is its test-only wire codec.
#[test]
fn the_client_model_holds_only_client_rules_over_core() {
    let graph = workspace();
    let direct: BTreeMap<_, _> = graph[CLIENT_MODEL]
        .iter()
        .map(|(name, kinds)| (name.as_str(), kinds.clone()))
        .collect();
    let normal = BTreeSet::from([Kind::Normal]);
    assert_eq!(
        direct,
        BTreeMap::from([
            ("mmorpg-core", normal.clone()),
            ("mmorpg-protocol", BTreeSet::from([Kind::Dev])),
            ("serde", normal.clone()),
            ("serde_json", normal.clone()),
            ("strum", normal),
        ])
    );
    assert_never_links(
        graph,
        &[CLIENT_MODEL],
        &[SCENERY, PRESENTATION, "game-server", "tokio"],
    );
}

/// Host World (docs/NATIVE_CLIENT.md) runs a zone host inside the native client's process, so
/// `mmorpg-client` links `mmorpg-game-server` as a normal dependency. That is the one client
/// crate that does; the browser crates only use it as test evidence.
#[test]
fn only_the_native_client_links_the_network_host_for_host_world() {
    let graph = workspace();
    let linking: Vec<_> = graph
        .iter()
        .filter(|(name, _)| is_workspace_crate(name))
        .filter(|(name, _)| linked(graph, name).contains(NETWORK_HOST))
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(linking, ["mmorpg-client", "mmorpg-scenarios"]);
}

/// World Studio is development tooling: no workspace crate at link time, in either direction.
/// It runs `cargo run -p mmorpg-scenery --example export-world-layers` in a checkout as a
/// separate program instead (docs/WORLD_STUDIO.md).
#[test]
fn world_studio_links_no_game_crate() {
    let graph = workspace();
    assert_eq!(workspace_edges(graph, WORLD_STUDIO), BTreeMap::new());
    for (name, edges) in graph {
        assert!(
            !edges.contains_key(WORLD_STUDIO),
            "{name} must not depend on {WORLD_STUDIO}"
        );
    }
}

/// Physics Engine is a normal dependency of the foundation, shared by native/WASM production;
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
            "mmorpg-local-host",
            include_str!("../../mmorpg-local-host/Cargo.toml"),
        ),
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
    let users: Vec<_> = workspace()
        .iter()
        .filter(|(name, edges)| is_workspace_crate(name) && edges.contains_key("physics-engine"))
        .map(|(name, _)| name.as_str())
        .collect();
    // Core links it only for the authored-backend A/B test.
    assert_eq!(users, ["mmorpg-core", "sim-foundation"]);
}

#[test]
fn a_transitive_link_is_detected_and_a_test_only_edge_is_not() {
    let graph = metadata_graph(&serde_json::json!({
        "packages": [
            {"id": "host", "name": "mmorpg-game-server"},
            {"id": "wire", "name": "mmorpg-protocol"},
            {"id": "scenery", "name": "mmorpg-scenery"},
            {"id": "model", "name": "mmorpg-client-model"},
        ],
        "resolve": {"nodes": [
            {"id": "host", "deps": [
                {"pkg": "wire", "dep_kinds": [{"kind": null}]},
                {"pkg": "model", "dep_kinds": [{"kind": "dev"}]},
            ]},
            {"id": "wire", "deps": [{"pkg": "scenery", "dep_kinds": [{"kind": "build"}]}]},
            {"id": "scenery", "deps": []},
            {"id": "model", "deps": []},
        ]},
    }));
    let host = linked(&graph, NETWORK_HOST);
    assert!(host.contains(SCENERY) && !host.contains(CLIENT_MODEL));
}
