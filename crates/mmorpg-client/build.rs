//! Embeds asset-tooling's Ranger humanoid bundle (Atlas 6.3.7): the entry's glTF
//! and the resources its manifest declares, from the unpacked bundle that
//! `scripts/fetch-character-bundle.sh` places in `web/public/characters`.
//! Acquisition stays in that script; the build only reads local files.
use std::{env, fs, path::PathBuf};

const KEY: &str = "humanoid.ranger.male";

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let bundle = manifest_dir.join("../../web/public/characters/humanoid.ranger.male");
    println!(
        "cargo:rerun-if-changed={}",
        bundle.join("RELEASE").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        bundle.join("manifest.json").display()
    );
    let manifest = fs::read_to_string(bundle.join("manifest.json")).unwrap_or_else(|error| {
        panic!(
            "the Ranger character bundle is missing ({error}); run scripts/fetch-character-bundle.sh"
        )
    });
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).expect("the bundle manifest is JSON");
    let entry = manifest["assets"]
        .as_array()
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset["key"] == KEY && asset["variant"] == "default")
        })
        .expect("the bundle declares the Ranger entry");
    let path = entry["path"].as_str().expect("the entry names its glTF");
    let mut files = vec![path.to_owned()];
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
    let mut code =
        format!("pub const ENTRY: &str = {path:?};\npub const FILES: &[(&str, &[u8])] = &[\n");
    for file in &files {
        let absolute = bundle.join(file);
        let absolute = absolute
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
