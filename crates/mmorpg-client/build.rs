//! Embeds locally acquired assets; builds perform no network acquisition.
//! Ranger default models and the shared clip library are standard GLBs prepared by
//! asset-tooling (`scripts/prepare-ranger-native.sh`), with original socket metadata.
//! The Greyhaven inn, the Sunken Shrine kit (#431), the ambient animals (#419) and the
//! Outpost building kit (#420) keep the manifest/resource embedding path.
#[path = "build/warden_icons.rs"]
mod warden_icons;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const CHARACTER: &str = "humanoid.ranger.male";
const SWORD: &str = "equipment.sword.arming";
const SHIELD: &str = "equipment.shield.heater";
/// The class weapons (asset-tooling #375): the Rogue's dagger and the casters' staffs.
const DAGGER: &str = "equipment.dagger.blackened";
const BONE_STAFF: &str = "equipment.staff.bone";
const GILDED_STAFF: &str = "equipment.staff.gilded";
const GNARLED_STAFF: &str = "equipment.staff.gnarled";
/// The authored headwear on `head.armor` (#374).
const CAP: &str = "equipment.cap.ranger";
const HELM: &str = "equipment.helm.ironcrest";
/// The starter hair (#376) in its tintable texture variant.
const HAIR: &str = "hair.simple-parted";
const HAIR_VARIANT: &str = "hair-1";
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
    let appearance = character["files"]
        .as_array()
        .expect("entry files")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .find(|file| file.ends_with(".humanoid-appearance.json"))
        .expect("character appearance")
        .to_owned();
    let clips = format!(
        "native/{}",
        character["clips"]
            .as_str()
            .expect("shared clips")
            .replace(".glb", ".native.glb")
    );
    let mut files = vec![sockets.clone(), clips.clone(), appearance.clone()];
    let mut code = format!(
        "pub const SOCKETS: &str = {sockets:?};\npub const CLIPS: &str = {clips:?};\npub const APPEARANCE: &str = {appearance:?};\n"
    );
    for (name, key) in [
        ("CHARACTER", CHARACTER),
        ("SWORD", SWORD),
        ("SHIELD", SHIELD),
        ("DAGGER", DAGGER),
        ("BONE_STAFF", BONE_STAFF),
        ("GILDED_STAFF", GILDED_STAFF),
        ("GNARLED_STAFF", GNARLED_STAFF),
        ("CAP", CAP),
        ("HELM", HELM),
    ] {
        let path = format!("native/{key}.default.native.glb");
        code.push_str(&format!("pub const {name}: &str = {path:?};\n"));
        files.push(path);
    }
    let hair = format!("native/{HAIR}.{HAIR_VARIANT}.native.glb");
    code.push_str(&format!(
        "pub const HAIR: &str = {hair:?};\npub const HAIR_KEY: &str = {HAIR:?};\npub const CAP_KEY: &str = {CAP:?};\npub const HELM_KEY: &str = {HELM:?};\n"
    ));
    files.push(hair);
    write("ranger_bundle.rs", &bundle, code, files);
    static_bundle(
        "inn_bundle.rs",
        &manifest_dir.join("../../web/public/buildings/greyhaven.inn"),
        Some(&INN),
        false,
    );
    // Every module of the Sunken Shrine kit (#431).
    static_bundle(
        "shrine_bundle.rs",
        &manifest_dir.join("../../web/public/scenery/shrine.kit"),
        None,
        false,
    );
    // Greyhaven's ambient cat, dog and rodent (#419).
    static_bundle(
        "ambient_bundle.rs",
        &manifest_dir.join("../../web/public/scenery/greyhaven.ambient-animals"),
        None,
        false,
    );
    // Every module and roof variant of the Outpost building kit (#420).
    static_bundle(
        "outpost_bundle.rs",
        &manifest_dir.join("../../web/public/buildings/outpost.kit"),
        None,
        true,
    );
}

/// A static bundle's drawn entries (`keys`, or every entry) as `ENTRIES` (key, glTF path) and
/// their `FILES`. With `variants`, every other variant of an entry is drawn too, keyed
/// `<key>/<variant>`.
fn static_bundle(name: &str, bundle: &Path, keys: Option<&[&str]>, variants: bool) {
    for watched in ["RELEASE", "SHA256", "manifest.json"] {
        println!("cargo:rerun-if-changed={}", bundle.join(watched).display());
    }
    let manifest = fs::read_to_string(bundle.join("manifest.json")).unwrap_or_else(|error| {
        panic!(
            "the bundle {} is missing ({error}); run scripts/fetch-bundles.sh",
            bundle.display()
        )
    });
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).expect("the bundle manifest is JSON");
    let mut code = String::from("pub const ENTRIES: &[(&str, &str)] = &[\n");
    let mut files = Vec::new();
    let assets = manifest["assets"].as_array().expect("bundle assets");
    let entries: Vec<&serde_json::Value> = match keys {
        Some(keys) => keys
            .iter()
            .map(|key| {
                assets
                    .iter()
                    .find(|asset| asset["key"] == *key && asset["variant"] == "default")
                    .unwrap_or_else(|| panic!("the bundle declares {key}"))
            })
            .collect(),
        None => assets
            .iter()
            .filter(|asset| variants || asset["variant"] == "default")
            .collect(),
    };
    for entry in entries {
        let key = entry["key"].as_str().expect("the entry names its key");
        let key = match entry["variant"].as_str() {
            Some("default") | None => key.to_owned(),
            Some(variant) => format!("{key}/{variant}"),
        };
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
    write(name, bundle, code, files);
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
