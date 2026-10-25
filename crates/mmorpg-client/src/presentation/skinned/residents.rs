//! The named residents (#418) and humanoid enemies (#39) on the Ranger figure, dressed in their
//! shared `humanoid-appearance` presets (`mmorpg_presentation::units::presets`) like the
//! browser: the outfit variant of the preset's skin, its armour less `hiddenArmor`, beards and
//! hair in the preset's tint, headwear, faction colour, skin tone and height, with each guard
//! and rank-and-file enemy in his own seeded variation (asset-tooling #380). The face, age and
//! build morph targets are baked into each look, and the painted head skin of the man's age and
//! stubble replaces the authored one. Presentation only.
use super::{
    Equipment, Model, Plates, appearance, hood_nodes, ranger_bundle, ranger_file, socket, strings,
};
use crate::world::SceneDraw;
use mmorpg_presentation::units::{
    imported::{Kind, Layer, Resident},
    kit::Frame,
    presets,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, OnceLock},
};

/// One dressed preset: the figure, the plates and beards on their sockets, the headwear and
/// hair, and the height factor.
pub(in crate::presentation) struct ResidentLook {
    model: Model,
    plates: Plates,
    items: Equipment,
    height: f32,
}

impl ResidentLook {
    /// Draws of the look posed in `pose`, standing at `root` and scaled to `height` metres times
    /// its own height factor, and the model-to-world placement its carried items use.
    pub fn draws(
        &self,
        id: &str,
        root: Frame,
        height: f64,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
    ) -> (&Model, three_d_animation::Mat4, Vec<SceneDraw>) {
        let height = height * f64::from(self.height);
        let placement = self.model.placement(root, height);
        let mut draws = self.model.draws(id, root, height, pose, tint, hood_nodes());
        draws.extend(
            self.plates
                .draws(id, &self.model, placement, pose, tint, |_| true),
        );
        draws.extend(
            self.items
                .draws(&format!("{id}-worn"), &self.model, placement, pose, tint),
        );
        (&self.model, placement, draws)
    }
}

/// The appearance with the game's enemy presets beside the bundle's own.
fn look() -> &'static Value {
    static LOOK: OnceLock<Value> = OnceLock::new();
    LOOK.get_or_init(|| presets::with_enemy_presets(appearance()))
}

fn sockets() -> &'static Value {
    static SOCKETS: OnceLock<Value> = OnceLock::new();
    SOCKETS.get_or_init(|| {
        serde_json::from_slice(ranger_file(ranger_bundle::SOCKETS))
            .expect("the sockets descriptor is JSON")
    })
}

fn list(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

/// `[r, g, b, …]` as a linear colour.
fn rgb(value: &Value) -> Option<[f32; 3]> {
    let c = value.as_array().filter(|c| c.len() >= 3)?;
    Some(std::array::from_fn(|i| c[i].as_f64().unwrap_or(1.) as f32))
}

fn find<'a>(items: &'a Value, key: &str, id: &Value) -> Option<&'a Value> {
    list(items).iter().find(|item| item[key] == *id)
}

/// The morph weights of an appearance option (`face`, `ages` or `builds`); none for None.
fn morphs(option: Option<&Value>) -> BTreeMap<String, f32> {
    option
        .and_then(|option| option["morphs"].as_object())
        .into_iter()
        .flatten()
        .filter_map(|(target, weight)| Some((target.clone(), weight.as_f64()? as f32)))
        .collect()
}

/// The face morph weights of `preset` on the outfit `variant`: each `face` feature's chosen
/// option, or its default; none when the bundle's face keys do not cover the variant.
pub(in crate::presentation) fn face_morphs(preset: &Value, variant: &str) -> BTreeMap<String, f32> {
    let face = &appearance()["face"];
    if !list(&face["variants"]).iter().any(|v| v == variant) {
        return BTreeMap::new();
    }
    let mut weights = BTreeMap::new();
    for feature in list(&face["features"]) {
        let options = &feature["options"];
        let chosen = feature["id"]
            .as_str()
            .and_then(|id| find(options, "id", &preset["face"][id]));
        weights.extend(morphs(
            chosen.or_else(|| find(options, "id", &feature["default"])),
        ));
    }
    weights
}

