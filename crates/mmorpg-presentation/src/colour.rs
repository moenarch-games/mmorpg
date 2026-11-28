//! Packed `0xrrggbb` sRGB colours both clients draw (#1268): one owner for the channel maths,
//! so a world prop, a unit and the native fog shade and blend alike.

/// The colour's channels in `[0, 1]`.
#[must_use]
pub fn rgb(c: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((c >> shift) & 255) as f32 / 255.)
}

/// Every channel scaled by `f`, rounded to the nearest step and kept in `[0, 255]`.
#[must_use]
pub fn shade(c: u32, f: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |out, shift| {
        out | (((f64::from((c >> shift) & 255) * f).round().clamp(0., 255.) as u32) << shift)
    })
}

/// `a` blended towards `b` by `t`, clamped to `[0, 1]`, each channel rounded to the nearest
/// step: the browser's quantised palette interpolation.
#[must_use]
pub fn mix(a: u32, b: u32, t: f64) -> u32 {
    let t = t.clamp(0., 1.);
    [16, 8, 0].into_iter().fold(0, |out, shift| {
        let x = f64::from((a >> shift) & 255);
        let y = f64::from((b >> shift) & 255);
        out | (((x + (y - x) * t).round() as u32) << shift)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shading_rounds_and_saturates_and_mixing_clamps() {
        assert_eq!(rgb(0xff0080), [1., 0., 128. / 255.]);
        assert_eq!(shade(0x808080, 0.5), 0x404040);
        assert_eq!(shade(0x808080, 4.), 0xffffff);
        assert_eq!(shade(0x808080, -1.), 0);
        assert_eq!(mix(0x000000, 0xffffff, 0.5), 0x808080);
        assert_eq!(mix(0x102030, 0xffffff, 2.), 0xffffff);
        assert_eq!(mix(0x102030, 0xffffff, -1.), 0x102030);
    }
}
