//! The Ranger figures' `humanoid-appearance` presets both clients draw: each figure bundle's own
//! (the male residents', and Tanner Hilda Brook's on the female figure, #493), the game's
//! humanoid enemy presets ([`ENEMY_PRESET_IDS`], #39) authored over the male outfits, and a
//! unit's own seeded variation of a shared preset (asset-tooling #380). The browser reads them
//! through `mmorpg-wasm`, the native client directly, so a guard or bandit is the same man in
//! both. Presentation only, never read by gameplay.
use super::imported::Figure;
use serde_json::{Map, Value, json};

/// The Redbrand band's red on the soldier and knight outfits' `faction` materials (linear).
const REDBRAND_RED: [f64; 4] = [0.3, 0.025, 0.02, 1.];

/// The ids of [`enemy_presets`], in order (`imported::Resident::preset`).
pub const ENEMY_PRESET_IDS: [&str; 5] = [
    "enemy.marauder",
    "enemy.redbrand.bandit",
    "enemy.redbrand.crossbowman",
    "enemy.redbrand.brute",
    "enemy.redbrand.garrick",
];

/// The humanoid enemies' presets (#39), authored over the bundle's own outfits, armour,
/// headwear, hair, beards and face options. The marauders, bandits, crossbowmen and brutes each
/// wear their own seeded variation ([`vary_preset`]); Garrick wears his as authored. Weapons come
/// from the shared presenter's loadout.
#[must_use]
pub fn enemy_presets() -> Vec<Value> {
    vec![
        // Ragged farmhands turned raiders: a smock, long unkempt hair and a full beard.
        json!({ "id": "enemy.marauder", "outfit": "outfit.farmer", "skin": "skin.dark",
            "hair": "hair.long", "tint": "tint.black", "facialHair": ["beard.full"],
            "face": { "eyes": "eyes.narrow", "face": "face.narrow", "mouth": "mouth.thin", "nose": "nose.long" } }),
        // The rank and file: the soldier's gambeson and sash in Redbrand red, bareheaded.
        json!({ "id": "enemy.redbrand.bandit", "outfit": "outfit.soldier", "skin": "skin.light",
            "hair": "hair.buzzed", "tint": "tint.brown", "facialHair": ["beard.short"],
            "hiddenArmor": ["soldier.kettle-hat"], "faction": REDBRAND_RED,
            "face": { "eyes": "eyes.base", "face": "face.base", "mouth": "mouth.thin", "nose": "nose.broad" } }),
        // The outriders keep the kettle hat.
        json!({ "id": "enemy.redbrand.crossbowman", "outfit": "outfit.soldier", "skin": "skin.dark",
            "hair": "hair.buzzed", "tint": "tint.black", "facialHair": ["beard.moustache"],
            "faction": REDBRAND_RED,
            "face": { "eyes": "eyes.narrow", "face": "face.narrow", "mouth": "mouth.base", "nose": "nose.long" } }),
        // The shield-bearers: knight's plate in Redbrand red under the Ironcrest helm.
        json!({ "id": "enemy.redbrand.brute", "outfit": "outfit.knight", "skin": "skin.dark",
            "headwear": "equipment.helm.ironcrest", "tint": "tint.black", "facialHair": ["beard.full"],
            "hiddenArmor": ["knight.bascinet"], "faction": REDBRAND_RED,
            "face": { "eyes": "eyes.base", "face": "face.broad", "mouth": "mouth.full", "nose": "nose.broad" } }),
        // Garrick Redbrand: bareheaded in plate and a deeper crimson, long black hair and a full beard.
        json!({ "id": "enemy.redbrand.garrick", "outfit": "outfit.knight", "skin": "skin.light",
            "hair": "hair.long", "tint": "tint.black", "facialHair": ["beard.full", "beard.moustache"],
            "hiddenArmor": ["knight.bascinet"], "faction": [0.42, 0.015, 0.015, 1],
            "face": { "eyes": "eyes.narrow", "face": "face.broad", "mouth": "mouth.thin", "nose": "nose.broad" } }),
    ]
}

