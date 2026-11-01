//! Cosmetic Greyhaven lighting inputs: the hour the sun and moon follow, distance fog and the
//! content's point lights. The renderer (render-engine) turns them into its physical scene.
//! No simulation clock is read here.

#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Hours since midnight on the zone's clock; render-engine's `DayCycle` places the sun
    /// and moon from it.
    pub hour: f32,
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

/// How far Greyhaven's air hides the scene (#526): about 9 % haze at 100 m, 43 % at 600 m,
/// so the distant ranges stay shaded silhouettes. `FOG_VISIBILITY_METRES` in
/// `web/src/world/lighting.ts`.
pub const FOG_VISIBILITY_METRES: f32 = 3200.0;
/// Per-channel weights of the fog's extinction and inscattering: red fades and blue gathers
/// first. `FOG_EXTINCTION` and `FOG_INSCATTERING` in `web/src/world/lighting.ts`.
pub const FOG_EXTINCTION: [f32; 3] = [1.2, 1.0, 0.8];
pub const FOG_INSCATTERING: [f32; 3] = [0.8, 1.0, 1.2];

#[derive(Clone, Debug, PartialEq)]
pub struct PointLight {
    /// Metres.
    pub position: [f32; 3],
    /// sRGB colour.
    pub color: [f32; 3],
    pub lumens: f32,
    pub range: f32,
}

// Match the browser's quantized sRGB palette interpolation, so both clients fog alike.
fn mix(from: u32, to: u32, amount: f64) -> u32 {
    let t = amount.clamp(0.0, 1.0);
    [16, 8, 0].into_iter().fold(0, |color, shift| {
        let a = f64::from((from >> shift) & 255);
        let b = f64::from((to >> shift) & 255);
        color | (((a + (b - a) * t).round() as u32) << shift)
    })
}

pub(crate) fn rgb(color: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((color >> shift) & 255) as f32 / 255.0)
}

/// The zone's hour at `tick` (#158): the sky follows the zone's clock rather than the client's.
#[must_use]
pub fn zone_hour(tick: u64) -> f32 {
    (f64::from(mmorpg_core::time_of_day(tick)) / f64::from(mmorpg_core::DAY_TICKS) * 24.0) as f32
}

/// The Pages twenty-minute cycle's hour at `seconds`, whose zero is noon; reduced motion holds
/// noon.
#[must_use]
pub fn pages_hour(seconds: f64, animate: bool) -> f32 {
    let phase = if animate && seconds.is_finite() {
        (seconds / 1200.0).rem_euclid(1.0)
    } else {
        0.0
    };
    ((12.0 + phase * 24.0) % 24.0) as f32
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
            fog: Fog {
                color: rgb(mix(horizon, 0xb9cddb, daylight)),
                visibility: FOG_VISIBILITY_METRES,
            },
            point_lights: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_cycle_starts_at_noon_and_the_zone_at_eight() {
        assert_eq!(pages_hour(0.0, true), 12.0);
        assert_eq!(pages_hour(600.0, true), 0.0);
        assert_eq!(pages_hour(600.0, false), 12.0);
        assert!((zone_hour(0) - 8.0).abs() < 1e-4);
    }
}
