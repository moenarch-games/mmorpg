//! Stillwater's connected inflow, rim outlet and Greyhaven brook (#513): cascade, crossing and
//! low drainage culverts. Immutable content shared by physics and both presentations.
use crate::{Ramp, RampRise, StaticCollider, WaterChannel, WaterVolume};

pub const SURFACE: i32 = -5;
pub const HALF_WIDTH: i32 = 90;
pub const COURSE: &[[i32; 2]] = &[
    [-4200, 2400],
    [-4000, 2200],
    [-3400, 2000],
    [-3100, 2000],
    [-2800, 2450],
    [-2200, 2750],
    [-1700, 3100],
    [-1000, 3200],
    [-600, 3250],
    [-200, 3100],
    [300, 3200],
    [340, 3500],
    [260, 3850],
    [300, 4200],
    [50, 4350],
    [-300, 4500],
    [-500, 5000],
    [-650, 5450],
    [-660, 5850],
    [-800, 6500],
    [-920, 7050],
    [-760, 7600],
    [-880, 8200],
    [-800, 8800],
];
/// Upper water shares Stillwater's level and has an explicit direction through the lake.
pub const UPPER_SURFACE: i32 = super::lake::SURFACE;
pub const UPPER_HALF_WIDTH: i32 = 130;
pub const INFLOW: &[[i32; 2]] = &[
    [11000, -9500],
    [10200, -8800],
    [9600, -8200],
    [9000, -7000],
    [8000, -6000],
    [7200, -5200],
];
pub const OUTLET: &[[i32; 2]] = &[
    [4800, -6600],
    [4200, -7400],
    [4000, -7900],
    [3900, -8500],
    [3800, -9600],
    [3600, -10700],
    [3600, -11800],
    [3200, -12500],
    [2100, -12800],
    [900, -12600],
    [-200, -12800],
    [-1400, -12600],
    [-2600, -12800],
    [-3800, -12300],
    [-4600, -11800],
    [-5800, -9600],
    [-6400, -7600],
    [-6600, -5000],
    [-6600, -3000],
    [-6500, -1400],
    [-6300, 200],
    [-6000, 1500],
    [-5600, 2200],
    [-5000, 2400],
    [-4800, 2400],
];
pub const BRIDGE: u32 = 680;
pub const RAILS: [u32; 2] = [681, 682];
pub const CULVERT_POST: u32 = 683;
pub const CULVERT_TOP: u32 = 684;
pub const DRAIN_STONES: [u32; 3] = [685, 686, 687];

#[must_use]
pub fn channel() -> WaterChannel {
    WaterChannel {
        points: COURSE.to_vec(),
        half_width: HALF_WIDTH,
    }
}

#[must_use]
pub fn waters() -> [WaterVolume; 3] {
    let volume = |name: &str, points: &[[i32; 2]], width, surface| WaterVolume {
        name: name.to_owned(),
        centre: points[0],
        radii: [width, width],
        surface,
        air_pockets: vec![],
        channel: Some(WaterChannel {
            points: points.to_vec(),
            half_width: width,
        }),
    };
    [
        volume("Greyhaven Brook", COURSE, HALF_WIDTH, SURFACE),
        volume("Stillwater Inflow", INFLOW, UPPER_HALF_WIDTH, UPPER_SURFACE),
        volume("Stillwater Outlet", OUTLET, UPPER_HALF_WIDTH, UPPER_SURFACE),
    ]
}

/// Centimetres to the closest authored centreline, using the same segment metric as water.
fn distance(points: &[[i32; 2]], x: i32, z: i32) -> i32 {
    let squared = points
        .windows(2)
        .map(|p| {
            let [ax, az] = p[0].map(i128::from);
            let [bx, bz] = p[1].map(i128::from);
            let (dx, dz) = (bx - ax, bz - az);
            let (px, pz) = (i128::from(x) - ax, i128::from(z) - az);
            let length = dx * dx + dz * dz;
            let along = px * dx + pz * dz;
            if along <= 0 {
                px * px + pz * pz
            } else if along >= length {
                (px - dx).pow(2) + (pz - dz).pow(2)
            } else {
                (px * dz - pz * dx).pow(2) / length
            }
        })
        .min()
        .unwrap_or(i128::MAX);
    (squared as u128).isqrt().min(i32::MAX as u128) as i32
}

/// Clear scenery relief and trunk placement from the authored water banks too.
#[must_use]
pub fn wet_ground(x: i32, z: i32) -> bool {
    bed_depth(x, z) > 0 || upper_distance(x, z) < 650
}

fn upper_distance(x: i32, z: i32) -> i32 {
    if !(-7300..=11700).contains(&x) || !(-13600..=3100).contains(&z) {
        return i32::MAX;
    }
    distance(INFLOW, x, z).min(distance(OUTLET, x, z))
}

/// Carve a connected upper river into the rim, leaving the deep lake basin untouched.
#[must_use]
pub fn upper_ground(x: i32, z: i32, base: i32) -> i32 {
    let d = upper_distance(x, z);
    if d >= 650 {
        return base;
    }
    let bed = UPPER_SURFACE - 90;
    let weight = i64::from(650 - d);
    let cut = bed + i32::try_from(i64::from(base - bed).max(0) * (650 - weight) / 650).unwrap_or(0);
    base.min(cut)
}

/// A soft bank cut sampled onto the existing authoritative terrain grid. The whole spawn
/// lattice and its interpolation neighbours stay level; there is no client-only river bed.
#[must_use]
pub fn bed_depth(x: i32, z: i32) -> i32 {
    if (-1600..=1600).contains(&x) && (1200..=2800).contains(&z) {
        return 0;
    }
    if !(-4800..=900).contains(&x) || !(1400..=9400).contains(&z) {
        return 0;
    }
    let distance = distance(COURSE, x, z).min(550);
    65 * (550 - distance) / 550
}

pub fn colliders() -> Vec<StaticCollider> {
    vec![
        StaticCollider {
            id: BRIDGE,
            position: [0, 6, 3200],
            half_extents: [150, 6, 200],
        },
        StaticCollider {
            id: RAILS[0],
            position: [-180, 65, 3200],
            half_extents: [15, 55, 230],
        },
        StaticCollider {
            id: RAILS[1],
            position: [180, 65, 3200],
            half_extents: [15, 55, 230],
        },
        // The south palisade's small water opening: at most 1.1 m clear over the bed,
        // so it drains without becoming another player-sized gate.
        StaticCollider {
            id: CULVERT_POST,
            position: [-325, 150, 5000],
            half_extents: [25, 150, 30],
        },
        StaticCollider {
            id: CULVERT_TOP,
            position: [-500, 188, 5000],
            half_extents: [150, 112, 30],
        },
        StaticCollider {
            id: DRAIN_STONES[0],
            position: [-1000, 60, 8780],
            half_extents: [50, 90, 80],
        },
        StaticCollider {
            id: DRAIN_STONES[1],
            position: [-600, 60, 8780],
            half_extents: [50, 90, 80],
        },
        StaticCollider {
            id: DRAIN_STONES[2],
            position: [-800, 125, 8780],
            half_extents: [150, 25, 80],
        },
    ]
}
pub fn ramps() -> [Ramp; 2] {
    [
        Ramp {
            body: 688,
            position: [0, 6, 2900],
            half_extents: [150, 6, 100],
            rises: RampRise::PosZ,
        },
        Ramp {
            body: 689,
            position: [0, 6, 3500],
            half_extents: [150, 6, 100],
            rises: RampRise::NegZ,
        },
    ]
}
