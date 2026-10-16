//! Acceptance for #72 (architecture): native input composes the shared
//! `input-bindings` foundation, pinned by git rev, and stays out of the
//! authoritative and host-side crates.
//!
//! - `mmorpg-client` depends on `input-bindings-core` from
//!   `github.com/moritzbrantner/input-bindings` pinned by a full 40-hex `rev`
//!   (directly or through `[workspace.dependencies]`; no branch, tag or path),
//!   and `Cargo.lock` resolves exactly that commit.
//! - `mmorpg-core`, `mmorpg-protocol`, `mmorpg-game-server`, `game-server` and
//!   `mmorpg-scenery` never reach any `input-bindings*` package in the
//!   resolved graph, and their manifests never name it.
//! - `mmorpg_client::controls` reuses `InputRuntime` and
//!   `SemanticControlState` instead of resolving keys itself and knows no
//!   ability ids; `mmorpg_client::hud` reads projections only (no clock, no
//!   simulation).
//! - `desktop.rs` routes ability keys and cast cancelling through
//!   `NativeControls`/`ControlAction` and shows the HUD from `CombatHud`;
//!   its raw Digit1–4 slot table and the direct Escape-to-cancel path are gone.
//!
//! Each check has a fixture test proving it detects a concrete violation.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const FOUNDATION: &str = "input-bindings-core";
const FOUNDATION_REPO: &str = "https://github.com/moritzbrantner/input-bindings";

fn client_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_dir() -> PathBuf {
    client_dir().join("../..")
}

fn read(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The lines of the TOML table `[name]`, up to the next table header.
fn table<'a>(manifest: &'a str, name: &str) -> Vec<&'a str> {
    let header = format!("[{name}]");
    manifest
        .lines()
        .skip_while(|line| line.trim() != header)
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .collect()
}

/// The inline value of `crate_name` in `lines`, from `crate_name = …` or
/// `crate_name.workspace = true`.
fn entry<'a>(lines: &[&'a str], crate_name: &str) -> Option<&'a str> {
    lines.iter().find_map(|line| {
        let rest = line.trim().strip_prefix(crate_name)?;
        let rest = rest.trim_start();
        if rest.starts_with('=') || rest.starts_with(".workspace") {
            Some(line.trim())
        } else {
            None
        }
    })
}

fn quoted_field<'a>(spec: &'a str, field: &str) -> Option<&'a str> {
    spec.split([',', '{', '}']).map(str::trim).find_map(|part| {
        let rest = part.strip_prefix(field)?.trim_start().strip_prefix('=')?;
        rest.trim().strip_prefix('"')?.strip_suffix('"')
    })
}

/// The pinned rev of `input-bindings-core` for the client manifest, following
/// `workspace = true` into the workspace manifest. Errors name the violation.
fn pinned_rev(client: &str, workspace: &str) -> Result<String, String> {
    if entry(&table(client, "dev-dependencies"), FOUNDATION).is_some()
        && entry(&table(client, "dependencies"), FOUNDATION).is_none()
    {
        return Err(format!("{FOUNDATION} is only a dev-dependency"));
    }
    let mut spec = entry(&table(client, "dependencies"), FOUNDATION)
        .ok_or_else(|| format!("{FOUNDATION} is not a [dependencies] entry"))?;
    if spec.contains("workspace") && spec.contains("true") {
        spec = entry(&table(workspace, "workspace.dependencies"), FOUNDATION)
            .ok_or_else(|| format!("{FOUNDATION} is not in [workspace.dependencies]"))?;
    }
    for forbidden in ["path", "branch", "tag", "version"] {
        if quoted_field(spec, forbidden).is_some() {
            return Err(format!("{FOUNDATION} must not use `{forbidden}`: {spec}"));
        }
    }
    let git = quoted_field(spec, "git").ok_or_else(|| format!("not a git dependency: {spec}"))?;
    if git.trim_end_matches(".git") != FOUNDATION_REPO {
        return Err(format!(
            "{FOUNDATION} must come from {FOUNDATION_REPO}, got {git}"
        ));
    }
    let rev = quoted_field(spec, "rev").ok_or_else(|| format!("not pinned by rev: {spec}"))?;
    if rev.len() != 40 || !rev.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("rev must be a full commit hash, got {rev:?}"));
    }
    Ok(rev.to_owned())
}

