//! Embeds asset-tooling's Ranger humanoid bundle (Atlas 6.3.7): the character,
//! the Warden's sword and shield entries with the resources the manifest
//! declares, and the character's `humanoid-sockets` descriptor, from the
//! unpacked bundle that `scripts/fetch-character-bundle.sh` places in
//! `web/public/characters`. Acquisition stays in that script; the build only
//! reads local files.
use std::{env, fs, path::PathBuf};

const CHARACTER: &str = "humanoid.ranger.male";
const SWORD: &str = "equipment.sword.arming";
const SHIELD: &str = "equipment.shield.heater";

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let bundle = manifest_dir.join("../../web/public/characters/humanoid.ranger.male");
    for watched in ["RELEASE", "manifest.json"] {
        println!("cargo:rerun-if-changed={}", bundle.join(watched).display());
    }
    let manifest = fs::read_to_string(bundle.join("manifest.json")).unwrap_or_else(|error| {
        panic!(
            "the Ranger character bundle is missing ({error}); run scripts/fetch-character-bundle.sh"
        )
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
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets it")).join("ranger_bundle.rs");
    fs::write(out, code).expect("OUT_DIR is writable");
}
