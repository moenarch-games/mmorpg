//! Embeds locally acquired assets; builds perform no network acquisition.
//! The Ranger figures and their equipment are embedded as asset-tooling publishes them, file by
//! file (#866): packed glTF the client decodes itself (`presentation::skinned::packed`).
//! The Greyhaven inn, the Sunken Shrine kit (#431), the ambient animals (#419), the
//! Outpost building kit (#420), the settlement props (#421) and Greyhaven's trees, rocks and bushes
//! (#49), their ground layer materials (#525) and the strike and spell effects (#894) keep the
//! manifest/resource embedding path.
#[path = "build/warden_icons.rs"]
mod warden_icons;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const CHARACTER: &str = "humanoid.ranger.male";
/// The female figure (#493, `imported::Figure::Female`).
const FEMALE: &str = "humanoid.ranger.female";
/// The figures' weapons, throwables and headwear (asset-tooling #517).
const EQUIPMENT: &str = "humanoid.equipment";
/// The loadout items whose sockets the native client poses specially: the Ranger's longbow
/// stowed on the back (#244) and the Rogue's dagger with its belt throwables (#603). Every
/// loadout's items and sockets are the shared `imported::Loadout::attachments` (#1012), found
/// in `ENTRIES`.
const BOW: &str = "equipment.bow.long";
const DAGGER: &str = "equipment.dagger.blackened";
/// The Redbrand Crossbowmen's crossbow and the arrow standing in for its bolt (#607).
const CROSSBOW: &str = "equipment.crossbow";
const ARROW: &str = "equipment.arrow";
/// The Rogue's throwables (`imported::Throwable::item`, #611).
const THROWING_KNIFE: &str = "equipment.throwing-knife";
const VENOM_FLASK: &str = "equipment.venom-flask";
const BLASTING_BOMB: &str = "equipment.blasting-bomb";
/// The bundle outfit whose plates equipped drops wear (`imported::DROP_ARMOR_OUTFIT`).
const DROP_ARMOR_OUTFIT: &str = "outfit.knight";
/// The authored headwear on `head.armor` (#374).
const CAP: &str = "equipment.cap.ranger";
const HELM: &str = "equipment.helm.ironcrest";
/// The starter hair (#376) in its tintable texture variant.
const HAIR: &str = "hair.simple-parted";
const HAIR_VARIANT: &str = "hair-1";
/// The inn entries the clients draw (`mmorpg_presentation::buildings::INN`).
const INN: [&str; 5] = [
    "greyhaven.inn.shell",
    "greyhaven.inn.furniture",
    "greyhaven.inn.ceiling",
    "greyhaven.inn.roof",
    "greyhaven.inn.door",
];
/// Elden's house entries the clients draw (`mmorpg_presentation::buildings::MARSHAL_HOUSE`).
const MARSHAL_HOUSE: [&str; 5] = [
    "greyhaven.marshal-house.shell",
    "greyhaven.marshal-house.furniture",
    "greyhaven.marshal-house.ceiling",
    "greyhaven.marshal-house.roof",
    "greyhaven.marshal-house.door",
];

