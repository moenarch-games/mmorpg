//! The named residents (#418, #493) and humanoid enemies (#39) on their Ranger figures, dressed
//! in their shared `humanoid-appearance` presets (`mmorpg_presentation::units::presets`) like
//! the browser: the outfit variant of the preset's skin, its armour less `hiddenArmor`, beards and
//! hair in the preset's tint, headwear, faction colour, skin tone and height, with each guard
//! and rank-and-file enemy in his own seeded variation (asset-tooling #380). The face, age and
//! build morph targets are baked into each look, and the painted head skin of the man's age and
//! stubble replaces the authored one. Each figure (`Figure`) is its own embedded bundle with its
//! own clips, sockets and appearance; options a figure's bundle lacks (the female's beards, head
//! skins and headwear) are simply absent. Presentation only.
use super::{
    Equipment, Model, Plates, SecondaryPose, appearance, ranger_bundle, ranger_female_bundle,
    ranger_file, secondary_of, socket, strings,
};
use crate::world::SceneDraw;
use mmorpg_presentation::units::{
    imported::{Figure, Kind, Layer, Resident},
    kit::Frame,
    presets,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, OnceLock},
};
use strum::VariantArray;

/// One dressed preset: the figure, the plates and beards on their sockets, the headwear and
/// hair, the height factor and the figure's hood nodes, hidden on residents.
pub(in crate::presentation) struct ResidentLook {
    model: Model,
    plates: Plates,
    items: Equipment,
    height: f32,
    hood: &'static [String],
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
        let mut draws = self.model.draws(id, root, height, pose, tint, self.hood);
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

/// The embedded files of `figure`'s bundle.
fn bundle(figure: Figure) -> &'static [(&'static str, &'static [u8])] {
    match figure {
        Figure::Male => ranger_bundle::FILES,
        Figure::Female => ranger_female_bundle::FILES,
    }
}

/// The embedded file `path` of `figure`'s bundle.
fn file(figure: Figure, path: &str) -> &'static [u8] {
    match figure {
        Figure::Male => ranger_file(path),
        Figure::Female => bundle(figure)
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| *bytes)
            .unwrap_or_else(|| panic!("prepared {} file {path} is embedded", figure.character())),
    }
}

/// `figure`'s appearance with the game's presets (`presets::figure_appearance`): the male's
/// with the enemy presets beside the bundle's own, the female's as published.
fn look(figure: Figure) -> &'static Value {
    static MALE: OnceLock<Value> = OnceLock::new();
    static FEMALE: OnceLock<Value> = OnceLock::new();
    match figure {
        Figure::Male => MALE.get_or_init(|| presets::figure_appearance(figure, appearance())),
        Figure::Female => FEMALE.get_or_init(|| {
            let bundle: Value =
                serde_json::from_slice(file(figure, ranger_female_bundle::APPEARANCE))
                    .expect("the appearance descriptor is JSON");
            presets::figure_appearance(figure, &bundle)
        }),
    }
}

fn sockets(figure: Figure) -> &'static Value {
    static MALE: OnceLock<Value> = OnceLock::new();
    static FEMALE: OnceLock<Value> = OnceLock::new();
    let (cell, path) = match figure {
        Figure::Male => (&MALE, ranger_bundle::SOCKETS),
        Figure::Female => (&FEMALE, ranger_female_bundle::SOCKETS),
    };
    cell.get_or_init(|| {
        serde_json::from_slice(file(figure, path)).expect("the sockets descriptor is JSON")
    })
}

/// The glTF nodes of `figure`'s toggleable hood (`humanoid-appearance` part `outfit.hood`).
fn hood(figure: Figure) -> &'static [String] {
    static MALE: OnceLock<Vec<String>> = OnceLock::new();
    static FEMALE: OnceLock<Vec<String>> = OnceLock::new();
    let cell = match figure {
        Figure::Male => &MALE,
        Figure::Female => &FEMALE,
    };
    cell.get_or_init(|| {
        find(&look(figure)["parts"], "id", &"outfit.hood".into())
            .map(|part| strings(&part["nodes"]))
            .unwrap_or_default()
    })
}

/// The clip library prepared with `figure`'s outfit variants.
fn clips(figure: Figure) -> &'static [u8] {
    match figure {
        Figure::Male => file(figure, ranger_bundle::CLIPS),
        Figure::Female => file(figure, ranger_female_bundle::CLIPS),
    }
}

