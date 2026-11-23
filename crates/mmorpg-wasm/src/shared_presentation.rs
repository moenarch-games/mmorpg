//! The shared presentation tables and curves the browser draws from (#1059): Greyhaven's scene
//! lighting by the sun's height, the day/night clock and what a Ranger figure wears besides its
//! loadout. `mmorpg-presentation` owns them; this only translates them for JavaScript, so the
//! browser keeps only its three.js bindings.

use mmorpg_presentation::{
    clock, lighting,
    units::{
        chores::Chores,
        imported::{
            BOLT_GROOVE, BOLT_HAND_SOCKET, BOLT_ITEM, BOLT_SCALE, CROSSBOW_ITEM,
            CROSSBOW_LEFT_JOINT, HAIR_SOCKET, HAIR_VARIANT, HEADWEAR_FIGURE, HairStyle, Hat,
            RESIDENT_HEADWEAR,
        },
        signal,
    },
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use wasm_bindgen::prelude::wasm_bindgen;

/// The equipment presentation tables, as JSON: per headwear name its items and sockets, the
/// residents' headwear and the figure headwear is fitted to; per hairstyle its entry, with the
/// tintable variant and slot; the crossbow and its bolt; every chore item on its socket; and
/// the signal horn.
#[wasm_bindgen(js_name = equipmentTables)]
#[must_use]
pub fn equipment_tables() -> String {
    tables().to_string()
}

fn tables() -> Value {
    let items = |attachments: &[mmorpg_presentation::units::imported::Attachment]| {
        attachments
            .iter()
            .map(|at| json!({ "key": at.item, "socket": at.socket }))
            .collect::<Vec<_>>()
    };
    let headwear: serde_json::Map<_, _> = Hat::ALL
        .iter()
        .map(|hat| (hat.name().to_owned(), Value::from(items(hat.attachments()))))
        .collect();
    let hair: serde_json::Map<_, _> = HairStyle::ALL
        .iter()
        .map(|style| (style.name().to_owned(), Value::from(style.entry())))
        .collect();
    // Each distinct item on its socket: a figure carries every one hidden.
    let chores: BTreeSet<_> = Chores::get()
        .items()
        .map(|chore| (chore.item, chore.socket))
        .collect();
    let (translation, rotation) = BOLT_GROOVE;
    json!({
        "headwear": headwear,
        "residentHeadwear": items(&RESIDENT_HEADWEAR),
        "headwearFigure": HEADWEAR_FIGURE.character(),
        "hair": hair,
        "hairVariant": HAIR_VARIANT,
        "hairSocket": HAIR_SOCKET,
        "crossbow": { "key": CROSSBOW_ITEM, "leftJoint": CROSSBOW_LEFT_JOINT },
        "bolt": {
            "key": BOLT_ITEM,
            "handSocket": BOLT_HAND_SOCKET,
            "scale": BOLT_SCALE,
            "groove": { "translation": translation, "rotation": rotation },
        },
        "chores": chores
            .into_iter()
            .map(|(item, socket)| json!({ "item": item, "socket": socket }))
            .collect::<Vec<_>>(),
        "signalHorn": { "item": signal::HORN.item, "socket": signal::HORN.socket },
    })
}

/// The hour the sky shows at zone `tick` (`clock::sky_hour`): reduced motion steps it hourly.
#[wasm_bindgen(js_name = zoneSkyHour)]
#[must_use]
pub fn zone_sky_hour(tick: u64, animate: bool) -> f32 {
    clock::sky_hour(tick, animate)
}

/// The Pages cycle's hour at `seconds` (`clock::pages_hour`), whose zero is noon.
#[wasm_bindgen(js_name = pagesHour)]
#[must_use]
pub fn pages_hour(seconds: f64, animate: bool) -> f32 {
    clock::pages_hour(seconds, animate)
}

/// The day's fraction since noon at `hour` (`clock::day_fraction`).
#[wasm_bindgen(js_name = dayFraction)]
#[must_use]
pub fn day_fraction(hour: f32) -> f32 {
    clock::day_fraction(hour)
}

/// Whether zone `tick` falls between dusk and dawn (`clock::zone_night`).
#[wasm_bindgen(js_name = isZoneNight)]
#[must_use]
pub fn is_zone_night(tick: u64) -> bool {
    clock::zone_night(tick)
}

/// The lighting constants a renderer binding sets once, as JSON: the sun path, moonlight, the
/// bounce colour and the aerial perspective's per-metre coefficients.
#[wasm_bindgen(js_name = lightingTables)]
#[must_use]
pub fn lighting_tables() -> String {
    let (extinction, inscattering) = lighting::fog_coefficients(lighting::FOG_VISIBILITY_METRES);
    json!({
        "noonElevationDegrees": lighting::NOON_ELEVATION_DEGREES,
        "moonLagHours": lighting::MOON_LAG_HOURS,
        "moonLux": lighting::MOON_LUX,
        "moonColor": lighting::MOON_COLOR,
        "bounceColor": lighting::BOUNCE_COLOR,
        "fogVisibilityMetres": lighting::FOG_VISIBILITY_METRES,
        "fogExtinction": extinction,
        "fogInscattering": inscattering,
    })
    .to_string()
}

/// One frame's [`lighting::SceneLight`] as `[elevation, ev100, transmission,
/// environmentIntensity, bounceNits, sunShadows (0 or 1), sunlight r, g, b, ambient r, g, b,
/// ambientNits]`.
#[wasm_bindgen(js_name = sceneLight)]
#[must_use]
pub fn scene_light(to_sun_up: f32, interior: f32, sky: bool, raw_sunlight: f32) -> Vec<f32> {
    let light = lighting::SceneLight::new(to_sun_up, interior, sky, raw_sunlight);
    let mut out = vec![
        light.elevation,
        light.ev100,
        light.transmission,
        light.environment_intensity,
        light.bounce_nits,
        f32::from(u8::from(light.sun_shadows)),
    ];
    out.extend(light.sunlight);
    out.extend(light.ambient_color);
    out.push(light.ambient_nits);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_name_every_choice_and_the_one_chore_per_item_and_socket() {
        let tables = tables();
        assert_eq!(tables["headwear"]["ranger-cap"][0]["socket"], "head.armor");
        assert_eq!(tables["headwear"]["class"], json!([]));
        assert_eq!(tables["hair"]["parted"], "hair.simple-parted");
        assert_eq!(tables["signalHorn"]["item"], "equipment.tool.signal-horn");
        assert!(!tables["chores"].as_array().unwrap().is_empty());
        assert_eq!(scene_light(1.0, 0.0, true, 1.0).len(), 13);
    }
}