/// The crag kit entries the clients place (`mmorpg_presentation::crag::KIT_ENTRIES`): the box
/// modules (#668) and the outcrops the world dressing places (#683).
const CRAG: [&str; 18] = [
    "crag.kit.cap-1",
    "crag.kit.cap-2",
    "crag.kit.cap-corner",
    "crag.kit.corner-climb-1",
    "crag.kit.corner-climb-2",
    "crag.kit.corner-sheer-1",
    "crag.kit.corner-sheer-2",
    "crag.kit.face-climb-1",
    "crag.kit.face-climb-2",
    "crag.kit.face-climb-3",
    "crag.kit.face-sheer-1",
    "crag.kit.face-sheer-2",
    "crag.kit.face-sheer-3",
    "crag.kit.outcrop-buttress-1",
    "crag.kit.outcrop-buttress-2",
    "crag.kit.outcrop-spire",
    "crag.kit.outcrop-talus-1",
    "crag.kit.outcrop-talus-2",
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let art = manifest_dir.join("../../web/assets/warden-icons");
    println!("cargo:rerun-if-changed={}", art.display());
    warden_icons::validate(&art)
        .unwrap_or_else(|cause| panic!("Warden artwork validation failed: {cause}"));
    let characters = manifest_dir.join("../../web/public/characters");
    let equipment = Downloads::read(&characters, EQUIPMENT);
    let male = Downloads::read(&characters, CHARACTER);
    let (mut code, appearance) = figure(&male, &equipment);
    for (name, key) in [
        ("CHARACTER", CHARACTER),
        ("BOW", BOW),
        ("DAGGER", DAGGER),
        ("CROSSBOW", CROSSBOW),
        ("ARROW", ARROW),
        ("THROWING_KNIFE", THROWING_KNIFE),
        ("VENOM_FLASK", VENOM_FLASK),
        ("BLASTING_BOMB", BLASTING_BOMB),
        ("CAP", CAP),
        ("HELM", HELM),
    ] {
        let source = if key == CHARACTER { &male } else { &equipment };
        let path = source.gltf(key, "default");
        code.push_str(&format!("pub const {name}: &str = {path:?};\n"));
    }
    // The class starter outfits (asset-tooling #379): per `class.<id>` preset, its outfit's
    // variant for the preset's skin and its armour, and the drop armour.
    let appearance_path = characters.join(&appearance);
    println!("cargo:rerun-if-changed={}", appearance_path.display());
    let look: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&appearance_path).expect("the Ranger appearance is embedded"),
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
        male.gltf(entry, "default")
    };
    let drop = armor(outfit(DROP_ARMOR_OUTFIT));
    code.push_str(&format!("pub const DROP_ARMOR: &str = {drop:?};\n"));
    code.push_str("pub const OUTFITS: &[(&str, &str, &str)] = &[\n");
    for preset in look["presets"].as_array().unwrap_or_else(|| {
        panic!(
            "Ranger appearance {} has no presets; refresh the downloaded assets with scripts/fetch-bundles.sh",
            appearance_path.display()
        )
    }) {
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
        let model = male.gltf(CHARACTER, variant);
        let plates = armor(worn);
        code.push_str(&format!("    ({id:?}, {model:?}, {plates:?}),\n"));
    }
    code.push_str("];\n");
    // The legacy Ranger hair is optional after asset-tooling #517: an empty path names no file.
    let hair = male
        .entry(HAIR, HAIR_VARIANT)
        .map_or_else(String::new, |entry| male.file(entry, ".gltf"));
    code.push_str(&format!(
        "pub const HAIR: &str = {hair:?};\npub const HAIR_KEY: &str = {HAIR:?};\npub const CAP_KEY: &str = {CAP:?};\npub const HELM_KEY: &str = {HELM:?};\n"
    ));
    let files = [male.files(), equipment.files()].concat();
    write("ranger_bundle.rs", &characters, code, files);
    // The female figure (#493): Tanner Hilda Brook's outfit, armour and hair.
    let female = Downloads::read(&characters, FEMALE);
    let (code, _) = figure(&female, &equipment);
    let files = [female.files(), equipment.files()].concat();
    write("ranger_female_bundle.rs", &characters, code, files);
    // The Redbrand faction kit (#675): every entry and its descriptor. Optional: without it the
    // bandits keep the soldier's outfit.
    optional_kit(
        &characters,
        "redbrand.kit",
        "redbrand-descriptor",
        "redbrand",
        "the bandits keep the soldier's outfit",
    );
    // The hair and beard kit (#677). Optional: without it the figures keep the Ranger's hair.
    optional_kit(
        &characters,
        "hair.kit",
        "hair-descriptor",
        "hair",
        "the figures keep the Ranger bundle's hair and beards",
    );
    static_bundle(
        "inn_bundle.rs",
        &manifest_dir.join("../../web/public/buildings/greyhaven.inn"),
        Some(&INN),
        false,
    );
    // Marshal Elden Greywatch's private home (#158).
    static_bundle(
        "marshal_house_bundle.rs",
        &manifest_dir.join("../../web/public/buildings/greyhaven.marshal-house"),
        Some(&MARSHAL_HOUSE),
        false,
    );
    static_bundle(
        "monuments_bundle.rs",
        &manifest_dir.join("../../web/public/buildings/monuments.kit"),
        Some(&[
            "monuments.kit.mausoleum-walk-in",
            "monuments.kit.mausoleum-walk-in-roof",
        ]),
        false,
    );
    // Every module of the Sunken Shrine kit (#431).
    static_bundle(
        "shrine_bundle.rs",
        &manifest_dir.join("../../web/public/scenery/shrine.kit"),
        None,
        false,
    );
    // The cat is installed separately and may be received from the host (#733).
    // The binary contains only its immutable manifest identity, never its mesh bytes.
    static_bundle(
        "ambient_bundle.rs",
        &manifest_dir.join("../../web/public/scenery/greyhaven.ambient-animals"),
        Some(&["greyhaven.ambient.dog", "greyhaven.ambient.rodent"]),
        false,
    );
    // Greyhaven's farm animals (#849), every one.
    static_bundle(
        "farm_bundle.rs",
        &manifest_dir.join("../../web/public/scenery/greyhaven.farm-animals"),
        None,
        false,
    );
    // Greyhaven's wildlife (#857), every one.
    static_bundle(
        "wild_bundle.rs",
        &manifest_dir.join("../../web/public/scenery/greyhaven.wildlife"),
        None,
        false,
    );
    // Its descriptor, which names each animal's rest clip (#989).
    descriptor(
        &manifest_dir.join("../../web/public/scenery/greyhaven.wildlife"),
        "wildlife-descriptor",
        "wild_descriptor.rs",
    );
    // The strike and spell effects (#894), every one.
    static_bundle(
        "strike_effects_bundle.rs",
        &manifest_dir.join("../../web/public/effects/strike.effects"),
        None,
        false,
    );
    static_bundle(
        "spell_effects_bundle.rs",
        &manifest_dir.join("../../web/public/effects/spell.effects"),
        None,
        false,
    );
    let ambient: serde_json::Value = serde_json::from_slice(
        &fs::read(
            manifest_dir.join("../../web/public/scenery/greyhaven.ambient-animals/manifest.json"),
        )
        .expect("ambient manifest"),
    )
    .expect("ambient manifest JSON");
    use sha2::{Digest, Sha256};
    let identity: String =
        Sha256::digest(serde_json::to_vec(&ambient).expect("manifest serializes"))
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("shared_art.rs"),
        format!("pub const BUNDLE_IDENTITY: &str = {identity:?};\n"),
    )
    .expect("shared art metadata");
    // Every module and roof variant of the Outpost building kit (#420).
    static_bundle(
        "outpost_bundle.rs",
        &manifest_dir.join("../../web/public/buildings/outpost.kit"),
        None,
        true,
    );
    static_bundle(
        "fort_bundle.rs",
        &manifest_dir.join("../../web/public/authored/greyhaven-fort"),
        None,
        false,
    );
    // Every prop of the settlement props (#421) and their descriptor, for the placed-kit library.
    let settlement = manifest_dir.join("../../web/public/scenery/settlement.props");
    static_bundle("settlement_bundle.rs", &settlement, None, false);
    descriptor(
        &settlement,
        "settlement-descriptor",
        "settlement_descriptor.rs",
    );
    sound_bundle(&manifest_dir.join("../../web/public/sounds/combat-sounds"));
    // Greyhaven's trees, rocks and bushes with every level of detail (#49), and their descriptor.
    let nature = manifest_dir.join("../../web/public/scenery/greyhaven.nature");
    static_bundle("nature_bundle.rs", &nature, None, true);
    descriptor(&nature, "nature-descriptor", "nature_descriptor.rs");
    // Greyhaven's ladders, ropes and ivy with every level of detail (#621), and their descriptor.
    let climbing = manifest_dir.join("../../web/public/scenery/climbing.kit");
    static_bundle("climbing_bundle.rs", &climbing, None, true);
    descriptor(&climbing, "climbing-descriptor", "climbing_descriptor.rs");
    // The crag kit's rock modules over the highland crags (#668) and its outcrops the world
    // dressing places (#683), with every level of detail, and their descriptor; not its macro
    // texture, which nothing places.
    let crag = manifest_dir.join("../../web/public/scenery/crag.kit");
    static_bundle("crag_bundle.rs", &crag, Some(&CRAG), true);
    descriptor(&crag, "crag-descriptor", "crag_descriptor.rs");
    // Greyhaven's limestone rocks (asset-tooling#536) with every level of detail, and their descriptor.
    let limestone = manifest_dir.join("../../web/public/scenery/limestone-rocks.kit");
    static_bundle("limestone_bundle.rs", &limestone, None, true);
    descriptor(
        &limestone,
        "limestone-rocks-descriptor",
        "limestone_descriptor.rs",
    );
    // The stones along Greyhaven's brook (asset-tooling#538), and their descriptor.
    let river = manifest_dir.join("../../web/public/scenery/river-stones.kit");
    static_bundle("river_bundle.rs", &river, None, true);
    descriptor(&river, "river-stones-descriptor", "river_descriptor.rs");
    let meadow = manifest_dir.join("../../web/public/scenery/greyhaven-meadow.kit");
    static_bundle("meadow_bundle.rs", &meadow, None, true);
    descriptor(
        &meadow,
        "greyhaven-meadow-descriptor",
        "meadow_descriptor.rs",
    );
    // The basin's ore veins and their spent look (#917, asset-tooling#763), and their descriptor.
    let gatherables = manifest_dir.join("../../web/public/scenery/gatherables.kit");
    static_bundle("gatherables_bundle.rs", &gatherables, None, true);
    descriptor(
        &gatherables,
        "gatherables-descriptor",
        "gatherables_descriptor.rs",
    );
    // Every ordinary placed kit of the shared inventory (#683), and their descriptors.
    placed_kits(&manifest_dir);
    // Greyhaven's ground layer materials (#525).
    let terrain = manifest_dir.join("../../web/public/scenery/greyhaven.terrain");
    static_bundle("terrain_bundle.rs", &terrain, None, false);
}

