//! Layered natural stone (#732, asset-tooling#567): a kit whose descriptor declares a `surface`
//! block in `"layered"` mode ships geometry with a per-vertex surface mask (`_SURFACE_MASK`:
//! R soil, G moss, B grass, A cavity) and no baked maps. Both clients draw it with a blend over
//! the shared tileable library (`<kit>.surface-library`: limestone colour and normal, moss,
//! soil), projected in the piece's object space at each layer's `tileMetres`, and the
//! [`Weathering`] picks the descriptor's moss threshold and cavity strength.
//!
//! The blend, in the order of the baked kit's bake source: start from limestone, mix in moss by
//! the weathering's moss factor of G, soil by R and moss by B (grass), then darken toward
//! [`CAVITY_COLOUR`] by the cavity strength times A ([`blend`]). The browser runs the same
//! formula per fragment (`web/src/world/layered-surface.ts`). Presentation only.
use serde::{Deserialize, Serialize};

/// The colour a cavity darkens toward (linear RGB), as the descriptor's blend describes.
pub const CAVITY_COLOUR: [f32; 3] = [0.18, 0.17, 0.16];
/// How far above its threshold the moss mask reaches full cover.
pub const MOSS_RAMP: f32 = 0.3;
/// The most moss covers the stone where G alone grows it.
pub const MOSS_COVER: f32 = 0.9;

/// How weathered a layered kit is drawn: a threshold on the moss mask and a cavity strength
/// from the descriptor; no extra textures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Weathering {
    Light,
    Medium,
    Heavy,
}

/// Greyhaven's stone: the vale's damp limestone, between fresh scree and an overgrown ruin.
pub const GREYHAVEN_WEATHERING: Weathering = Weathering::Medium;

/// One weathering level's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Thresholds {
    /// G at which moss starts to cover the stone.
    pub moss: f32,
    /// How strongly A darkens toward [`CAVITY_COLOUR`].
    pub cavity: f32,
}

/// One library layer: its material in the library entry and metres per texture repeat.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub material: String,
    pub tile_metres: f32,
}

/// The library's three layers.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Layers {
    pub limestone: Layer,
    pub moss: Layer,
    pub soil: Layer,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Library {
    key: String,
    layers: Layers,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Levels {
    light: Thresholds,
    medium: Thresholds,
    heavy: Thresholds,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    mode: String,
    #[serde(default)]
    mask_attribute: Option<String>,
    #[serde(default)]
    material: Option<String>,
    #[serde(default)]
    library: Option<Library>,
    #[serde(default)]
    weathering: Option<Levels>,
}

/// A layered kit's surface, from its descriptor's `surface` block.
#[derive(Clone, Debug, PartialEq)]
pub struct LayeredSurface {
    /// The glTF vertex attribute holding the mask (`_SURFACE_MASK`).
    pub mask_attribute: String,
    /// The pieces' shared untextured material the blend replaces.
    pub material: String,
    /// The bundle entry holding the library's materials.
    pub library: String,
    pub layers: Layers,
    weathering: Levels,
}

impl LayeredSurface {
    /// The layered surface a descriptor declares, or `None` for a baked kit (no `surface`
    /// block, or another mode).
    ///
    /// # Errors
    ///
    /// For malformed JSON or a layered block missing its library or weathering.
    pub fn from_descriptor(json: &str) -> Result<Option<Self>, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|error| format!("descriptor: {error}"))?;
        let Some(surface) = value.get("surface") else {
            return Ok(None);
        };
        let block: Block = serde_json::from_value(surface.clone())
            .map_err(|error| format!("descriptor surface: {error}"))?;
        if block.mode != "layered" {
            return Ok(None);
        }
        let missing = |field: &str| format!("descriptor surface: a layered surface needs {field}");
        let library = block.library.ok_or_else(|| missing("library"))?;
        Ok(Some(Self {
            mask_attribute: block
                .mask_attribute
                .unwrap_or_else(|| "_SURFACE_MASK".to_owned()),
            material: block.material.ok_or_else(|| missing("material"))?,
            library: library.key,
            layers: library.layers,
            weathering: block.weathering.ok_or_else(|| missing("weathering"))?,
        }))
    }

    /// The descriptor's numbers for `weathering`.
    #[must_use]
    pub fn thresholds(&self, weathering: Weathering) -> Thresholds {
        match weathering {
            Weathering::Light => self.weathering.light,
            Weathering::Medium => self.weathering.medium,
            Weathering::Heavy => self.weathering.heavy,
        }
    }
}