/// The `humanoid-appearance` a figure's characters are drawn from: its bundle's own, with the
/// game's enemy presets beside the male figure's (they wear his outfits, beards and headwear).
/// The female figure's bundle (asset-tooling #378) is drawn as published: her residents' presets
/// (`resident.tanner`, #493) and no male-only options.
#[must_use]
pub fn figure_appearance(figure: Figure, appearance: &Value) -> Value {
    let mut value = appearance.clone();
    if figure == Figure::Female {
        return value;
    }
    if let Some(object) = value.as_object_mut() {
        let presets = object
            .entry("presets")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(list) = presets.as_array_mut() {
            list.extend(enemy_presets());
        }
    }
    value
}

/// The tintable texture variant every hairstyle is drawn in.
pub const HAIR_VARIANT: &str = "hair-1";

/// The hair entries the bundle offers in the tintable [`HAIR_VARIANT`], in its order.
#[must_use]
pub fn tintable_hair(appearance: &Value) -> Vec<String> {
    list(&appearance["hair"])
        .iter()
        .filter(|style| {
            list(&style["textures"])
                .iter()
                .any(|texture| texture["variant"] == HAIR_VARIANT)
        })
        .filter_map(|style| style["entry"].as_str().map(str::to_owned))
        .collect()
}

fn list(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

/// The string `id`s of a list of options.
fn ids(value: &Value) -> Vec<String> {
    list(value)
        .iter()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect()
}

/// A small deterministic generator (mulberry32) over a string seed, bit-for-bit the browser's
/// former `seeded`.
struct Seeded(u32);

impl Seeded {
    fn new(seed: &str) -> Self {
        let mut state = 2_166_136_261_u32;
        for unit in seed.encode_utf16() {
            state = (state ^ u32::from(unit)).wrapping_mul(16_777_619);
        }
        Self(state)
    }

    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let state = self.0;
        let mut t = (state ^ (state >> 15)).wrapping_mul(1 | state);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.
    }

    /// One of `items`; a draw is spent even on an empty list.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        let at = self.next() * items.len() as f64;
        items.get(at as usize)
    }
}