/// Every ordinary placed kit of `mmorpg-presentation`'s `placed-kits.json` (#683), embedded as
/// `placed_kit_<n>_bundle.rs` and `placed_kit_<n>_descriptor.rs`, and `placed_kits.rs`: their
/// `KITS` in inventory order, each a kit's glTF entries, files and descriptor JSON.
fn placed_kits(manifest_dir: &Path) {
    let inventory = manifest_dir.join("../mmorpg-presentation/placed-kits.json");
    println!("cargo:rerun-if-changed={}", inventory.display());
    let inventory: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&inventory).expect("the placed-kit inventory is readable"),
    )
    .expect("the placed-kit inventory is JSON");
    let mut code = String::new();
    let mut kits = String::from("pub const KITS: &[Kit] = &[\n");
    let kit_list = inventory["kits"]
        .as_array()
        .expect("the inventory lists kits");
    for (index, kit) in kit_list.iter().enumerate() {
        let bundle = kit["bundle"].as_str().expect("a kit names its bundle");
        let name = kit["descriptor"]
            .as_str()
            .expect("a kit names its descriptor");
        let directory = manifest_dir.join("../../web/public").join(bundle);
        static_bundle(
            &format!("placed_kit_{index}_bundle.rs"),
            &directory,
            None,
            true,
        );
        descriptor(
            &directory,
            name,
            &format!("placed_kit_{index}_descriptor.rs"),
        );
        code.push_str(&format!(
            "mod bundle_{index} {{ include!(concat!(env!(\"OUT_DIR\"), \"/placed_kit_{index}_bundle.rs\")); }}\n\
             mod descriptor_{index} {{ include!(concat!(env!(\"OUT_DIR\"), \"/placed_kit_{index}_descriptor.rs\")); }}\n"
        ));
        kits.push_str(&format!(
            "    Kit {{ entries: bundle_{index}::ENTRIES, files: bundle_{index}::FILES, descriptor: descriptor_{index}::DESCRIPTOR }},\n"
        ));
    }
    kits.push_str("];\n");
    code.push_str(&kits);
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("placed_kits.rs"),
        code,
    )
    .expect("OUT_DIR is writable");
}