/// What one mask value blends to: weights of the three layers (summing to 1) and the cavity
/// shade (linear RGB) the result is multiplied by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blend {
    pub limestone: f32,
    pub moss: f32,
    pub soil: f32,
    pub shade: [f32; 3],
}

impl Blend {
    /// Opacities to draw this blend as limestone, then soil over it, then moss over both:
    /// `(soil, moss)`. Compositing them reproduces the weights.
    #[must_use]
    pub fn overlay_alphas(&self) -> (f32, f32) {
        let moss = self.moss.clamp(0., 1.);
        let soil = if moss >= 1. {
            0.
        } else {
            (self.soil / (1. - moss)).clamp(0., 1.)
        };
        (soil, moss)
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// The blend of `mask` (R soil, G moss, B grass, A cavity; each 0..=1) at `thresholds`.
#[must_use]
pub fn blend(mask: [f32; 4], thresholds: Thresholds) -> Blend {
    let [soil, moss, grass, cavity] = mask.map(|channel| channel.clamp(0., 1.));
    // surface = mix(limestone, moss, m); mix(_, soil, r); mix(_, moss, b).
    let m = smoothstep(thresholds.moss, thresholds.moss + MOSS_RAMP, moss) * MOSS_COVER;
    let mut weights = [1. - m, m, 0.];
    for (layer, amount) in [(2, soil), (1, grass)] {
        for weight in &mut weights {
            *weight *= 1. - amount;
        }
        weights[layer] += amount;
    }
    let k = (thresholds.cavity * cavity).clamp(0., 1.);
    Blend {
        limestone: weights[0],
        moss: weights[1],
        soil: weights[2],
        shade: CAVITY_COLOUR.map(|c| 1. + (c - 1.) * k),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTOR: &str = r#"{"surface": {
        "mode": "layered", "maskAttribute": "_SURFACE_MASK", "material": "limestone-rock-layered",
        "library": {"key": "limestone-rocks.kit.surface-library", "edge": 1024, "layers": {
            "limestone": {"material": "surface.limestone", "tileMetres": 2.7},
            "moss": {"material": "surface.moss", "tileMetres": 1.6},
            "soil": {"material": "surface.soil", "tileMetres": 2}}},
        "weathering": {"light": {"moss": 0.6, "cavity": 0.6}, "medium": {"moss": 0.25, "cavity": 1},
            "heavy": {"moss": 0, "cavity": 1.4}}}}"#;

    #[test]
    fn a_layered_descriptor_parses_and_a_baked_one_is_none() {
        let surface = LayeredSurface::from_descriptor(DESCRIPTOR)
            .unwrap()
            .unwrap();
        assert_eq!(surface.library, "limestone-rocks.kit.surface-library");
        assert_eq!(surface.layers.moss.tile_metres, 1.6);
        assert_eq!(
            surface.thresholds(GREYHAVEN_WEATHERING),
            Thresholds {
                moss: 0.25,
                cavity: 1.
            }
        );
        assert_eq!(LayeredSurface::from_descriptor("{}").unwrap(), None);
        assert_eq!(
            LayeredSurface::from_descriptor(r#"{"surface": {"mode": "baked"}}"#).unwrap(),
            None
        );
    }

    #[test]
    fn heavier_weathering_grows_moss_and_the_overlays_composite_to_the_weights() {
        let surface = LayeredSurface::from_descriptor(DESCRIPTOR)
            .unwrap()
            .unwrap();
        let mask = [0.3, 0.5, 0.2, 0.5];
        let light = blend(mask, surface.thresholds(Weathering::Light));
        let heavy = blend(mask, surface.thresholds(Weathering::Heavy));
        assert!(heavy.moss > light.moss);
        assert!(heavy.shade[0] < light.shade[0]);
        for b in [light, heavy] {
            assert!((b.limestone + b.moss + b.soil - 1.).abs() < 1e-5);
            let (soil, moss) = b.overlay_alphas();
            // limestone, then soil at `soil`, then moss at `moss`.
            assert!(((1. - soil) * (1. - moss) - b.limestone).abs() < 1e-5);
            assert!((soil * (1. - moss) - b.soil).abs() < 1e-5);
        }
        assert_eq!(
            blend([0.; 4], surface.thresholds(Weathering::Medium)).limestone,
            1.
        );
    }
}