/// `[[package]]` entries of a lockfile: name → (source, dependency names).
fn lock_packages(lockfile: &str) -> BTreeMap<String, (Option<String>, Vec<String>)> {
    let mut packages = BTreeMap::new();
    for package in lockfile.split("[[package]]").skip(1) {
        let field = |key: &str| {
            package.lines().find_map(|line| {
                line.strip_prefix(&format!("{key} = \""))
                    .and_then(|rest| rest.strip_suffix('"'))
                    .map(str::to_owned)
            })
        };
        let Some(name) = field("name") else { continue };
        let dependencies = package
            .split_once("dependencies = [")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(list, _)| {
                list.lines()
                    .filter_map(|line| line.trim().strip_prefix('"'))
                    .filter_map(|entry| entry.split(['"', ' ']).next())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        packages.insert(name, (field("source"), dependencies));
    }
    packages
}

fn reachable(
    packages: &BTreeMap<String, (Option<String>, Vec<String>)>,
    root: &str,
) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut pending: Vec<String> = packages
        .get(root)
        .map(|(_, dependencies)| dependencies.clone())
        .unwrap_or_default();
    while let Some(name) = pending.pop() {
        if seen.insert(name.clone()) {
            pending.extend(packages.get(&name).into_iter().flat_map(|(_, d)| d.clone()));
        }
    }
    seen
}

/// Host-side or authoritative crates that reach an `input-bindings*` package.
fn leaking_hosts(lockfile: &str) -> Vec<String> {
    let packages = lock_packages(lockfile);
    HOST_SIDE
        .iter()
        .filter(|host| {
            reachable(&packages, host)
                .iter()
                .any(|name| name.starts_with("input-bindings"))
        })
        .map(|host| (*host).to_owned())
        .collect()
}

const HOST_SIDE: [&str; 5] = [
    "mmorpg-core",
    "mmorpg-protocol",
    "mmorpg-game-server",
    "game-server",
    "mmorpg-scenery",
];

/// A module's source, as `src/<name>.rs` or `src/<name>/mod.rs`.
fn module_source(name: &str) -> String {
    let src = client_dir().join("src");
    let file = src.join(format!("{name}.rs"));
    let dir = src.join(name).join("mod.rs");
    if file.exists() {
        read(file)
    } else if dir.exists() {
        read(dir)
    } else {
        panic!("mmorpg_client::{name} has no source (src/{name}.rs or src/{name}/mod.rs)")
    }
}

/// Violations of the desktop routing rule in `desktop` (desktop.rs source).
fn desktop_violations(desktop: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for required in [
        "NativeControls",
        "ControlAction::AbilitySlot",
        "ControlAction::CancelCast",
        ".focus_lost()",
        ".set_casting(",
        "CombatHud",
    ] {
        if !desktop.contains(required) {
            violations.push(format!("desktop.rs does not use `{required}`"));
        }
    }
    for forbidden in [
        "SLOT_KEYS",
        "KeyCode::Digit1",
        "KeyCode::Digit2",
        "KeyCode::Digit3",
        "KeyCode::Digit4",
    ] {
        if desktop.contains(forbidden) {
            violations.push(format!("desktop.rs still maps keys itself: `{forbidden}`"));
        }
    }
    // Each command counter is bumped only in the arm of its control action.
    let lines: Vec<&str> = desktop.lines().collect();
    for (counter, action) in [
        ("cancels.wrapping_add", "ControlAction::CancelCast"),
        ("ability_uses.wrapping_add", "ControlAction::AbilitySlot"),
    ] {
        for (index, _) in lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.contains(counter))
        {
            let window = &lines[index.saturating_sub(10)..index];
            if !window.iter().any(|line| line.contains(action)) {
                violations.push(format!(
                    "desktop.rs line {} bumps `{counter}` outside a `{action}` arm",
                    index + 1
                ));
            }
        }
    }
    violations
}

