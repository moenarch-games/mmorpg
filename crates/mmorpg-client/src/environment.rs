//! Cosmetic Greyhaven lighting inputs: the hour the sun and moon follow, distance fog and the
//! content's point lights. The renderer (render-engine) turns them into its physical scene.
//! No simulation clock is read here; the hour comes from `mmorpg_presentation::clock` and the
//! curves from `mmorpg_presentation::lighting`.

// `mix` is the browser's quantised sRGB palette interpolation, so both clients fog alike.
use mmorpg_presentation::colour::{mix, rgb};

#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Hours since midnight on the zone's clock; render-engine's `DayCycle` places the sun
    /// and moon from it.
    pub hour: f32,
    /// Projected-viewer cave adaptation (0 outdoors, 1 in the chamber).
    pub interior: f32,
    pub fog: Fog,
    /// The scenery's lights (campfires, lantern posts) at this frame's flicker.
    pub point_lights: Vec<PointLight>,
}

/// Aerial perspective (#526): atmospheric fog whose contrast falls to 5 % at `visibility`
/// metres, red light fading first so distant ranges turn blue but keep their shading. Its
/// colour is sRGB and displayed as is (not exposed).
#[derive(Clone, Debug, PartialEq)]
pub struct Fog {
    pub color: [f32; 3],
    pub visibility: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointLight {
    /// Metres.
    pub position: [f32; 3],
    /// sRGB colour.
    pub color: [f32; 3],
    pub lumens: f32,
    pub range: f32,
}

impl Lighting {
    /// Lighting at `hour` without point lights.
    #[must_use]
    pub fn at(hour: f32) -> Self {
        let angle = f64::from(hour - 12.0) / 24.0 * std::f64::consts::TAU;
        let elevation = angle.cos();
        let amount = ((elevation + 0.15) / 0.8).clamp(0.0, 1.0);
        let daylight = amount * amount * (3.0 - 2.0 * amount);
        let dusk = (1.0 - elevation * elevation).powi(3);
        let blend = |day, night, twilight| mix(mix(night, day, daylight), twilight, dusk * 0.55);
        let horizon = blend(0xcfe0ea, 0x38475e, 0xe4a176);
        Self {
            hour,
            interior: 0.,
            fog: Fog {
                color: rgb(mix(horizon, 0x9faeb3, daylight)),
                visibility: mmorpg_presentation::lighting::FOG_VISIBILITY_METRES,
            },
            point_lights: Vec::new(),
        }
    }
}
