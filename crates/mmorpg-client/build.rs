//! Embeds locally acquired assets; builds perform no network acquisition.
//! Ranger default models and the shared clip library are standard GLBs prepared by
//! asset-tooling (`scripts/prepare-ranger-native.sh`), with original socket metadata.
//! The Greyhaven inn, the Sunken Shrine kit (#431), the ambient animals (#419), the
//! Outpost building kit (#420) and the settlement props (#421) keep the manifest/resource
//! embedding path.
#[path = "build/warden_icons.rs"]
mod warden_icons;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const CHARACTER: &str = "humanoid.ranger.male";
const SWORD: &str = "equipment.sword.arming";
const SHIELD: &str = "equipment.shield.heater";
/// The Ranger's longbow and quiver stowed on the back (#244).
const BOW: &str = "equipment.bow.long";
const QUIVER: &str = "equipment.quiver.back";
/// The class weapons and props (asset-tooling #375, #379): the Rogue's dagger, the casters'
/// staffs, the Arcanist's tome and the Cleric's mace.
const DAGGER: &str = "equipment.dagger.blackened";
const BONE_STAFF: &str = "equipment.staff.bone";
const CRYSTAL_STAFF: &str = "equipment.staff.crystal";
const GNARLED_STAFF: &str = "equipment.staff.gnarled";
const TOME: &str = "equipment.book.tome";
const MACE: &str = "equipment.mace.flanged";
/// The bundle outfit whose plates equipped drops wear (`imported::DROP_ARMOR_OUTFIT`).
const DROP_ARMOR_OUTFIT: &str = "outfit.knight";
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
        ("BOW", BOW),
        ("QUIVER", QUIVER),
        ("DAGGER", DAGGER),
        ("BONE_STAFF", BONE_STAFF),
        ("CRYSTAL_STAFF", CRYSTAL_STAFF),
        ("GNARLED_STAFF", GNARLED_STAFF),
        ("TOME", TOME),
        ("MACE", MACE),
        ("CAP", CAP),
        ("HELM", HELM),
    ] {
        let path = format!("native/{key}.default.native.glb");
        code.push_str(&format!("pub const {name}: &str = {path:?};\n"));
        files.push(path);
    }
    // The class starter outfits (asset-tooling #379): per `class.<id>` preset, its outfit's
    // variant for the preset's skin and its armour, and the drop armour.
    let look: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(bundle.join(&appearance)).expect("the Ranger appearance is embedded"),
    )
    .expect("the Ranger appearance is JSON");
    let outfit = |id: &str| {
        look["outfits"]
            .as_array()
            .and_then(|outfits| outfits.iter().find(|outfit| outfit["id"] == id))
            .unwrap_or_else(|| panic!("the Ranger appearance declares {id}"))
    };
    let armor = |outfit: &serde_json::Value| {
        let entry = outfit["armor"]["entry"]
            .as_str()
            .expect("an outfit armour entry");
        format!("native/{entry}.default.native.glb")
    };
    let drop = armor(outfit(DROP_ARMOR_OUTFIT));
    code.push_str(&format!("pub const DROP_ARMOR: &str = {drop:?};\n"));
    files.push(drop);
    code.push_str("pub const OUTFITS: &[(&str, &str, &str)] = &[\n");
    for preset in look["presets"].as_array().expect("appearance presets") {
        let id = preset["id"].as_str().expect("a preset id");
        if !id.starts_with("class.") {
            continue;
        }
        let worn = outfit(preset["outfit"].as_str().expect("a preset outfit"));
        let variant = worn["skins"]
            .as_array()
            .and_then(|skins| skins.iter().find(|skin| skin["id"] == preset["skin"]))
            .and_then(|skin| skin["variant"].as_str())
            .unwrap_or_else(|| panic!("the preset {id} names its skin's outfit variant"));
        let model = format!("native/{CHARACTER}.{variant}.native.glb");
        let plates = armor(worn);
        code.push_str(&format!("    ({id:?}, {model:?}, {plates:?}),\n"));
        files.extend([model, plates]);
    }
    code.push_str("];\n");
    let hair = format!("native/{HAIR}.{HAIR_VARIANT}.native.glb");
    code.push_str(&format!(
        "pub const HAIR: &str = {hair:?};\npub const HAIR_KEY: &str = {HAIR:?};\npub const CAP_KEY: &str = {CAP:?};\npub const HELM_KEY: &str = {HELM:?};\n"
    ));
    files.push(hair);
    // Every other prepared entry too: the residents' and humanoid enemies' outfit variants,
    // armour, beards, headwear and hairstyles (#418, #39), looked up by key and variant.
    let native = bundle.join("native");
    println!("cargo:rerun-if-changed={}", native.display());
    for prepared in fs::read_dir(&native).expect("run scripts/prepare-ranger-native.sh") {
        let name = prepared.expect("a prepared file").file_name();
        let name = name.to_str().expect("prepared file names are UTF-8");
        if name.ends_with(".native.glb") {
            files.push(format!("native/{name}"));
        }
    }
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
    // Every prop of the settlement props (#421).
    static_bundle(
        "settlement_bundle.rs",
        &manifest_dir.join("../../web/public/scenery/settlement.props"),
        None,
        false,
    );
    sound_bundle(&manifest_dir.join("../../web/public/sounds/combat-sounds"));
}

/// The CC0 combat sounds (#447, asset-tooling#297): `MANIFEST`, the bundle path of its
/// SoundManifest, and every clip and the manifest as `FILES`, keyed by bundle path (the
/// manifest's clip URLs).
fn sound_bundle(bundle: &Path) {
    for watched in ["SHA256", "manifest.json"] {
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
    let assets = manifest["assets"].as_array().expect("bundle assets");
    let path = |asset: &serde_json::Value| {
        asset["path"]
            .as_str()
            .expect("a bundle asset path")
            .to_owned()
    };
    let sounds = assets
        .iter()
        .find(|asset| asset["key"] == "combat-sounds.sound-manifest")
        .map(path)
        .expect("the bundle declares its SoundManifest");
    let files = assets
        .iter()
        .filter(|asset| asset["source"]["kind"] == "audio")
        .map(path)
        .chain([sounds.clone()])
        .collect();
    write(
        "sound_bundle.rs",
        bundle,
        format!("pub const MANIFEST: &str = {sounds:?};\n"),
        files,
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
