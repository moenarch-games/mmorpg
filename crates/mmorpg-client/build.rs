//! Embeds asset-tooling's bundles from the unpacked releases that
//! `scripts/fetch-bundles.sh` places in `web/public`: the Ranger humanoid
//! (Atlas 6.3.7) character, the Warden's sword and shield entries with the
//! resources the manifest declares and the character's `humanoid-sockets`
//! descriptor; and the Greyhaven inn's drawn entries (#158). Acquisition stays
//! in that script; the build only reads local files.
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
    let bundle = manifest_dir.join("../../web/public/characters/humanoid.ranger.male");
    for watched in ["RELEASE", "manifest.json"] {
        println!("cargo:rerun-if-changed={}", bundle.join(watched).display());
    }
    let manifest = fs::read_to_string(bundle.join("manifest.json")).unwrap_or_else(|error| {
        panic!("the Ranger character bundle is missing ({error}); run scripts/fetch-bundles.sh")
    });
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).expect("the bundle manifest is JSON");
    let entry = |key: &str| {
        manifest["assets"]
            .as_array()
            .and_then(|assets| {
                assets
                    .iter()
                    .find(|asset| asset["key"] == key && asset["variant"] == "default")
            })
            .unwrap_or_else(|| panic!("the bundle declares {key}"))
            .clone()
    };
    let mut files = Vec::new();
    let mut code = String::new();
    for (name, key) in [
        ("CHARACTER", CHARACTER),
        ("SWORD", SWORD),
        ("SHIELD", SHIELD),
    ] {
        let entry = entry(key);
        let path = entry["path"]
            .as_str()
            .expect("the entry names its glTF")
            .to_owned();
        code.push_str(&format!("pub const {name}: &str = {path:?};\n"));
        files.push(path);
        for resource in entry["resources"]
            .as_array()
            .expect("the entry lists resources")
        {
            files.push(
                resource["path"]
                    .as_str()
                    .expect("a resource path")
                    .to_owned(),
            );
        }
    }
    let sockets = entry(CHARACTER)["attachments"]
        .as_array()
        .and_then(|attachments| {
            attachments
                .iter()
                .find(|attachment| attachment["name"] == "humanoid-sockets")
        })
        .and_then(|attachment| attachment["path"].as_str())
        .expect("the character carries its humanoid-sockets descriptor")
        .to_owned();
    code.push_str(&format!("pub const SOCKETS: &str = {sockets:?};\n"));
    files.push(sockets);
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
            .unwrap_or_else(|error| panic!("bundle file {file} is missing ({error})"));
        code.push_str(&format!(
            "    ({file:?}, include_bytes!({:?})),\n",
            absolute.display().to_string()
        ));
    }
    code.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets it")).join(name);
    fs::write(out, code).expect("OUT_DIR is writable");
}
