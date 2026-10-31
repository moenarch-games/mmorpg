//! Rock collision hulls (#49): asset-tooling's authored convex proxies fitted into rock boxes.
//!
//! The `greyhaven-nature` bundle's three rocks each carry a `collisionProxy` convex hull in the
//! rock's own glTF frame (metres, +Y up, footprint-base-centre origin). [`PROXIES`]
//! transcribes those hulls and the rocks' bounds in units. A rock collider keeps its box (the
//! zone's footprint, interest and placement rules read boxes) and Rapier collides with the
//! proxy fitted into it: the rock's bounds map onto the box, turned by whole quarter turns.
//! Clients draw the same rock with the same fit, so the drawn rock and its collision agree.

use crate::{ZoneError, physics::Vec3i};

/// One rock's authored convex collision proxy, in units in the rock's own frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RockProxy {
    /// The rock mesh's bounds.
    pub min: [i32; 3],
    pub max: [i32; 3],
    /// Hull vertices, on or just inside the rock.
    pub vertices: &'static [[i32; 3]],
}

/// The `greyhaven-nature` rocks' proxies: index `n` is `greyhaven.nature.rock.<n + 1>`.
pub const PROXIES: [RockProxy; 3] = [
    // `greyhaven.nature.rock.1` (stratified).
    RockProxy {
        min: [-160, 0, -150],
        max: [160, 220, 150],
        vertices: &[
            [-160, 5, 45],
            [-157, 1, 47],
            [-160, 72, 16],
            [-125, 109, 58],
            [-149, 79, 46],
            [-150, 1, -37],
            [-77, 214, -22],
            [-78, 216, -8],
            [-79, 193, 28],
            [-43, 220, 11],
            [-151, 5, -37],
            [-134, 84, -88],
            [-84, 1, -107],
            [-28, 80, -133],
            [-97, 197, -33],
            [-68, 161, -98],
            [-20, 199, -85],
            [21, 95, 122],
            [64, 2, 150],
            [66, 5, 147],
            [16, 126, 106],
            [18, 215, -10],
            [72, 183, -6],
            [54, 1, -150],
            [158, 90, -69],
            [152, 1, -89],
            [155, 0, -110],
            [160, 1, -107],
            [7, 200, -77],
            [45, 195, -66],
            [105, 174, -58],
            [154, 122, -53],
        ],
    },
    // `greyhaven.nature.rock.2` (boulder).
    RockProxy {
        min: [-150, 0, -125],
        max: [150, 190, 125],
        vertices: &[
            [-129, 1, 17],
            [-129, 9, 28],
            [-94, 79, 111],
            [-64, 3, 89],
            [-109, 139, 92],
            [-102, 165, 86],
            [-70, 146, 109],
            [-96, 187, 66],
            [-91, 189, 70],
            [-90, 183, 2],
            [-59, 188, 71],
            [-16, 158, 100],
            [-145, 1, -34],
            [-145, 2, -47],
            [-146, 17, -11],
            [-139, 50, -37],
            [-142, 1, -49],
            [-145, 20, -53],
            [-23, 1, -124],
            [-30, 11, -124],
            [-27, 21, -123],
            [-103, 104, -72],
            [-95, 172, -14],
            [-47, 167, -54],
            [11, 53, 125],
            [21, 1, 104],
            [13, 8, 110],
            [22, 10, 110],
            [121, 1, 17],
            [124, 1, 16],
            [129, 8, 10],
            [140, 91, 16],
            [147, 86, 9],
            [51, 171, 24],
            [50, 171, 28],
            [126, 2, -69],
            [126, 2, -67],
            [22, 166, -47],
            [34, 146, -69],
            [149, 99, 1],
            [146, 96, -36],
            [148, 107, -21],
            [149, 104, -25],
        ],
    },
    // `greyhaven.nature.rock.3` (slab).
    RockProxy {
        min: [-170, 0, -170],
        max: [170, 230, 170],
        vertices: &[
            [-170, 2, 82],
            [-169, 2, 91],
            [-152, 2, 161],
            [-148, 2, 170],
            [-148, 7, 170],
            [-123, 57, 158],
            [-21, 12, 143],
            [-80, 167, 81],
            [-31, 229, 21],
            [-76, 2, -135],
            [-57, 2, -151],
            [-32, 2, -157],
            [-36, 230, -13],
            [-6, 228, -133],
            [5, 223, -144],
            [16, 222, -143],
            [9, 186, -158],
            [149, 2, 91],
            [151, 9, 90],
            [154, 103, 67],
            [133, 176, 29],
            [106, 10, -159],
            [157, 60, -149],
            [152, 99, -149],
            [168, 2, -75],
            [162, 1, -138],
            [164, 60, -140],
            [166, 10, -137],
            [133, 160, -100],
        ],
    },
];