/// The optional kit `directory` beside the figures, as `<module>_bundle.rs` and
/// `<module>_descriptor.rs`: every entry and its descriptor attachment `name`, or none of them
/// when the kit was not fetched, and `absent` says what the client keeps.
fn optional_kit(characters: &Path, directory: &str, name: &str, module: &str, absent: &str) {
    let kit = characters.join(directory);
    let bundle = format!("{module}_bundle.rs");
    let description = format!("{module}_descriptor.rs");
    if kit.join("manifest.json").is_file() {
        static_bundle(&bundle, &kit, None, true);
        descriptor(&kit, name, &description);
        return;
    }
    println!("cargo:rerun-if-changed={}", characters.display());
    println!(
        "cargo:warning=the {directory} kit is not fetched; {absent} (run scripts/fetch-bundles.sh)"
    );
    write(
        &bundle,
        &kit,
        "pub const ENTRIES: &[(&str, &str)] = &[];\n".into(),
        Vec::new(),
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets it"));
    fs::write(
        out.join(description),
        "pub const DESCRIPTOR: &str = \"\";\n",
    )
    .expect("OUT_DIR is writable");
}

/// A bundle's descriptor attachment `name` as `DESCRIPTOR` in `out`: the nature bundle's
/// `nature-descriptor` (#49, its levels of detail and the rocks' collision proxies) or the
/// climbing kit's `climbing-descriptor` (#621).
fn descriptor(bundle: &Path, name: &str, out: &str) {
    let manifest = fs::read_to_string(bundle.join("manifest.json")).unwrap_or_else(|error| {
        panic!(
            "the bundle {} is missing ({error}); run scripts/fetch-bundles.sh",
            bundle.display()
        )
    });
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).expect("the bundle manifest is JSON");
    let path = manifest["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|asset| asset["attachments"].as_array().into_iter().flatten())
        .find(|attachment| attachment["name"] == name)
        .and_then(|attachment| attachment["path"].as_str())
        .unwrap_or_else(|| panic!("the bundle {} attaches its {name}", bundle.display()));
    let absolute = bundle
        .join(path)
        .canonicalize()
        .unwrap_or_else(|_| panic!("the {name} is in the bundle"));
    let code = format!(
        "pub const DESCRIPTOR: &str = include_str!({:?});\n",
        absolute.display().to_string()
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets it")).join(out);
    fs::write(out, code).expect("OUT_DIR is writable");
}

/// The CC0 combat sounds (#447, asset-tooling#297): `MANIFEST`, the bundle path of its
/// SoundManifest, and every clip and the manifest as `FILES`, keyed by bundle path (the
/// manifest's clip URLs).
fn sound_bundle(bundle: &Path) {
    for watched in ["export.ref.json", "manifest.json"] {
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

/// A release `scripts/fetch-bundles.sh` fetches file by file (`fetch_rolling_files`, #866): a
/// Ranger figure or their equipment, with its `asset-downloads-v1` index. Paths are relative to
/// `web/public/characters/`, the root both figures' `FILES` are embedded from.
struct Downloads {
    directory: String,
    entries: Vec<serde_json::Value>,
    /// A figure's per-family clip files (#898, asset-tooling #733); empty without them.
    families: Vec<serde_json::Value>,
}

impl Downloads {
    fn read(characters: &Path, directory: &str) -> Self {
        let bundle = characters.join(directory);
        for watched in ["RELEASE", "DIGESTS"] {
            println!("cargo:rerun-if-changed={}", bundle.join(watched).display());
        }
        let index = bundle.join(format!("{}.index.json", directory.replace('.', "-")));
        let index: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&index).unwrap_or_else(|error| {
                panic!(
                    "{} is missing ({error}); run scripts/fetch-bundles.sh",
                    index.display()
                )
            }))
            .expect("a download index is JSON");
        assert_eq!(index["schema"], "asset-downloads-v1");
        Self {
            directory: directory.to_owned(),
            entries: index["entries"]
                .as_array()
                .expect("download entries")
                .clone(),
            families: index["clips"]["families"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        }
    }

    fn entry(&self, key: &str, variant: &str) -> Option<&serde_json::Value> {
        self.entries
            .iter()
            .find(|entry| entry["key"] == key && entry["variant"] == variant)
    }

    /// The path of `entry`'s one file ending in `suffix`.
    fn file(&self, entry: &serde_json::Value, suffix: &str) -> String {
        let name = entry["files"]
            .as_array()
            .expect("entry files")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|file| file.ends_with(suffix))
            .unwrap_or_else(|| panic!("the {} entry has its {suffix}", entry["key"]));
        format!("{}/{name}", self.directory)
    }

    /// The glTF path of entry `key` in `variant`.
    fn gltf(&self, key: &str, variant: &str) -> String {
        let entry = self.entry(key, variant).unwrap_or_else(|| {
            panic!(
                "{} declares no {key}/{variant}; run scripts/fetch-bundles.sh",
                self.directory
            )
        });
        self.file(entry, ".gltf")
    }

    /// Every entry's files, textures and shared clips, and the clip family files, as published.
    fn files(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|entry| {
                ["files", "textures"]
                    .iter()
                    .flat_map(|list| entry[*list].as_array().into_iter().flatten())
                    .chain(entry.get("clips"))
                    .filter_map(serde_json::Value::as_str)
            })
            .chain(
                self.families
                    .iter()
                    .filter_map(|family| family["file"].as_str()),
            )
            .map(|name| format!("{}/{name}", self.directory))
            .collect()
    }

    /// `CLIP_FAMILIES` lines: each family's name, file path and clips with their lengths.
    fn clip_families(&self) -> String {
        self.families
            .iter()
            .map(|family| {
                let clips: String = family["clips"]
                    .as_array()
                    .expect("a family's clips")
                    .iter()
                    .map(|clip| {
                        let name = clip["name"].as_str().expect("a clip name");
                        #[allow(clippy::cast_possible_truncation, reason = "clip lengths in f32")]
                        let duration =
                            clip["durationSeconds"].as_f64().expect("a clip length") as f32;
                        format!("({name:?}, {duration:?}), ")
                    })
                    .collect();
                format!(
                    "    ({:?}, {:?}, &[{clips}]),\n",
                    family["family"].as_str().expect("a family name"),
                    format!(
                        "{}/{}",
                        self.directory,
                        family["file"].as_str().expect("a family file")
                    )
                )
            })
            .collect()
    }

    /// `ENTRIES` lines: each entry's `<key>/<variant>` and glTF path.
    fn entries(&self) -> String {
        self.entries
            .iter()
            .map(|entry| {
                let id = format!(
                    "{}/{}",
                    entry["key"].as_str().expect("an entry key"),
                    entry["variant"].as_str().expect("an entry variant")
                );
                format!("    ({id:?}, {:?}),\n", self.file(entry, ".gltf"))
            })
            .collect()
    }
}

/// A Ranger figure's code (`imported::Figure`): its `SOCKETS`, shared `CLIPS`, `APPEARANCE`, clip
/// metadata `MOTIONS` and `ENTRIES` (its own and the `equipment`'s), and the appearance path.
fn figure(figure: &Downloads, equipment: &Downloads) -> (String, String) {
    let character = figure.directory.as_str();
    let entry = figure
        .entry(character, "default")
        .unwrap_or_else(|| panic!("{character} declares its default entry"));
    let sockets = figure.file(entry, ".humanoid-sockets.json");
    let appearance = figure.file(entry, ".humanoid-appearance.json");
    let clips = format!(
        "{character}/{}",
        entry["clips"].as_str().expect("shared clips")
    );
    // The clips' `humanoid-clips` metadata: the bow clips' draw (#556).
    let motions = figure.file(entry, ".humanoid-clips.json");
    let code = format!(
        "pub const SOCKETS: &str = {sockets:?};\npub const CLIPS: &str = {clips:?};\npub const CLIP_FAMILIES: &[super::ClipFamily] = &[\n{}];\npub const APPEARANCE: &str = {appearance:?};\npub const MOTIONS: &str = {motions:?};\npub const ENTRIES: &[(&str, &str)] = &[\n{}{}];\n",
        figure.clip_families(),
        figure.entries(),
        equipment.entries()
    );
    (code, appearance)
}

/// A static bundle's drawn entries (`keys`, or every entry) as `ENTRIES` (key, glTF path) and
/// their `FILES`. With `variants`, every other variant of an entry is drawn too, keyed
/// `<key>/<variant>`.
fn static_bundle(name: &str, bundle: &Path, keys: Option<&[&str]>, variants: bool) {
    for watched in ["RELEASE", "export.ref.json", "manifest.json"] {
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
        // The listed keys (with every level when `variants`).
        Some(keys) => keys
            .iter()
            .flat_map(|key| {
                let levels: Vec<_> = assets
                    .iter()
                    .filter(|asset| {
                        asset["key"] == *key && (variants || asset["variant"] == "default")
                    })
                    .collect();
                assert!(!levels.is_empty(), "the bundle declares {key}");
                levels
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
        let absolute = bundle.join(file).canonicalize().unwrap_or_else(|error| {
            panic!("bundle file {file} is missing ({error}); run scripts/fetch-bundles.sh")
        });
        code.push_str(&format!(
            "    ({file:?}, include_bytes!({:?})),\n",
            absolute.display().to_string()
        ));
    }
    code.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets it")).join(name);
    fs::write(out, code).expect("OUT_DIR is writable");
}
