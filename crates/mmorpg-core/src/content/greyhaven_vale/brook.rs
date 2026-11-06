//! Stillwater's connected inflow, rim outlet and Greyhaven brook (#513): cascade, crossing and
//! low drainage culverts. Immutable content shared by physics and both presentations.
use crate::{Ramp, RampRise, StaticCollider, WaterChannel, WaterVolume};

pub const SURFACE: i32 = -5;
pub const HALF_WIDTH: i32 = 90;
/// Shallow village bed, with clearance for the terrain grid beside the protected spawn plaza.
pub const BED_DEPTH: i32 = 35;

/// Upper water shares Stillwater's level and has an explicit direction through the lake.
pub const UPPER_SURFACE: i32 = super::lake::SURFACE;
pub const UPPER_HALF_WIDTH: i32 = 130;

/// Authored control points, shared by terrain, authoritative water and client scenery.
pub fn village() -> &'static [[i32; 2]] {
    &super::landscape::data().watercourses.village
}
pub fn inflow() -> &'static [[i32; 2]] {
    &super::landscape::data().watercourses.inflow
}
pub fn outlet() -> &'static [[i32; 2]] {
    &super::landscape::data().watercourses.outlet
}

pub const BRIDGE: u32 = 680;
pub const RAILS: [u32; 2] = [681, 682];
pub const CULVERT_POST: u32 = 683;
pub const CULVERT_TOP: u32 = 684;
pub const DRAIN_STONES: [u32; 3] = [685, 686, 687];

#[must_use]
pub fn channel() -> WaterChannel {
    WaterChannel {
        points: village().to_vec(),
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
        volume("Greyhaven Brook", village(), HALF_WIDTH, SURFACE),
        volume(
            "Stillwater Inflow",
            inflow(),
            UPPER_HALF_WIDTH,
            UPPER_SURFACE,
        ),
        volume(
            "Stillwater Outlet",
            outlet(),
            UPPER_HALF_WIDTH,
            UPPER_SURFACE,
        ),
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
    bed_depth(x, z) > 0 || upper_distance(x, z) < 650 || super::lake::near_shore(x, z, 250)
}

fn upper_distance(x: i32, z: i32) -> i32 {
    if !(-14900..=-4100).contains(&x) || !(-10200..=3100).contains(&z) {
        return i32::MAX;
    }
    distance(inflow(), x, z).min(distance(outlet(), x, z))
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
    let distance = distance(village(), x, z).min(550);
    BED_DEPTH * (550 - distance) / 550
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