/// `figure`'s cloth chain poses by clip (#500).
fn secondary(figure: Figure) -> &'static BTreeMap<String, SecondaryPose> {
    static POSES: OnceLock<Vec<BTreeMap<String, SecondaryPose>>> = OnceLock::new();
    let poses = POSES.get_or_init(|| {
        Figure::VARIANTS
            .iter()
            .map(|&figure| {
                secondary_of(file(
                    figure,
                    match figure {
                        Figure::Male => ranger_bundle::MOTIONS,
                        Figure::Female => ranger_female_bundle::MOTIONS,
                    },
                ))
            })
            .collect()
    });
    let index = Figure::VARIANTS
        .iter()
        .position(|&candidate| candidate == figure)
        .expect("every figure is listed");
    &poses[index]
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

/// The face morph weights of `preset` on `figure`'s outfit `variant`: each `face` feature's
/// chosen option, or its default; none when the bundle's face keys do not cover the variant.
pub(in crate::presentation) fn face_morphs(
    figure: Figure,
    preset: &Value,
    variant: &str,
) -> BTreeMap<String, f32> {
    let face = &look(figure)["face"];
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

/// The painted head skin (asset-tooling #380) `preset` wears on `figure`: the head material and
/// the base-colour image of the `headSkins` texture for its skin, its age's skin and, when it
/// shows stubble, the stubble shade of its hair tint. None for the authored skin, and on a
/// figure without head skins.
fn head_skin(figure: Figure, preset: &Value) -> Option<(&'static str, &'static gltf::image::Data)> {
    let look = look(figure);
    let skins = &look["headSkins"];
    let age = find(&look["ages"]["options"], "id", &preset["age"])
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
    let image = head_skin_image(
        figure,
        skins["entry"].as_str()?,
        painted["material"].as_str()?,
    )?;
    Some((skins["material"].as_str()?, image))
}

/// The base-colour image of material `name` in `figure`'s prepared head-skin entry `key`,
/// decoded once per process; None when the entry is not embedded or has no such textured
/// material. Only the male bundle publishes head skins.
fn head_skin_image(figure: Figure, key: &str, name: &str) -> Option<&'static gltf::image::Data> {
    type Images = HashMap<String, Option<&'static gltf::image::Data>>;
    static ENTRY: OnceLock<Option<(gltf::Document, Vec<gltf::buffer::Data>)>> = OnceLock::new();
    static IMAGES: OnceLock<Mutex<Images>> = OnceLock::new();
    if figure != Figure::Male {
        return None;
    }
    let (document, buffers) = ENTRY
        .get_or_init(|| {
            if !embedded(figure, key, "default") {
                eprintln!(
                    "head skins keep the authored skin; {} is not prepared",
                    prepared(key, "default")
                );
                return None;
            }
            let gltf = gltf::Gltf::from_slice(file(figure, &prepared(key, "default"))).ok()?;
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

fn embedded(figure: Figure, key: &str, variant: &str) -> bool {
    let path = prepared(key, variant);
    bundle(figure).iter().any(|(name, _)| *name == path)
}

/// A prepared entry of `figure`'s bundle imported once per process: the figure in an outfit
/// variant with its shared clips, or an item.
fn entry(figure: Figure, key: &str, variant: &str) -> &'static Model {
    static MODELS: OnceLock<Mutex<HashMap<(Figure, String), &'static Model>>> = OnceLock::new();
    let path = prepared(key, variant);
    let mut models = MODELS
        .get_or_init(Mutex::default)
        .lock()
        .expect("the model cache is not poisoned");
    models.entry((figure, path.clone())).or_insert_with(|| {
        let bytes = file(figure, &path);
        let model = if key == figure.character() {
            Model::parse_with_clips(bytes, Kind::Ranger.clips().idle, Some(clips(figure)))
                .map(|model| model.with_secondary(secondary(figure)))
        } else {
            Model::parse(bytes, "")
        };
        Box::leak(Box::new(
            model.expect("a prepared Ranger entry is valid glTF"),
        ))
    })
}

/// The outfit variant `preset` wears for its skin, in `look`'s outfits.
fn variant<'a>(look: &'a Value, preset: &Value) -> Option<(&'a Value, &'a str)> {
    let outfit = find(&look["outfits"], "id", &preset["outfit"])?;
    let variant = find(&outfit["skins"], "id", &preset["skin"])?["variant"].as_str()?;
    Some((outfit, variant))
}

/// Every prepared file `resident`'s looks draw: for a varied preset, both skins and every
/// tintable hairstyle.
fn files(resident: Resident) -> Option<Vec<(String, String)>> {
    let figure = resident.figure();
    let look = look(figure);
    let preset = find(&look["presets"], "id", &resident.preset().into())?;
    let (outfit, own) = variant(look, preset)?;
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
        files.push((figure.character().to_owned(), variant.to_owned()));
    }
    if let Some(armor) = outfit["armor"]["entry"].as_str() {
        files.push((armor.to_owned(), "default".to_owned()));
    }
    if let Some(beards) = look["facialHair"]["entry"].as_str() {
        files.push((beards.to_owned(), "default".to_owned()));
    }
    if let Some(headwear) = preset["headwear"].as_str() {
        files.push((headwear.to_owned(), "default".to_owned()));
    }
    let hair = if resident.varied() && preset["headwear"].is_null() {
        presets::tintable_hair(look)
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
    let missing: Vec<String> = Resident::VARIANTS
        .iter()
        .flat_map(|&resident| {
            files(resident).map_or_else(
                || vec![format!("the preset {}", resident.preset())],
                |files| {
                    files
                        .into_iter()
                        .filter(|(key, variant)| !embedded(resident.figure(), key, variant))
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
    let figure = resident.figure();
    let preset = match key.1 {
        Some(seed) => presets::vary_preset(look(figure), id, seed),
        None => find(&look(figure)["presets"], "id", &id.into()).cloned(),
    }
    .unwrap_or_else(|| panic!("the appearance declares the preset {id}"));
    let built: &'static ResidentLook = Box::leak(Box::new(dress(figure, &preset)));
    LOOKS
        .get_or_init(Mutex::default)
        .lock()
        .expect("the look cache is not poisoned")
        .insert(key, built);
    built
}

/// Dresses `figure` in `preset`, as the browser's `residentLooks` and resident `instantiate`.
fn dress(figure: Figure, preset: &Value) -> ResidentLook {
    let look = look(figure);
    let (outfit, variant) =
        variant(look, preset).expect("the preset names its skin's outfit variant");
    let mut model = entry(figure, figure.character(), variant).clone();
    // The painted head skin first: the skin tone below multiplies it as it does the plain one.
    if let Some((material, image)) = head_skin(figure, preset) {
        model.reskin(material, image);
    }
    model.morph(
        Some(&strings(&look["face"]["nodes"])),
        &face_morphs(figure, preset, variant),
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
        let (joint, matrix) = socket(sockets(figure), socket_id);
        (socket_id.to_owned(), model, joint, matrix)
    };
    let mut plates = Vec::new();
    let hidden = strings(&preset["hiddenArmor"]);
    if let Some(armor) = outfit["armor"]["entry"].as_str() {
        let armor = entry(figure, armor, "default");
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
        let beards = entry(
            figure,
            facial["entry"].as_str().expect("a beards entry"),
            "default",
        );
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
        let (joint, matrix) = socket(sockets(figure), at);
        items.push((entry(figure, headwear, "default").clone(), joint, matrix));
    }
    if let Some(style) = preset["hair"].as_str() {
        let mut hair = entry(figure, style, presets::HAIR_VARIANT).clone();
        if figure == Figure::Male && style == "hair.long" {
            // The male long locks are authored lower than the buzzed scalp in the same asset.
            hair.lift("Hair_Long", 0.1);
        }
        if let Some(tint) = tint {
            let materials = find(&look["hair"], "entry", &style.into())
                .map(|style| strings(&style["tintMaterials"]))
                .unwrap_or_default();
            hair.recolor(None, &materials, tint);
        }
        let (joint, matrix) = socket(sockets(figure), "head.hair");
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
            bow: None,
        },
        height: preset["height"].as_f64().map_or(1., |height| height as f32),
        hood: hood(figure),
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
        let preset = find(
            &look(Figure::Male)["presets"],
            "id",
            &"enemy.redbrand.garrick".into(),
        )
        .cloned()
        .expect("Garrick's preset");
        let (_, variant) = variant(look(Figure::Male), &preset).expect("his outfit variant");
        let authored = entry(Figure::Male, "humanoid.ranger.male", variant);
        assert!(
            garrick
                .model
                .primitives
                .iter()
                .zip(&authored.primitives)
                .any(|(own, base)| own.positions != base.positions)
        );
    }

    #[test]
    fn hilda_wears_the_tanner_look_on_the_female_figure() {
        let hilda = resident_look(Resident::Tanner, Some(2));
        // Only the tanner's belt on the pelvis: no beards on the female figure.
        assert_eq!(hilda.plates.items.len(), 1);
        assert_eq!(hilda.plates.items[0].0, "pelvis.armor");
        // Her buns, and no headwear.
        assert_eq!(hilda.items.items.len(), 1);
        // The preset's broad face and nose are baked into her head.
        let preset = find(
            &look(Figure::Female)["presets"],
            "id",
            &"resident.tanner".into(),
        )
        .cloned()
        .expect("the tanner preset");
        let (_, variant) = variant(look(Figure::Female), &preset).expect("her outfit variant");
        assert_eq!(variant, "outfit.tanner.skin.light");
        let authored = entry(Figure::Female, "humanoid.ranger.female", variant);
        assert!(
            hilda
                .model
                .primitives
                .iter()
                .zip(&authored.primitives)
                .any(|(own, base)| own.positions != base.positions)
        );
    }
}
