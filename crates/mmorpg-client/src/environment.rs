//! Cosmetic Greyhaven lighting, fog and sky. No simulation clock is read here.

#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    /// CSS-equivalent cloud drift in screen pixels; frozen by reduced motion.
    pub cloud_offset: f32,
    pub background: [f32; 3],
    pub sky: Hemisphere,
    pub sun: Sun,
    pub fog: Fog,
    pub sky_stops: SkyStops,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hemisphere {
    pub sky_color: [f32; 3],
    pub ground_color: [f32; 3],
    pub intensity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sun {
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fog {
    pub color: [f32; 3],
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkyStops {
    pub zenith: [f32; 3],
    pub upper: [f32; 3],
    pub horizon: [f32; 3],
    pub ground: [f32; 3],
}

// Match the browser's quantized sRGB palette interpolation before the renderer
// converts to linear light. Keeping the rounding here avoids color drift at dusk.
fn mix(from: u32, to: u32, amount: f64) -> u32 {
    let t = amount.clamp(0.0, 1.0);
    [16, 8, 0].into_iter().fold(0, |color, shift| {
        let a = f64::from((from >> shift) & 255);
        let b = f64::from((to >> shift) & 255);
        color | (((a + (b - a) * t).round() as u32) << shift)
    })
}

fn rgb(color: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((color >> shift) & 255) as f32 / 255.0)
}

impl Environment {
    /// The current Pages twenty-minute cycle; reduced motion holds noon.
    #[must_use]
    pub fn at(seconds: f64, animate: bool) -> Self {
        let phase = if animate && seconds.is_finite() {
            (seconds / 1200.0).rem_euclid(1.0)
        } else {
            0.0
        };
        let angle = phase * std::f64::consts::TAU;
        let elevation = angle.cos();
        let amount = ((elevation + 0.15) / 0.8).clamp(0.0, 1.0);
        let daylight = amount * amount * (3.0 - 2.0 * amount);
        let dusk = (1.0 - elevation * elevation).powi(3);
        let blend = |day, night, twilight| mix(mix(night, day, daylight), twilight, dusk * 0.55);
        let horizon = blend(0xcfe0ea, 0x38475e, 0xe4a176);
        Self {
            cloud_offset: if animate && seconds.is_finite() {
                (seconds.rem_euclid(480.0) / 480.0 * 1400.0) as f32
            } else {
                0.0
            },
            background: rgb(horizon),
            sky: Hemisphere {
                sky_color: rgb(mix(0x8fa4c9, 0x78a9dd, daylight)),
                ground_color: rgb(mix(0x56657b, 0xb8c9c8, daylight)),
                intensity: (0.9 + daylight * 0.8) as f32,
            },
            sun: Sun {
                direction: [
                    (angle.sin() * 18.0 + 10.0 * angle.cos()) as f32,
                    (6.0 + 12.0 * angle.cos().powi(2)) as f32,
                    8.0,
                ],
                color: rgb(mix(mix(0x9cb8eb, 0xfff4df, daylight), 0xffc08a, dusk * 0.7)),
                intensity: (0.35 + daylight * 1.85) as f32,
            },
            fog: Fog {
                color: rgb(mix(horizon, 0xb9cddb, daylight)),
                near: 140.0,
                far: 620.0,
            },
            sky_stops: SkyStops {
                zenith: rgb(blend(0x3f78c4, 0x10223f, 0x554f78)),
                upper: rgb(blend(0x78a9dd, 0x1a3553, 0x986f89)),
                horizon: rgb(horizon),
                ground: rgb(blend(0xb8c9c8, 0x26394a, 0x8e7774)),
            },
        }
    }
}
