//! The glTF animation maths both native rigid paths share (#1231): skinned units
//! ([`super::skinned`]) and strike/spell kit effects ([`super::kit_effects`]) pose nodes from
//! the same column-major matrices and sample their channels the same way.

/// A column-major 4×4 matrix.
pub(super) type M4 = [f32; 16];

pub(super) fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|i| {
        let (c, r) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum()
    })
}

/// The matrix of translation `t`, rotation quaternion `q` and scale `s`.
pub(super) fn trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> M4 {
    let [x, y, z, w] = q;
    [
        (1. - 2. * (y * y + z * z)) * s[0],
        2. * (x * y + z * w) * s[0],
        2. * (x * z - y * w) * s[0],
        0.,
        2. * (x * y - z * w) * s[1],
        (1. - 2. * (x * x + z * z)) * s[1],
        2. * (y * z + x * w) * s[1],
        0.,
        2. * (x * z + y * w) * s[2],
        2. * (y * z - x * w) * s[2],
        (1. - 2. * (x * x + y * y)) * s[2],
        0.,
        t[0],
        t[1],
        t[2],
        1.,
    ]
}

/// A channel's value at `t`: the keyframe before it when `step`, else interpolated between the
/// keyframes around it, a `rotation` by normalised lerp along the shorter arc.
pub(super) fn sample(
    times: &[f32],
    values: &[[f32; 4]],
    step: bool,
    rotation: bool,
    t: f32,
) -> [f32; 4] {
    let last = times.len() - 1;
    let k = times.partition_point(|&x| x <= t).saturating_sub(1);
    if k >= last || step {
        return values[k.min(last)];
    }
    let (a, b) = (values[k], values[k + 1]);
    let span = times[k + 1] - times[k];
    let u = if span > 0. {
        ((t - times[k]) / span).clamp(0., 1.)
    } else {
        0.
    };
    if rotation {
        let sign = if a.iter().zip(&b).map(|(x, y)| x * y).sum::<f32>() < 0. {
            -1.
        } else {
            1.
        };
        let q: [f32; 4] = std::array::from_fn(|i| a[i] + (b[i] * sign - a[i]) * u);
        let len = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
        q.map(|v| v / len)
    } else {
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * u)
    }
}
