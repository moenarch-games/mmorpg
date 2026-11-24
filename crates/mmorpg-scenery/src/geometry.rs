//! Integer XZ geometry and hashing. Only integer arithmetic participates, so
//! every platform derives the same scenery bit for bit.

/// Closed rectangle in XZ units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub min: [i32; 2],
    pub max: [i32; 2],
}

impl Rect {
    #[must_use]
    pub const fn around(centre: [i32; 2], half: [i32; 2]) -> Self {
        Self {
            min: [centre[0] - half[0], centre[1] - half[1]],
            max: [centre[0] + half[0], centre[1] + half[1]],
        }
    }

    #[must_use]
    pub const fn expanded(self, margin: i32) -> Self {
        Self {
            min: [self.min[0] - margin, self.min[1] - margin],
            max: [self.max[0] + margin, self.max[1] + margin],
        }
    }

    /// Strict overlap: rectangles that only touch do not intersect.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.min[0] < other.max[0]
            && other.min[0] < self.max[0]
            && self.min[1] < other.max[1]
            && other.min[1] < self.max[1]
    }

    /// Distance from a point to this rectangle, zero inside.
    #[must_use]
    pub fn distance(self, point: [i32; 2]) -> i64 {
        let dx = i64::from((self.min[0] - point[0]).max(point[0] - self.max[0]).max(0));
        let dz = i64::from((self.min[1] - point[1]).max(point[1] - self.max[1]).max(0));
        isqrt(dx * dx + dz * dz)
    }
}

/// Distance from `point` to the segment `a–b`, rounded down to whole units.
#[must_use]
pub fn segment_distance(point: [i32; 2], a: [i32; 2], b: [i32; 2]) -> i64 {
    root(segment_squared(point, a, b))
}

/// The squared distance [`segment_distance`] takes the root of, rounded down to whole units².
fn segment_squared(point: [i32; 2], a: [i32; 2], b: [i32; 2]) -> u128 {
    let direction = [
        i64::from(b[0]) - i64::from(a[0]),
        i64::from(b[1]) - i64::from(a[1]),
    ];
    let offset = [
        i64::from(point[0]) - i64::from(a[0]),
        i64::from(point[1]) - i64::from(a[1]),
    ];
    let length_squared = direction[0] * direction[0] + direction[1] * direction[1];
    let along = offset[0] * direction[0] + offset[1] * direction[1];
    if length_squared == 0 || along <= 0 {
        return (offset[0] * offset[0] + offset[1] * offset[1])
            .unsigned_abs()
            .into();
    }
    if along >= length_squared {
        let dx = i64::from(point[0]) - i64::from(b[0]);
        let dz = i64::from(point[1]) - i64::from(b[1]);
        return (dx * dx + dz * dz).unsigned_abs().into();
    }
    let cross = i128::from(direction[0]) * i128::from(offset[1])
        - i128::from(direction[1]) * i128::from(offset[0]);
    (cross * cross / i128::from(length_squared)).unsigned_abs()
}

/// The root of a squared distance, rounded down to whole units.
fn root(squared: u128) -> i64 {
    match u64::try_from(squared) {
        Ok(narrow) => i64::try_from(narrow.isqrt()),
        Err(_) => i64::try_from(squared.isqrt()),
    }
    .unwrap_or(i64::MAX)
}

/// Distance from `point` to a polyline: the least of its segments' distances. The root rises
/// with its square, so it is taken once, of the least square (#855: the roots were most of the
/// scenery's cost in WebAssembly).
#[must_use]
pub fn polyline_distance(point: [i32; 2], points: &[[i32; 2]]) -> i64 {
    points
        .windows(2)
        .map(|segment| segment_squared(point, segment[0], segment[1]))
        .min()
        .map_or(i64::MAX, root)
}

/// Axis-aligned ellipse in XZ units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ellipse {
    pub centre: [i32; 2],
    pub radii: [i32; 2],
}

impl Ellipse {
    #[must_use]
    pub fn contains(self, point: [i32; 2]) -> bool {
        self.scaled_norm(point) <= self.scale()
    }

    /// Approximate distance outside the ellipse, zero inside. Exact for circles.
    #[must_use]
    pub fn distance(self, point: [i32; 2]) -> i64 {
        let norm = self.scaled_norm(point);
        let scale = self.scale();
        if norm <= scale {
            return 0;
        }
        let largest = i128::from(self.radii[0].max(self.radii[1]));
        i64::try_from((norm - scale) / largest).unwrap_or(i64::MAX)
    }

    /// [`Self::distance`] where it is below `limit`, and `limit` elsewhere: a point clearly that
    /// far away is decided in floating point, without the exact integer root (#855).
    #[must_use]
    pub fn distance_within(self, point: [i32; 2], limit: i64) -> i64 {
        let dx = f64::from(point[0]) - f64::from(self.centre[0]);
        let dz = f64::from(point[1]) - f64::from(self.centre[1]);
        let [rx, rz] = self.radii.map(f64::from);
        let estimate = ((dx * dx * rz * rz + dz * dz * rx * rx).sqrt() - rx * rz) / rx.max(rz);
        // The exact distance rounds its root and its quotient down: at most two units less.
        if estimate >= limit as f64 + 2. {
            return limit;
        }
        self.distance(point).min(limit)
    }