/// The painted head skin (asset-tooling #380) `preset` wears: the head material and the
/// base-colour image of the `headSkins` texture for its skin, its age's skin and, when it shows
/// stubble, the stubble shade of its hair tint. None for the authored skin.
fn head_skin(preset: &Value) -> Option<(&'static str, &'static gltf::image::Data)> {
    let skins = &look()["headSkins"];
    let age = find(&look()["ages"]["options"], "id", &preset["age"])
        .and_then(|age| age["skin"].as_str())
        .unwrap_or("base");
    let shade = if preset["stubble"] == true {
        preset["tint"]
            .as_str()
            .and_then(|tint| skins["stubbleShades"][tint].as_str())
    } else {
        None
    };
    let painted = list(&skins["textures"]).iter().find(|texture| {
        texture["skin"] == preset["skin"]
            && texture["age"] == age
            && texture["stubble"].as_str() == shade
    })?;
    let image = head_skin_image(skins["entry"].as_str()?, painted["material"].as_str()?)?;
    Some((skins["material"].as_str()?, image))
}

/// The base-colour image of material `name` in the prepared head-skin entry `key`, decoded once
/// per process; None when the entry is not embedded or has no such textured material.
fn head_skin_image(key: &str, name: &str) -> Option<&'static gltf::image::Data> {
    type Images = HashMap<String, Option<&'static gltf::image::Data>>;
    static ENTRY: OnceLock<Option<(gltf::Document, Vec<gltf::buffer::Data>)>> = OnceLock::new();
    static IMAGES: OnceLock<Mutex<Images>> = OnceLock::new();
    let (document, buffers) = ENTRY
        .get_or_init(|| {
            if !embedded(key, "default") {
                eprintln!(
                    "head skins keep the authored skin; {} is not prepared",
                    prepared(key, "default")
                );
                return None;
            }
            let gltf = gltf::Gltf::from_slice(ranger_file(&prepared(key, "default"))).ok()?;
            let buffers = gltf::import_buffers(&gltf.document, None, gltf.blob).ok()?;
            Some((gltf.document, buffers))
        })
        .as_ref()?;
    *IMAGES
        .get_or_init(Mutex::default)
        .lock()
        .expect("the head skin cache is not poisoned")
        .entry(name.to_owned())
        .or_insert_with(|| {
            let material = document.materials().find(|m| m.name() == Some(name))?;
            let texture = material.pbr_metallic_roughness().base_color_texture()?;
            let image =
                gltf::image::Data::from_source(texture.texture().source().source(), None, buffers)
                    .ok()?;
            Some(Box::leak(Box::new(image)))
        })
}

/// The prepared file of bundle entry `key` in `variant`.
fn prepared(key: &str, variant: &str) -> String {
    format!("native/{key}.{variant}.native.glb")
}

fn embedded(key: &str, variant: &str) -> bool {
    let path = prepared(key, variant);
    ranger_bundle::FILES.iter().any(|(name, _)| *name == path)
}

/// A prepared entry imported once per process: the figure in an outfit variant with the shared
/// clips, or an item.
fn entry(key: &str, variant: &str) -> &'static Model {
    static MODELS: OnceLock<Mutex<HashMap<String, &'static Model>>> = OnceLock::new();
    let path = prepared(key, variant);
    let mut models = MODELS
        .get_or_init(Mutex::default)
        .lock()
        .expect("the model cache is not poisoned");
    models.entry(path.clone()).or_insert_with(|| {
        let bytes = ranger_file(&path);
        let model = if key == "humanoid.ranger.male" {
            Model::parse_with_clips(
                bytes,
                Kind::Ranger.clips().idle,
                Some(ranger_file(ranger_bundle::CLIPS)),
            )
        } else {
            Model::parse(bytes, "")
        };
        Box::leak(Box::new(
            model.expect("a prepared Ranger entry is valid glTF"),
        ))
    })
}

/// The outfit variant `preset` wears for its skin.
fn variant(preset: &Value) -> Option<(&'static Value, &'static str)> {
    let outfit = find(&look()["outfits"], "id", &preset["outfit"])?;
    let variant = find(&outfit["skins"], "id", &preset["skin"])?["variant"].as_str()?;
    Some((outfit, variant))
}