/// Which rock a collider is drawn and collides as: [`PROXIES`]`[proxy]`, turned by `quarters`
/// quarter turns about +Y (+Z toward +X).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RockShape {
    pub proxy: u8,
    pub quarters: u8,
}

impl RockShape {
    /// Whether it names an authored proxy.
    pub fn validate(self) -> Result<(), ZoneError> {
        if usize::from(self.proxy) >= PROXIES.len() || self.quarters > 3 {
            return Err(ZoneError::new("invalid rock shape"));
        }
        Ok(())
    }

    /// The bundle entry of its rock, `greyhaven.nature.rock.<n>`.
    #[must_use]
    pub const fn entry(self) -> &'static str {
        match self.proxy {
            0 => "greyhaven.nature.rock.1",
            1 => "greyhaven.nature.rock.2",
            _ => "greyhaven.nature.rock.3",
        }
    }

    /// The box's half extents before the turn, from the collider's (turned) half extents.
    #[must_use]
    pub const fn local_half_extents(self, half_extents: [i32; 3]) -> [i32; 3] {
        let [x, y, z] = half_extents;
        if self.quarters % 2 == 1 {
            [z, y, x]
        } else {
            [x, y, z]
        }
    }

    /// The proxy's hull points fitted into a box of (turned) `half_extents`, relative to the
    /// box centre, in units.
    #[must_use]
    pub fn hull(self, half_extents: Vec3i) -> Vec<[f32; 3]> {
        let proxy = &PROXIES[usize::from(self.proxy).min(PROXIES.len() - 1)];
        let half = self.local_half_extents([half_extents.x, half_extents.y, half_extents.z]);
        #[allow(clippy::cast_precision_loss)]
        let fit = |axis: usize, value: i32| {
            let centre = (proxy.min[axis] + proxy.max[axis]) as f32 / 2.0;
            let span = (proxy.max[axis] - proxy.min[axis]) as f32 / 2.0;
            (value as f32 - centre) / span * half[axis] as f32
        };
        proxy
            .vertices
            .iter()
            .map(|vertex| {
                let [x, y, z] = [0, 1, 2].map(|axis| fit(axis, vertex[axis]));
                match self.quarters % 4 {
                    0 => [x, y, z],
                    1 => [z, y, -x],
                    2 => [-x, y, -z],
                    _ => [-z, y, x],
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hulls_fill_their_box_after_any_turn() {
        for proxy in 0..3 {
            for quarters in 0..4 {
                let shape = RockShape { proxy, quarters };
                let half = Vec3i::new(80, 50, 30);
                let points = shape.hull(half);
                for (axis, extent) in [(0, 80.0), (1, 50.0), (2, 30.0)] {
                    let low = points.iter().map(|p| p[axis]).fold(f32::INFINITY, f32::min);
                    let high = points
                        .iter()
                        .map(|p| p[axis])
                        .fold(f32::NEG_INFINITY, f32::max);
                    assert!(
                        low >= -extent - 0.01 && high <= extent + 0.01,
                        "{shape:?} {axis}"
                    );
                    assert!(high - low > extent, "{shape:?} spans axis {axis}");
                }
            }
        }
    }
}
