//! Greyhaven's scene lighting (#526, #529, #600, #1059): the sun path, exposure by sun elevation,
//! moonlight, the sunlit ground's bounce, aerial perspective and, for a frame drawn without the
//! atmosphere, an analytic sky. Both clients light alike from here; each keeps only its renderer
//! binding (render-engine natively, its three.js port in the browser), which places the sun and
//! moon from [`NOON_ELEVATION_DEGREES`] and [`MOON_LAG_HOURS`] and turns [`SceneLight`] into its
//! scene. Presentation only: nothing here feeds gameplay.

/// Greyhaven's sun path: 60° above the southern horizon at noon ...
pub const NOON_ELEVATION_DEGREES: f32 = 60.0;
/// ... and the moon opposite, 12 hours behind.
pub const MOON_LAG_HOURS: f32 = 12.0;
/// Moonlight in lux: brighter than a real full moon (0.25 lx) so night stays playable.
pub const MOON_LUX: f32 = 2.0;
/// Cool, slightly blue moonlight, linear RGB.
pub const MOON_COLOR: [f32; 3] = [0.75, 0.85, 1.0];
/// EV100 by sun elevation in degrees, interpolated linearly: render-engine's `sky_day_cycle`
/// oracle curve with a half-stop darker noon for Greyhaven's earth/stone palette, and a
/// brighter-than-physical night for play.
pub const EXPOSURE: [(f32, f32); 5] = [
    (-12.0, 2.0),
    (-4.0, 6.0),
    (2.0, 11.0),
    (15.0, 13.8),
    (60.0, 15.0),
];
/// Below this sun elevation (degrees) the sun casts no shadows; its light has faded out.
pub const SHADOWLESS_BELOW: f32 = -3.0;
/// Daylight bounce from the sunlit ground, cd/m², and its warm grass-and-earth colour (#874): the
/// atmosphere's environment light is the sky alone, so a sun shadow on the meadow kept only that
/// cold sky light and read as an ink-black slab. A uniform fill lifts shadows to about a quarter
/// of the sunlit ground while barely brightening it.
pub const BOUNCE_NITS: f32 = 6000.0;
/// The bounce's colour, linear RGB.
pub const BOUNCE_COLOR: [f32; 3] = [1.0, 0.96, 0.86];
/// How far Greyhaven's air hides the scene (#526): atmospheric fog whose contrast falls to
/// [`FOG_CONTRAST_THRESHOLD`] here, about 5 % haze at 100 m and 26 % at 600 m, so the distant
/// ranges stay shaded silhouettes.
pub const FOG_VISIBILITY_METRES: f32 = 6000.0;
/// Bevy's revised Koschmieder contrast threshold.
pub const FOG_CONTRAST_THRESHOLD: f32 = 0.05;
/// Per-channel weights of the fog's extinction and inscattering: red fades and blue gathers
/// first, so distant ranges turn blue but keep their shading.
pub const FOG_EXTINCTION: [f32; 3] = [1.1, 1.0, 0.9];
/// See [`FOG_EXTINCTION`].
pub const FOG_INSCATTERING: [f32; 3] = [0.9, 1.0, 1.1];

/// Without the atmosphere the sun is filtered by an analytic Earth atmosphere: Bevy's Rayleigh,
/// Mie and ozone optical depths at the zenith, over the Kasten–Young air mass.
const ZENITH_OPTICAL_DEPTH: [f32; 3] = [0.0663, 0.1468, 0.2762];
/// Without the atmosphere, the sky's ambient light in cd/m² by sun elevation in degrees, tuned
/// against the native frames ...
const SKY_AMBIENT: [(f32, f32); 7] = [
    (-12.0, 0.35),
    (-4.0, 3.0),
    (0.0, 900.0),
    (3.0, 2000.0),
    (8.0, 3500.0),
    (15.0, 6000.0),
    (60.0, 11000.0),
];
/// ... in this colour by day ...
const SKY_COLOR: [f32; 3] = [0.8, 0.88, 1.0];
/// ... turning to this one round sunrise and sunset.
const TWILIGHT_COLOR: [f32; 3] = [0.86, 0.84, 0.88];