/// Every prepared file `resident`'s looks draw: for a varied preset, both skins and every
/// tintable hairstyle.
fn files(resident: Resident) -> Option<Vec<(String, String)>> {
    let preset = find(&look()["presets"], "id", &resident.preset().into())?;
    let (outfit, own) = variant(preset)?;
    let mut files = Vec::new();
    let variants: Vec<&str> = if resident.varied() {
        list(&outfit["skins"])
            .iter()
            .filter_map(|skin| skin["variant"].as_str())
            .collect()
    } else {
        vec![own]
    };
    for variant in variants {
        files.push(("humanoid.ranger.male".to_owned(), variant.to_owned()));
    }
    if let Some(armor) = outfit["armor"]["entry"].as_str() {
        files.push((armor.to_owned(), "default".to_owned()));
    }
    if let Some(beards) = look()["facialHair"]["entry"].as_str() {
        files.push((beards.to_owned(), "default".to_owned()));
    }
    if let Some(headwear) = preset["headwear"].as_str() {
        files.push((headwear.to_owned(), "default".to_owned()));
    }
    let hair = if resident.varied() && preset["headwear"].is_null() {
        presets::tintable_hair(look())
    } else {
        preset["hair"]
            .as_str()
            .map(str::to_owned)
            .into_iter()
            .collect()
    };
    files.extend(
        hair.into_iter()
            .map(|style| (style, presets::HAIR_VARIANT.to_owned())),
    );
    Some(files)
}

/// Whether every resident's and humanoid enemy's preset can be drawn from the embedded
/// preparation; otherwise they keep their procedural models.
pub(in crate::presentation) fn residents_drawable() -> bool {
    let missing: Vec<String> = Resident::ALL
        .iter()
        .flat_map(|&resident| {
            files(resident).map_or_else(
                || vec![format!("the preset {}", resident.preset())],
                |files| {
                    files
                        .into_iter()
                        .filter(|(key, variant)| !embedded(key, variant))
                        .map(|(key, variant)| prepared(&key, &variant))
                        .collect()
                },
            )
        })
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "residents keep their procedural models; missing {} (run scripts/prepare-ranger-native.sh)",
            missing.join(", ")
        );
    }
    missing.is_empty()
}

/// The look `resident` wears: its preset, or with a `variety` seed its own variation of it,
/// built once per process on first use.
pub(in crate::presentation) fn resident_look(
    resident: Resident,
    variety: Option<u32>,
) -> &'static ResidentLook {
    /// Built looks by preset id and variety seed.
    type Looks = HashMap<(&'static str, Option<u32>), &'static ResidentLook>;
    static LOOKS: OnceLock<Mutex<Looks>> = OnceLock::new();
    let id = resident.preset();
    let key = (id, variety.filter(|_| resident.varied()));
    if let Some(look) = LOOKS
        .get_or_init(Mutex::default)
        .lock()
        .expect("the look cache is not poisoned")
        .get(&key)
    {
        return look;
    }
    let preset = match key.1 {
        Some(seed) => presets::vary_preset(look(), id, seed),
        None => find(&look()["presets"], "id", &id.into()).cloned(),
    }
    .unwrap_or_else(|| panic!("the appearance declares the preset {id}"));
    let built: &'static ResidentLook = Box::leak(Box::new(dress(&preset)));
    LOOKS
        .get_or_init(Mutex::default)
        .lock()
        .expect("the look cache is not poisoned")
        .insert(key, built);
    built
}

