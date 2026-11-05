//! Greyhaven's shallow spring brook (#513): cliff cascade, village channel, crossing and
//! low drainage culverts. Immutable content shared by physics and both presentations.
use crate::{Ramp, RampRise, StaticCollider, WaterChannel, WaterVolume};

pub const SURFACE: i32 = -5;
pub const HALF_WIDTH: i32 = 90;
pub const COURSE: &[[i32; 2]] = &[
    [-4400, 2400],
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
    [300, 4200],
    [-300, 4500],
    [-500, 5000],
    [-800, 6500],
    [-800, 9600],
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
pub fn waters() -> [WaterVolume; 2] {
    [
        WaterVolume {
            name: "Greyhaven Brook".to_owned(),
            centre: [-2050, 5400],
            radii: [2500, 4300],
            surface: SURFACE,
            air_pockets: vec![],
            channel: Some(channel()),
        },
        WaterVolume {
            name: "Cliffside Spring".to_owned(),
            centre: [-5000, 2400],
            radii: [200, 150],
            surface: 1110,
            air_pockets: vec![],
            channel: None,
        },
    ]
}

/// A soft bank cut sampled onto the existing authoritative terrain grid. The whole spawn
/// lattice and its interpolation neighbours stay level; there is no client-only river bed.
#[must_use]
pub fn bed_depth(x: i32, z: i32) -> i32 {
    if (-2000..=2000).contains(&x) && (800..=2800).contains(&z) {
        return 0;
    }
    let squared = COURSE
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
    let distance = (squared as u128).isqrt().min(350) as i32;
    35 * (350 - distance) / 350
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
            position: [-1000, 60, 9580],
            half_extents: [50, 90, 80],
        },
        StaticCollider {
            id: DRAIN_STONES[1],
            position: [-600, 60, 9580],
            half_extents: [50, 90, 80],
        },
        StaticCollider {
            id: DRAIN_STONES[2],
            position: [-800, 125, 9580],
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