    #[must_use]
    pub const fn bounds(self) -> Rect {
        Rect::around(self.centre, self.radii)
    }

    /// `sqrt(dx² rz² + dz² rx²)`, which equals `rx · rz` on the boundary.
    fn scaled_norm(self, point: [i32; 2]) -> i128 {
        let dx = i128::from(point[0]) - i128::from(self.centre[0]);
        let dz = i128::from(point[1]) - i128::from(self.centre[1]);
        let rx = i128::from(self.radii[0]);
        let rz = i128::from(self.radii[1]);
        let squared = dx * dx * rz * rz + dz * dz * rx * rx;
        i128::try_from(squared.unsigned_abs().isqrt()).unwrap_or(i128::MAX)
    }

    fn scale(self) -> i128 {
        i128::from(self.radii[0]) * i128::from(self.radii[1])
    }
}

#[must_use]
pub fn isqrt(value: i64) -> i64 {
    i64::try_from(value.unsigned_abs().isqrt()).unwrap_or(i64::MAX)
}

/// SplitMix64: a fixed integer hash, identical on every platform.
#[must_use]
pub const fn splitmix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Hash of a seed and an integer lattice point.
#[must_use]
pub const fn lattice_hash(seed: u64, x: i32, z: i32) -> u64 {
    splitmix64(seed ^ (((x as u32 as u64) << 32) | z as u32 as u64))
}

/// A seeded counter-based stream of integers.
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(1);
        splitmix64(self.0)
    }

    /// Uniform-enough integer in `low..high`; `high` must exceed `low`.
    pub fn range(&mut self, low: i32, high: i32) -> i32 {
        let span = u64::from(high.abs_diff(low)).max(1);
        let offset = self.next_u64() % span;
        low.saturating_add(i32::try_from(offset).unwrap_or(0))
    }
}

/// Value noise in `[-1024, 1024]`: hashed lattice values, smoothstep-blended
/// in Q10 fixed point over `cell`-unit squares.
#[must_use]
pub fn value_noise(seed: u64, x: i32, z: i32, cell: i32) -> i64 {
    let cell_x = x.div_euclid(cell);
    let cell_z = z.div_euclid(cell);
    let fraction = |value: i32| i64::from(value.rem_euclid(cell)) * 1024 / i64::from(cell);
    let smooth = |t: i64| t * t * (3 * 1024 - 2 * t) / (1024 * 1024);
    let (sx, sz) = (smooth(fraction(x)), smooth(fraction(z)));
    let lattice = |dx: i32, dz: i32| {
        let hash = lattice_hash(seed, cell_x.wrapping_add(dx), cell_z.wrapping_add(dz));
        i64::try_from(hash % 2049).unwrap_or(0) - 1024
    };
    let lerp = |a: i64, b: i64, t: i64| a + (b - a) * t / 1024;
    let near = lerp(lattice(0, 0), lattice(1, 0), sx);
    let far = lerp(lattice(0, 1), lattice(1, 1), sx);
    lerp(near, far, sz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_match_simple_cases() {
        let rect = Rect {
            min: [0, 0],
            max: [100, 50],
        };
        assert_eq!(rect.distance([50, 25]), 0);
        assert_eq!(rect.distance([130, 90]), 50);
        assert_eq!(segment_distance([50, 30], [0, 0], [100, 0]), 30);
        assert_eq!(segment_distance([-30, 40], [0, 0], [100, 0]), 50);
        assert_eq!(segment_distance([140, -30], [0, 0], [100, 0]), 50);
        let circle = Ellipse {
            centre: [0, 0],
            radii: [100, 100],
        };
        assert!(circle.contains([60, 80]));
        assert_eq!(circle.distance([0, 150]), 50);
        assert_eq!(circle.distance([30, 40]), 0);
    }

    /// Within its limit the floating-point shortcut answers exactly [`Ellipse::distance`], and
    /// never less than the limit beyond it.
    #[test]
    fn distance_within_a_limit_is_the_distance_below_it() {
        let ellipse = Ellipse {
            centre: [1_234, -5_678],
            radii: [42_000, 9_100],
        };
        for x in (-60_000..60_000).step_by(397) {
            for z in (-30_000..20_000).step_by(211) {
                let point = [x, z];
                let exact = ellipse.distance(point);
                assert_eq!(
                    ellipse.distance_within(point, 1_200),
                    exact.min(1_200),
                    "{point:?}"
                );
            }
        }
    }

    #[test]
    fn value_noise_is_bounded_continuous_and_seeded() {
        let mut previous = value_noise(7, -5_000, 1_234, 900);
        for x in -5_000..5_000 {
            let value = value_noise(7, x, 1_234, 900);
            assert!((-1024..=1024).contains(&value));
            if x > -5_000 {
                assert!((value - previous).abs() <= 8, "step at {x}");
            }
            previous = value;
        }
        assert_ne!(value_noise(7, 450, 450, 900), value_noise(8, 450, 450, 900));
    }
}