/// Dresses the figure in `preset`, as the browser's `residentLooks` and resident `instantiate`.
fn dress(preset: &Value) -> ResidentLook {
    let look = look();
    let (outfit, variant) = variant(preset).expect("the preset names its skin's outfit variant");
    let mut model = entry("humanoid.ranger.male", variant).clone();
    // The painted head skin first: the skin tone below multiplies it as it does the plain one.
    if let Some((material, image)) = head_skin(preset) {
        model.reskin(material, image);
    }
    model.morph(
        Some(&strings(&look["face"]["nodes"])),
        &face_morphs(preset, variant),
    );
    let tint =
        find(&look["tints"], "id", &preset["tint"]).and_then(|tint| rgb(&tint["baseColorFactor"]));
    let eyebrows = &look["eyebrows"];
    if let (Some(tint), true) = (tint, eyebrows["followsHairTint"] == true) {
        model.recolor(
            Some(&strings(&eyebrows["nodes"])),
            &strings(&eyebrows["tintMaterials"]),
            tint,
        );
    }
    if let Some(faction) = rgb(&preset["faction"]) {
        model.recolor(None, &strings(&outfit["faction"]["materials"]), faction);
    }
    let tones = &look["skinTones"];
    if tones["skin"] == preset["skin"]
        && let Some(tone) = find(&tones["tones"], "id", &preset["tone"])
            .and_then(|tone| rgb(&tone["baseColorFactor"]))
    {
        model.recolor(None, &strings(&tones["materials"]), tone);
    }
    let held = |socket_id: &str, model: Model| {
        let (joint, matrix) = socket(sockets(), socket_id);
        (socket_id.to_owned(), model, joint, matrix)
    };
    let mut plates = Vec::new();
    let hidden = strings(&preset["hiddenArmor"]);
    if let Some(armor) = outfit["armor"]["entry"].as_str() {
        let armor = entry(armor, "default");
        for piece in list(&outfit["armor"]["pieces"]) {
            let (Some(node), Some(at)) = (piece["node"].as_str(), piece["socket"].as_str()) else {
                continue;
            };
            if !hidden.iter().any(|hidden| hidden == node) {
                plates.push(held(at, armor.piece(node)));
            }
        }
    }
    let facial = &look["facialHair"];
    let beard_tint = find(&facial["tints"], "id", &preset["tint"])
        .and_then(|tint| rgb(&tint["baseColorFactor"]));
    for id in strings(&preset["facialHair"]) {
        let option = find(&facial["options"], "id", &id.as_str().into())
            .unwrap_or_else(|| panic!("the appearance declares the beard {id}"));
        let beards = entry(facial["entry"].as_str().expect("a beards entry"), "default");
        let mut beard = beards.piece(option["node"].as_str().expect("a beard node"));
        if let Some(tint) = beard_tint {
            beard.recolor(None, &strings(&option["tintMaterials"]), tint);
        }
        plates.push(held(
            option["socket"].as_str().expect("a beard socket"),
            beard,
        ));
    }
    let mut items = Vec::new();
    if let Some(headwear) = preset["headwear"].as_str() {
        let at = find(&look["headwear"], "entry", &headwear.into())
            .and_then(|item| item["socket"].as_str())
            .unwrap_or("head.armor");
        let (joint, matrix) = socket(sockets(), at);
        items.push((entry(headwear, "default").clone(), joint, matrix));
    }
    if let Some(style) = preset["hair"].as_str() {
        let mut hair = entry(style, presets::HAIR_VARIANT).clone();
        if style == "hair.long" {
            // The long locks are authored lower than the buzzed scalp in the same asset.
            hair.lift("Hair_Long", 0.1);
        }
        if let Some(tint) = tint {
            let materials = find(&look["hair"], "entry", &style.into())
                .map(|style| strings(&style["tintMaterials"]))
                .unwrap_or_default();
            hair.recolor(None, &materials, tint);
        }
        let (joint, matrix) = socket(sockets(), "head.hair");
        items.push((hair, joint, matrix));
    }
    // The age and build keys after the face, on everything worn: build targets are on the body,
    // armour, hair and beards, age targets on the head, brows and beards.
    let mut body = morphs(find(&look["ages"]["options"], "id", &preset["age"]));
    body.extend(morphs(find(
        &look["builds"]["options"],
        "id",
        &preset["build"],
    )));
    if !body.is_empty() {
        model.morph(None, &body);
        for (_, plate, _, _) in &mut plates {
            plate.morph(None, &body);
        }
        for (item, _, _) in &mut items {
            item.morph(None, &body);
        }
    }
    ResidentLook {
        model,
        plates: Plates { items: plates },
        items: Equipment {
            hanging: vec![false; items.len()],
            items,
        },
        height: preset["height"].as_f64().map_or(1., |height| height as f32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn residents_and_enemies_dress_in_their_presets() {
        assert!(residents_drawable());
        let brute = resident_look(Resident::RedbrandBrute, Some(3));
        // Plate less the bascinet, under the Ironcrest helm, and maybe a beard.
        assert!(brute.plates.items.len() >= 7);
        assert_eq!(brute.items.items.len(), 1);
        assert!(std::ptr::eq(
            brute,
            resident_look(Resident::RedbrandBrute, Some(3))
        ));
        let garrick = resident_look(Resident::Garrick, Some(9));
        assert!(std::ptr::eq(
            garrick,
            resident_look(Resident::Garrick, None)
        ));
        // Garrick's long hair, and two beards among his plates.
        assert_eq!(garrick.items.items.len(), 1);
        // His broad face and narrow eyes are baked into the head.
        let (_, variant) = variant(
            &find(&look()["presets"], "id", &"enemy.redbrand.garrick".into())
                .cloned()
                .expect("Garrick's preset"),
        )
        .expect("his outfit variant");
        let authored = entry("humanoid.ranger.male", variant);
        assert!(
            garrick
                .model
                .primitives
                .iter()
                .zip(&authored.primitives)
                .any(|(own, base)| own.positions != base.positions)
        );
    }
}
