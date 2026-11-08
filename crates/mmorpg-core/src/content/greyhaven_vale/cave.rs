//! The Greyfang cave (#159): an enclosed rock chamber set into the Greyfang Ridge hillside,
//! east of Redbrand Hollow. A 4 m mouth in its north wall opens on the flat foot of the ridge;
//! inside, an outer cavern joins a smaller inner chamber through a 4 m passage.
//!
//! [`Cave`] describes the terrain footprint. #160 makes it
//! Redbrand Hollow's shared outer cavern: Redbrands hold both chambers, and a sealed
//! [`THRESHOLD`] in the inner chamber is the barred way to the party-private Redbrand Depths
//! (#404): a player touching its [`depths_threshold`] strip is transferred or refused. Collision is persisted separately in `content/collision/redbrand.json`:
//! simplified wall/roof/floor triangle meshes and a sloping approach to an elevated shelf.
//! This footprint metadata only flattens the surrounding terrain and controls scenery relief.
//! The cave floor meets the basin's rim at the mouth. Clients cut the roof away indoors.

use crate::StaticCollider;

/// A cave footprint: `(min, max)` XZ footprints in units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cave {
    /// The floor's height.
    pub floor_y: i32,
    /// Flattened terrain footprints for chambers, the passage and the entrance apron.
    pub floors: &'static [([i32; 2], [i32; 2])],
    /// The roof slab's footprint: every floor and wall.
    pub roof: ([i32; 2], [i32; 2]),
}

impl Cave {
    /// Whether the terrain is flattened at `(x, z)`: on or inside a floor rect.
    #[must_use]
    pub fn flattens(&self, x: i32, z: i32) -> bool {
        self.floors
            .iter()
            .any(|(min, max)| (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z))
    }
}

/// The sealed threshold to Redbrand Hollow's deeper cave (#160): a 4 m wide, 3.5 m tall
/// barricade against the inner chamber's east wall, as a `(min, max)` XZ footprint standing on
/// the cave's floor. It stays barred; the depths are reached by standing in its strip
/// ([`depths_threshold`]). A stand-in box until the asset-tooling kit gives it its own look.
pub const THRESHOLD: ([i32; 2], [i32; 2]) = ([7_930, -14_000], [7_990, -13_600]);
/// The threshold's height.
pub const THRESHOLD_HEIGHT: i32 = 350;

/// The sealed threshold as a collider with `id`.
#[must_use]
pub fn threshold(id: u32) -> StaticCollider {
    let (min, max) = THRESHOLD;
    let half = [
        (max[0] - min[0]) / 2,
        THRESHOLD_HEIGHT / 2,
        (max[1] - min[1]) / 2,
    ];
    StaticCollider {
        id,
        position: [
            min[0] + half[0],
            GREYFANG.floor_y + half[1],
            min[1] + half[2],
        ],
        half_extents: half,
    }
}

/// The public rooms beneath an irregular rock mound, entered through its north side.
#[rustfmt::skip]
pub const GREYFANG: Cave = Cave {
    floor_y: super::basin::TERRACE_Y,
    floors: &[
        // The outer cavern.
        ([4_800, -14_400], [6_800, -13_200]),
        // The passage to the inner chamber.
        ([6_800, -14_000], [7_200, -13_600]),
        // The inner chamber.
        ([7_200, -14_400], [8_000, -13_200]),
        // The tapered north mouth, reached around the far side of the ridge.
        ([5_800, -15_600], [7_000, -14_400]),
    ],
    roof: ([3_600, -15_600], [9_200, -12_000]),
};

/// A depths exit lands in the public cave's north mouth, this far from where it crossed the
/// depths' exit strip (#404): the strip at x ∈ [-10000, -9800), z ∈ [-150, 150) maps onto the
/// mouth's clear floor, and the character keeps its offset across the strip like at a seam.
pub const EXIT_OFFSET: [i32; 2] = [16_300, -15_000];

/// The body centre where a character returning from the depths stands in Greyhaven: on the
/// public cave's mouth floor, away from its garrison. Also where a character is placed if its
/// stored location is a retired run (#404, Atlas 5.1.30).
#[must_use]
pub const fn depths_exit_entrance() -> [i32; 3] {
    [
        6_400,
        GREYFANG.floor_y + crate::PLAYER_HALF_EXTENTS_UNITS[1],
        -15_000,
    ]
}

/// The strip in front of the barricade where a living character asks to enter the depths, and
/// the inner chamber a party gathers in (#404).
#[must_use]
pub const fn depths_threshold() -> crate::DepthsThreshold {
    crate::DepthsThreshold {
        strip: crate::XzBounds {
            min: [7_700, THRESHOLD.0[1]],
            max: [THRESHOLD.0[0], THRESHOLD.1[1]],
        },
        gather: crate::XzBounds {
            min: [7_200, -14_400],
            max: [8_000, -13_200],
        },
        qualifying_quest: super::quests::GARRICK_REDBRAND_QUEST,
        entrance: depths_exit_entrance(),
        facing: 49_152,
    }
}

/// Repository-persisted simplified geometry, exported from Blender in game coordinates.
#[must_use]
pub fn collision_meshes() -> Vec<crate::StaticMesh> {
    serde_json::from_str(include_str!("../../../content/collision/redbrand.json"))
        .expect("committed Redbrand collision JSON is valid")
}

/// The north-side approach sits on a broad terrace blended into the surrounding hills.
/// Keeping this with core terrain prevents a visual path floating above a descending grid.
#[must_use]
pub fn approach_ground(x: i32, z: i32, base: i32) -> i32 {
    let dx = i64::from(x - 6400) * 1024 / 4200;
    let dz = i64::from(z + 13800) * 1024 / 3700;
    let r = (dx * dx + dz * dz).isqrt();
    let weight = ((1400 - r) * 1024 / 376).clamp(0, 1024);
    base.max((i64::from(GREYFANG.floor_y) * weight / 1024) as i32)
}
