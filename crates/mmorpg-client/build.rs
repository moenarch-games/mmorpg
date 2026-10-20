//! Embeds locally acquired assets; builds perform no network acquisition.
//! Ranger default models and the shared clip library are standard GLBs prepared by
//! asset-tooling (`scripts/prepare-ranger-native.sh`), with original socket metadata.
//! The Greyhaven inn retains its existing manifest/resource embedding path.
#[path = "build/warden_icons.rs"]
mod warden_icons;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const CHARACTER: &str = "humanoid.ranger.male";
const SWORD: &str = "equipment.sword.arming";
const SHIELD: &str = "equipment.shield.heater";
/// The inn entries the clients draw (`mmorpg_presentation::buildings::INN_ENTRIES`).
const INN: [&str; 5] = [
    "greyhaven.inn.shell",
    "greyhaven.inn.furniture",
    "greyhaven.inn.ceiling",
    "greyhaven.inn.roof",
    "greyhaven.inn.door",
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let art = manifest_dir.join("../../web/assets/warden-icons");
    println!("cargo:rerun-if-changed={}", art.display());
    warden_icons::validate(&art)
        .unwrap_or_else(|cause| panic!("Warden artwork validation failed: {cause}"));
    let bundle = manifest_dir.join("../../web/public/characters/humanoid.ranger.male");
    println!(
        "cargo:rerun-if-changed={}",
        bundle.join("RELEASE").display()
    );
    let index = bundle.join("humanoid-ranger-male.index.json");
    println!("cargo:rerun-if-changed={}", index.display());
    let index: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(index)
            .expect("Ranger downloads are missing; run scripts/fetch-bundles.sh"),
    )
    .expect("Ranger download index is JSON");
    assert_eq!(index["schema"], "asset-downloads-v1");
    let character = index["entries"]
        .as_array()
        .expect("download entries")
        .iter()
        .find(|entry| entry["key"] == CHARACTER && entry["variant"] == "default")
        .expect("default character entry");
    let sockets = character["files"]
        .as_array()
        .expect("entry files")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .find(|file| file.ends_with(".humanoid-sockets.json"))
        .expect("character sockets")
        .to_owned();
    let clips = format!(
        "native/{}",
        character["clips"]
            .as_str()
            .expect("shared clips")
            .replace(".glb", ".native.glb")
    );
    let mut files = vec![sockets.clone(), clips.clone()];
    let mut code =
        format!("pub const SOCKETS: &str = {sockets:?};\npub const CLIPS: &str = {clips:?};\n");
    for (name, key) in [
        ("CHARACTER", CHARACTER),
        ("SWORD", SWORD),
        ("SHIELD", SHIELD),
    ] {
        let path = format!("native/{key}.default.native.glb");
        code.push_str(&format!("pub const {name}: &str = {path:?};\n"));
        files.push(path);
    }
    write("ranger_bundle.rs", &bundle, code, files);
    inn(&manifest_dir.join("../../web/public/buildings/greyhaven.inn"));
}

/// The inn's drawn entries as `ENTRIES` (key, glTF path) and their `FILES`.
fn inn(bundle: &Path) {
    for watched in ["RELEASE", "manifest.json"] {
        println!("cargo:rerun-if-changed={}", bundle.join(watched).display());
    }
    let manifest = fs::read_to_string(bundle.join("manifest.json")).unwrap_or_else(|error| {
        panic!("the Greyhaven inn bundle is missing ({error}); run scripts/fetch-bundles.sh")
    });
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).expect("the bundle manifest is JSON");
    let mut code = String::from("pub const ENTRIES: &[(&str, &str)] = &[\n");
    let mut files = Vec::new();
    for key in INN {
        let entry = manifest["assets"]
            .as_array()
            .and_then(|assets| assets.iter().find(|asset| asset["key"] == key))
            .unwrap_or_else(|| panic!("the inn bundle declares {key}"));
        let path = entry["path"].as_str().expect("the entry names its glTF");
        code.push_str(&format!("    ({key:?}, {path:?}),\n"));
        files.push(path.to_owned());
        for resource in entry["resources"].as_array().into_iter().flatten() {
            files.push(
                resource["path"]
                    .as_str()
                    .expect("a resource path")
                    .to_owned(),
            );
        }
    }
    code.push_str("];\n");
    write("inn_bundle.rs", bundle, code, files);
}

/// Appends `FILES` (every bundle file, embedded) to `code` and writes it to `OUT_DIR/name`.
fn write(name: &str, bundle: &Path, mut code: String, mut files: Vec<String>) {
    files.sort();
    files.dedup();
    code.push_str("pub const FILES: &[(&str, &[u8])] = &[\n");
    for file in &files {
        let absolute = bundle
            .join(file)
            .canonicalize()
            .unwrap_or_else(|error| panic!("bundle file {file} is missing ({error}); run scripts/fetch-bundles.sh and scripts/prepare-ranger-native.sh"));
        code.push_str(&format!(
            "    ({file:?}, include_bytes!({:?})),\n",
            absolute.display().to_string()
        ));
    }
    code.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets it")).join(name);
    fs::write(out, code).expect("OUT_DIR is writable");
}