fn forbidden_in(source: &str, tokens: &[&str]) -> Vec<String> {
    tokens
        .iter()
        .filter(|token| source.contains(**token))
        .map(|token| (*token).to_owned())
        .collect()
}

#[test]
fn the_client_pins_input_bindings_core_by_git_rev() {
    let client = read(client_dir().join("Cargo.toml"));
    let workspace = read(workspace_dir().join("Cargo.toml"));
    let rev = pinned_rev(&client, &workspace).unwrap_or_else(|violation| panic!("{violation}"));
    let lockfile = read(workspace_dir().join("Cargo.lock"));
    let packages = lock_packages(&lockfile);
    let (source, _) = packages
        .get(FOUNDATION)
        .unwrap_or_else(|| panic!("Cargo.lock resolves {FOUNDATION}"));
    let source = source.as_deref().unwrap_or_default();
    assert!(
        source.starts_with(&format!("git+{FOUNDATION_REPO}"))
            && source.ends_with(&format!("#{rev}")),
        "Cargo.lock resolves the pinned commit {rev}, got {source}"
    );
    assert!(
        reachable(&packages, "mmorpg-client").contains(FOUNDATION),
        "mmorpg-client links {FOUNDATION}"
    );
}

#[test]
fn host_side_crates_never_link_input_bindings() {
    let lockfile = read(workspace_dir().join("Cargo.lock"));
    let packages = lock_packages(&lockfile);
    for host in HOST_SIDE {
        assert!(packages.contains_key(host), "{host} is in Cargo.lock");
    }
    assert_eq!(leaking_hosts(&lockfile), Vec::<String>::new());
    for manifest in [
        "crates/mmorpg-core/Cargo.toml",
        "crates/mmorpg-protocol/Cargo.toml",
        "crates/mmorpg-game-server/Cargo.toml",
        "crates/mmorpg-scenery/Cargo.toml",
    ] {
        assert!(
            !read(workspace_dir().join(manifest)).contains("input-bindings"),
            "{manifest} must not name input-bindings"
        );
    }
}

#[test]
fn controls_reuse_the_shared_runtime_and_the_hud_reads_projections_only() {
    let lib = read(client_dir().join("src/lib.rs"));
    for module in ["pub mod controls;", "pub mod hud;"] {
        assert!(lib.contains(module), "lib.rs declares `{module}`");
    }
    let controls = module_source("controls");
    for reused in [
        "input_bindings_core",
        "InputRuntime",
        "SemanticControlState",
    ] {
        assert!(controls.contains(reused), "controls reuses `{reused}`");
    }
    assert_eq!(
        forbidden_in(
            &controls,
            &[
                "ABILITY_CATALOG",
                "ability_by_id",
                "ZoneCommand",
                "ZoneSimulation"
            ]
        ),
        Vec::<String>::new(),
        "controls name slots, not abilities or commands"
    );
    let hud = module_source("hud");
    assert_eq!(
        forbidden_in(
            &hud,
            &[
                "Instant",
                "SystemTime",
                "ZoneSimulation",
                "apply_command",
                "advance_tick"
            ]
        ),
        Vec::<String>::new(),
        "the HUD follows received projections, never a clock or a simulation"
    );
}

#[test]
fn desktop_routes_input_through_controls_and_hud() {
    let desktop = read(client_dir().join("src/desktop.rs"));
    assert_eq!(desktop_violations(&desktop), Vec::<String>::new());
}