fn interpolate(table: &[(f32, f32)], x: f32) -> f32 {
    let first = table[0];
    if x <= first.0 {
        return first.1;
    }
    for pair in table.windows(2) {
        let [(x0, y0), (x1, y1)] = [pair[0], pair[1]];
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    table[table.len() - 1].1
}

/// EV100 for a sun `elevation` in degrees, from [`EXPOSURE`].
#[must_use]
pub fn ev100(elevation: f32) -> f32 {
    interpolate(&EXPOSURE, elevation)
}

/// The sun's elevation in degrees from the up component of the unit direction towards it.
#[must_use]
pub fn sun_elevation(to_sun_up: f32) -> f32 {
    to_sun_up.clamp(-1.0, 1.0).asin().to_degrees()
}

/// The atmospheric fog's per-metre extinction and inscattering coefficients for a `visibility`
/// in metres (Koschmieder at [`FOG_CONTRAST_THRESHOLD`], weighted by [`FOG_EXTINCTION`] and
/// [`FOG_INSCATTERING`]).
#[must_use]
pub fn fog_coefficients(visibility: f32) -> ([f32; 3], [f32; 3]) {
    let density = -FOG_CONTRAST_THRESHOLD.ln() / visibility;
    (
        FOG_EXTINCTION.map(|weight| weight * density),
        FOG_INSCATTERING.map(|weight| weight * density),
    )
}

/// The share of the raw sunlight reaching the ground at a sun `elevation` in degrees, per linear
/// RGB channel, through the analytic atmosphere; zero below the horizon, fading out over the
/// last half degree as the disc sets.
#[must_use]
pub fn sunlight(elevation: f32) -> [f32; 3] {
    if elevation <= -0.5 {
        return [0.0; 3];
    }
    let e = elevation.max(0.0);
    let air_mass = 1.0 / (e.to_radians().sin() + 0.50572 * (e + 6.07995).powf(-1.6364));
    let setting = ((elevation + 0.5) / 0.5).min(1.0);
    ZENITH_OPTICAL_DEPTH.map(|depth| setting * (-depth * air_mass).exp())
}

/// One frame's lighting by the sun's height, the viewer's cave adaptation and whether the
/// atmosphere draws the sky: everything a client's renderer binding sets besides the sun's and
/// moon's directions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneLight {
    /// The sun's elevation in degrees.
    pub elevation: f32,
    /// The camera's exposure: [`ev100`] by the sun, darkened towards a cave's in the interior.
    pub ev100: f32,
    /// How much outdoor light (sun, moon, bounce) reaches the viewer: 1 outdoors, a
    /// thousandth deep in a cave.
    pub transmission: f32,
    /// The sky's environment light relative to the atmosphere's own: less diffuse fill by day,
    /// so the relief and cast shadows keep their contrast.
    pub environment_intensity: f32,
    /// The [`BOUNCE_COLOR`] fill's brightness in cd/m², by daylight.
    pub bounce_nits: f32,
    /// Whether the sun casts shadows (above [`SHADOWLESS_BELOW`]).
    pub sun_shadows: bool,
    /// Without the atmosphere: the analytic sunlight's lux per linear RGB channel ...
    pub sunlight: [f32; 3],
    /// ... and the sky's ambient light colour ...
    pub ambient_color: [f32; 3],
    /// ... and brightness in cd/m².
    pub ambient_nits: f32,
}

impl SceneLight {
    /// The lighting with the sun in direction `to_sun_up` (the up component of the unit
    /// direction towards it), the viewer `interior` (0 outdoors, 1 in a cave) and the
    /// atmosphere drawing the `sky` (the world) or not (the character-selection stage, whose
    /// studio fill stays). `raw_sunlight` is the renderer's sunlight above the atmosphere, lux.
    #[must_use]
    pub fn new(to_sun_up: f32, interior: f32, sky: bool, raw_sunlight: f32) -> Self {
        let elevation = sun_elevation(to_sun_up);
        let interior = interior.clamp(0.0, 1.0);
        let exposure = ev100(elevation);
        let transmission = 10_f32.powf(-3.0 * interior);
        // Keep the night/cave fill while allowing daylight relief and cast shadows to read.
        let daylight = (elevation / 15.0).clamp(0.0, 1.0);
        let sky_fill = if sky { 0.9 - 0.25 * daylight } else { 0.9 };
        let environment_intensity =
            (sky_fill + (1.0 - sky_fill) * interior) * 10_f32.powf(-interior);
        let twilight = (1.0 - elevation.abs() / 8.0).max(0.0);
        Self {
            elevation,
            ev100: exposure + (exposure.min(7.5) - exposure) * interior,
            transmission,
            environment_intensity,
            bounce_nits: BOUNCE_NITS * daylight * transmission,
            sun_shadows: elevation > SHADOWLESS_BELOW,
            sunlight: sunlight(elevation).map(|share| share * raw_sunlight),
            ambient_color: std::array::from_fn(|i| {
                SKY_COLOR[i] + (TWILIGHT_COLOR[i] - SKY_COLOR[i]) * twilight
            }),
            ambient_nits: interpolate(&SKY_AMBIENT, elevation)
                * transmission
                * environment_intensity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_keeps_daylight_contrast_and_brightens_night() {
        assert_eq!(ev100(60.0), 15.0);
        assert_eq!(ev100(90.0), 15.0);
        assert_eq!(ev100(15.0), 13.8);
        assert_eq!(ev100(2.0), 11.0);
        assert_eq!(ev100(-40.0), 2.0);
        assert!(ev100(-8.0) > 2.0 && ev100(-8.0) < 6.0);
    }

    #[test]
    fn a_cave_darkens_the_outdoor_light_and_opens_the_exposure() {
        let noon = 60f32.to_radians().sin();
        let outdoors = SceneLight::new(noon, 0.0, true, 1.0);
        let cave = SceneLight::new(noon, 1.0, true, 1.0);
        assert_eq!(outdoors.ev100, 15.0);
        assert_eq!(cave.ev100, 7.5);
        assert!((cave.transmission - 1e-3).abs() < 1e-6);
        assert!(outdoors.sun_shadows);
        assert!((outdoors.bounce_nits - BOUNCE_NITS).abs() < 1e-2);
        assert!(!SceneLight::new(-0.2, 0.0, true, 1.0).sun_shadows);
    }

    #[test]
    fn the_analytic_sun_reddens_low_and_sets() {
        let noon = sunlight(60.0);
        let low = sunlight(2.0);
        assert!(noon[0] > noon[2] && low[0] / low[2] > noon[0] / noon[2]);
        assert_eq!(sunlight(-1.0), [0.0; 3]);
        let (extinction, inscattering) = fog_coefficients(FOG_VISIBILITY_METRES);
        assert!(extinction[0] > extinction[2] && inscattering[2] > inscattering[0]);
    }
}