/// A unit's own random-but-fixed variation of the preset `id` (asset-tooling #380: "not every
/// man the same man with a different costume"): the preset's outfit, headwear, faction and hidden
/// armour stay, while skin, hairstyle (of [`tintable_hair`]), hair tint (the beards follow it),
/// facial hair, face features and, where the bundle offers them, age, build, height, skin tone and
/// head-skin stubble are drawn from the bundle's `humanoid-appearance` options by `seed`. The same seed always gives
/// the same man. None when `appearance` has no preset `id`.
#[must_use]
pub fn vary_preset(appearance: &Value, id: &str, seed: u32) -> Option<Value> {
    let hair_styles = tintable_hair(appearance);
    let base = list(&appearance["presets"])
        .iter()
        .find(|preset| preset["id"] == id)?;
    let mut random = Seeded::new(&format!("{id}#{seed}"));
    let mut own: Map<String, Value> = base.as_object()?.clone();
    let outfit = list(&appearance["outfits"])
        .iter()
        .find(|outfit| outfit["id"] == base["outfit"]);
    let skins: Vec<String> = outfit
        .map(|outfit| list(&outfit["skins"]))
        .unwrap_or_default()
        .iter()
        .filter(|skin| skin["variant"].is_string())
        .filter_map(|skin| skin["id"].as_str().map(str::to_owned))
        .collect();
    let skin = random.pick(&skins).cloned();
    let beards = ids(&appearance["facialHair"]["options"]);
    let moustaches: Vec<String> = beards
        .iter()
        .filter(|beard| beard.contains("moustache") || beard.contains("walrus"))
        .cloned()
        .collect();
    let beard = if random.next() < 0.2 {
        None
    } else {
        random.pick(&beards).cloned()
    };
    let facial_hair: Vec<String> = match beard {
        None => Vec::new(),
        Some(beard) => {
            if !moustaches.contains(&beard) && !moustaches.is_empty() && random.next() < 0.35 {
                let moustache = random.pick(&moustaches).cloned();
                [beard].into_iter().chain(moustache).collect()
            } else {
                vec![beard]
            }
        }
    };
    let mut face = base["face"].as_object().cloned().unwrap_or_default();
    for feature in list(&appearance["face"]["features"]) {
        let option = random.pick(&ids(&feature["options"])).cloned();
        if let (Some(feature), Some(option)) = (feature["id"].as_str(), option) {
            face.insert(feature.to_owned(), Value::String(option));
        }
    }
    let skin_tones = &appearance["skinTones"];
    let skin_value = skin
        .clone()
        .map_or_else(|| base["skin"].clone(), Value::String);
    let tones = if skin_tones["skin"] == skin_value && !skin_value.is_null() {
        ids(&skin_tones["tones"])
    } else {
        Vec::new()
    };
    let heights = &appearance["heights"];
    if let Some(skin) = skin {
        own.insert("skin".into(), Value::String(skin));
    }
    let hooded = base["headwear"].as_str().is_some_and(|hat| !hat.is_empty());
    if !hooded && let Some(hair) = random.pick(&hair_styles) {
        own.insert("hair".into(), Value::String(hair.clone()));
    }
    if let Some(tint) = random.pick(&ids(&appearance["tints"])) {
        own.insert("tint".into(), Value::String(tint.clone()));
    }
    own.insert("facialHair".into(), json!(facial_hair));
    own.insert("face".into(), Value::Object(face));
    let mut optional = |key: &str, value: Option<Value>| match value {
        Some(value) => {
            own.insert(key.into(), value);
        }
        None => {
            own.remove(key);
        }
    };
    optional(
        "age",
        random
            .pick(&ids(&appearance["ages"]["options"]))
            .map(|v| json!(v)),
    );
    optional(
        "build",
        random
            .pick(&ids(&appearance["builds"]["options"]))
            .map(|v| json!(v)),
    );
    optional("tone", random.pick(&tones).map(|v| json!(v)));
    let height = match (heights["min"].as_f64(), heights["max"].as_f64()) {
        (Some(min), Some(max)) if max > min => Some(json!(min + random.next() * (max - min))),
        _ => None,
    };
    optional("height", height);
    // Half the men show stubble in the painted head skin when the bundle has head skins.
    let stubble = !appearance["headSkins"].is_null() && random.next() < 0.5;
    optional("stubble", stubble.then_some(Value::Bool(true)));
    Some(Value::Object(own))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mulberry32_matches_the_browser_generator() {
        // `seeded("resident.guard#3")` in the browser's former JavaScript.
        let mut random = Seeded::new("resident.guard#3");
        let draws: Vec<f64> = (0..3).map(|_| random.next()).collect();
        assert_eq!(
            draws,
            [
                0.205_510_765_546_932_82,
                0.134_662_900_352_850_56,
                0.920_486_883_027_479
            ]
        );
    }

    #[test]
    fn a_varied_preset_keeps_its_gear_and_draws_the_rest() {
        let appearance = figure_appearance(
            Figure::Male,
            &json!({
                "presets": [],
                "outfits": [{ "id": "outfit.soldier", "skins": [
                    { "id": "skin.dark", "variant": "outfit.soldier" },
                    { "id": "skin.light", "variant": "outfit.soldier.skin.light" }] }],
                "tints": [{ "id": "tint.black" }, { "id": "tint.grey" }],
                "facialHair": { "options": [{ "id": "beard.full" }, { "id": "beard.moustache" }] },
            }),
        );
        let own = vary_preset(&appearance, "enemy.redbrand.bandit", 7).expect("the bandit preset");
        assert_eq!(own["outfit"], "outfit.soldier");
        assert_eq!(own["faction"], json!(REDBRAND_RED));
        assert_eq!(own["hiddenArmor"], json!(["soldier.kettle-hat"]));
        assert_eq!(
            vary_preset(&appearance, "enemy.redbrand.bandit", 7),
            Some(own)
        );
        assert!(vary_preset(&appearance, "enemy.unknown", 7).is_none());
    }

    #[test]
    fn the_female_figure_keeps_her_own_presets_only() {
        let bundle = json!({ "presets": [{ "id": "resident.tanner", "outfit": "outfit.tanner" }] });
        let female = figure_appearance(Figure::Female, &bundle);
        assert_eq!(female, bundle);
        let male = figure_appearance(Figure::Male, &bundle);
        assert_eq!(male["presets"].as_array().map(Vec::len), Some(6));
        assert_eq!(
            super::super::imported::Resident::of_name("Tanner Hilda Brook")
                .map(super::super::imported::Resident::figure),
            Some(Figure::Female)
        );
    }
}