#[test]
fn pin_violations_are_detected() {
    let workspace = "[workspace.dependencies]\n";
    let client = |spec: &str| format!("[dependencies]\n{spec}\n\n[dev-dependencies]\n");
    let rev = "d5fcb81bb1eefde9e2f69d404a4d7e429fb2f12c";
    let good = format!(
        r#"input-bindings-core = {{ git = "https://github.com/moritzbrantner/input-bindings.git", rev = "{rev}" }}"#
    );
    assert_eq!(pinned_rev(&client(&good), workspace), Ok(rev.to_owned()));
    let via_workspace = format!("[workspace.dependencies]\n{good}\n");
    assert_eq!(
        pinned_rev(
            &client("input-bindings-core.workspace = true"),
            &via_workspace
        ),
        Ok(rev.to_owned())
    );
    for bad in [
        r#"input-bindings-core = { git = "https://github.com/moritzbrantner/input-bindings.git", branch = "main" }"#,
        r#"input-bindings-core = { git = "https://github.com/moritzbrantner/input-bindings.git", rev = "d5fcb81" }"#,
        r#"input-bindings-core = { git = "https://github.com/someone/input-bindings.git", rev = "d5fcb81bb1eefde9e2f69d404a4d7e429fb2f12c" }"#,
        r#"input-bindings-core = { path = "../../../input-bindings/crates/input-bindings-core" }"#,
        r#"input-bindings-core = "0.1""#,
        "three-d-core = { git = \"https://github.com/moritzbrantner/3d-lab.git\" }",
    ] {
        assert!(pinned_rev(&client(bad), workspace).is_err(), "{bad}");
    }
    let dev_only = format!("[dependencies]\n\n[dev-dependencies]\n{good}\n");
    assert!(pinned_rev(&dev_only, workspace).is_err());
}

#[test]
fn a_transitive_host_dependency_on_input_bindings_is_detected() {
    let lockfile = r#"
[[package]]
name = "mmorpg-game-server"
version = "0.1.0"
dependencies = [
 "game-server",
 "mmorpg-protocol",
]

[[package]]
name = "game-server"
version = "0.1.0"
source = "git+https://github.com/moritzbrantner/game-server.git?rev=1#1"

[[package]]
name = "mmorpg-protocol"
version = "0.1.0"
dependencies = [
 "input-bindings-core",
]

[[package]]
name = "input-bindings-core"
version = "0.1.0"
source = "git+https://github.com/moritzbrantner/input-bindings.git?rev=2#2"
"#;
    assert_eq!(
        leaking_hosts(lockfile),
        ["mmorpg-protocol", "mmorpg-game-server"]
    );
}

#[test]
fn raw_desktop_key_routing_is_detected() {
    // The current baseline shape: a raw slot table and Escape bumping the
    // cancel counter directly from the key event.
    let raw = r#"
const SLOT_KEYS: [KeyCode; 4] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];
fn key(&self, code: KeyCode) {
    if code == KeyCode::Escape && casting {
        self.input.send_modify(|input| {
            input.cancels = input.cancels.wrapping_add(1);
        });
    }
}
"#;
    let violations = desktop_violations(raw);
    for expected in [
        "NativeControls",
        "SLOT_KEYS",
        "KeyCode::Digit1",
        "cancels.wrapping_add",
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(expected)),
            "{expected} is reported in {violations:?}"
        );
    }
    let routed = r#"
let mut controls = NativeControls::new();
controls.set_casting(casting);
controls.focus_lost();
let hud = CombatHud::from_projection(latest);
for action in controls.key_down(code, event.repeat) {
    match action {
        ControlAction::AbilitySlot(slot) => {
            self.input.send_modify(|input| {
                input.ability_uses = input.ability_uses.wrapping_add(1);
            });
        }
        ControlAction::CancelCast => {
            self.input.send_modify(|input| {
                input.cancels = input.cancels.wrapping_add(1);
            });
        }
    }
}
"#;
    assert_eq!(desktop_violations(routed), Vec::<String>::new());
}
