//! Greyhaven's village basin (#423, Atlas 5.1.84): the Outpost stands on the valley floor in a
//! basin, and the rest of the inner valley is one rim 11 m above it, behind cliffs the
//! controllers cannot climb (a 4 m grid cell rising 11 m is 70°; they climb at most 45°).
//! Millbrook Farm (slice 1), Stillwater Lake with the Sunken Shrine (slice 2), and Redbrand
//! Hollow with the Greyfang cave and Wolfrun Woods (slice 3) all stand on the rim. Beyond 128 m
//! from the Outpost the rim descends gently (17°) into the outer valley's hills (#159).
//!
//! The basin is an irregular, rounded apron with a northern lobe west of the lake.
//! Fixed-seed weathering varies its rock faces and shoulders (#513). Six ways between the floor and the rim are cut into the terrain itself:
//!
//! - **Millbrook Climb**, the Millbrook Road: east from the Outpost's gate, north along a ledge
//!   in the cliff face, round a hairpin landing and back south up a cutting onto the farm
//!   (about 1:5, 11°).
//! - **Millbrook Steps**, a steeper footpath (about 1:3.6, 15°): south from the farmyard down a
//!   second ledge to the valley floor, a return that does not use the road.
//! - **Lakeshore Climb**, the Lakeshore Path (about 1:6, then 1:4.5): from the Hollow Road
//!   north along a ledge in the lake's west cliff, round a landing and back south up a cutting
//!   onto the lake's west shore.
//! - **Wolfrun Climb**, the Wolfrun Trail: the Millbrook Climb's mirror image west of the
//!   Outpost, up into Wolfrun Woods.
//! - **Hollow Climb**, the Hollow Road: from the notch west along a ledge under the Hollow's
//!   cliff, round a landing and back east up a cutting to the Hollow's camp.
//! - **South Climb**, the South Road: the Hollow Climb's mirror image south of the Outpost, up
//!   toward Southmeadow Downs.
//!
//! On top, the Stillwater Rim Trail joins the farm and the lake and the Wolfrun Ridge Path joins
//! the woods and the Hollow, so each terrace has a second way home. Every height here is
//! sampled on the terrain grid's 4 m lattice, so each lane is exactly two samples wide and flat
//! across.

/// The rim's height above the valley floor (11 m).
pub const TERRACE_Y: i32 = 1_100;

/// The rim descends from the first to the second Chebyshev distance from the Outpost (17°),
/// into the outer valley's hills, beyond the Sunken Shrine's cave.
pub const OUTER_SLOPE: [i32; 2] = [12_800, 16_400];

/// The axis along which a [`Lane`]'s height varies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Along {
    X,
    Z,
}

/// A lane of the terrain lattice: samples in `[min, max]` (inclusive) take a height that
/// varies linearly along one axis, from `heights[0]` at `min` to `heights[1]` at `max`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lane {
    pub min: [i32; 2],
    pub max: [i32; 2],
    pub heights: [i32; 2],
    pub along: Along,
}

impl Lane {
    #[must_use]
    pub const fn contains(self, x: i32, z: i32) -> bool {
        self.min[0] <= x && x <= self.max[0] && self.min[1] <= z && z <= self.max[1]
    }

    pub(crate) fn height(self, x: i32, z: i32) -> i32 {
        let axis = match self.along {
            Along::X => 0,
            Along::Z => 1,
        };
        let value = [x, z][axis];
        let [low, high] = self.heights.map(i64::from);
        let (start, span) = (
            i64::from(self.min[axis]),
            i64::from(self.max[axis] - self.min[axis]),
        );
        let height = low + (high - low) * (i64::from(value) - start) / span.max(1);
        i32::try_from(height).unwrap_or(0)
    }
}

/// A flat lane at `height`.
const fn level(min: [i32; 2], max: [i32; 2], height: i32) -> Lane {
    Lane {
        min,
        max,
        heights: [height, height],
        along: Along::Z,
    }
}

/// Height of the Millbrook, Wolfrun, Hollow and South Climbs' hairpin landings (6.4 m).
pub const LANDING_Y: i32 = 640;

/// The Millbrook Climb: the cliff ledge, the hairpin landing and the cutting, in that order.
pub const CLIMB: [Lane; 3] = [
    Lane {
        min: [4_000, -1_600],
        max: [4_400, 2_000],
        heights: [LANDING_Y, 0],
        along: Along::Z,
    },
    level([4_000, -2_000], [5_600, -1_600], LANDING_Y),
    Lane {
        min: [5_200, -1_600],
        max: [5_600, 1_200],
        heights: [LANDING_Y, TERRACE_Y],
        along: Along::Z,
    },
];

/// The Millbrook Steps: the farmyard landing and the ledge down to the valley floor.
pub const STEPS: [Lane; 2] = [
    level([4_000, 3_200], [4_400, 3_600], TERRACE_Y),
    Lane {
        min: [4_000, 3_600],
        max: [4_400, 7_600],
        heights: [TERRACE_Y, 0],
        along: Along::Z,
    },
];

/// Height of the Lakeshore Climb's landing (5.6 m).
pub const LAKE_LANDING_Y: i32 = 560;

/// The Lakeshore Climb: the west cliff's ledge, the landing and the cutting, in that order.
pub const LAKESHORE: [Lane; 3] = [
    Lane {
        min: [1_200, -6_000],
        max: [1_600, -2_800],
        heights: [LAKE_LANDING_Y, 0],
        along: Along::Z,
    },
    level([1_200, -6_400], [2_800, -6_000], LAKE_LANDING_Y),
    Lane {
        min: [2_400, -6_000],
        max: [2_800, -3_600],
        heights: [LAKE_LANDING_Y, TERRACE_Y],
        along: Along::Z,
    },
];

/// The Wolfrun Climb, the Millbrook Climb mirrored: the ledge north under the woods' cliff, the
/// landing and the cutting back south into the woods.
pub const WOLFRUN: [Lane; 3] = [
    Lane {
        min: [-4_400, -1_600],
        max: [-4_000, 2_000],
        heights: [LANDING_Y, 0],
        along: Along::Z,
    },
    level([-5_600, -2_000], [-4_000, -1_600], LANDING_Y),
    Lane {
        min: [-5_600, -1_600],
        max: [-5_200, 1_200],
        heights: [LANDING_Y, TERRACE_Y],
        along: Along::Z,
    },
];

/// The Hollow Climb: the ledge west under the Hollow's cliff, the landing, and the cutting back
/// east into the rim beside it (the bank between them is too steep to walk but by the landing).
pub const HOLLOW: [Lane; 3] = [
    Lane {
        min: [-3_200, -6_400],
        max: [0, -6_000],
        heights: [LANDING_Y, 0],
        along: Along::X,
    },
    level([-4_000, -7_200], [-3_200, -6_000], LANDING_Y),
    Lane {
        min: [-3_200, -7_200],
        max: [0, -6_800],
        heights: [LANDING_Y, TERRACE_Y],
        along: Along::X,
    },
];

/// The South Climb, the Hollow Climb mirrored across the basin: the ledge west above the
/// southern cliff's foot, the landing and the cutting back east onto the rim.
pub const SOUTH: [Lane; 3] = [
    Lane {
        min: [-3_200, 9_200],
        max: [0, 9_600],
        heights: [LANDING_Y, 0],
        along: Along::X,
    },
    level([-4_000, 9_200], [-3_200, 10_400], LANDING_Y),
    Lane {
        min: [-3_200, 10_000],
        max: [0, 10_400],
        heights: [LANDING_Y, TERRACE_Y],
        along: Along::X,
    },
];

/// Every lane, in the order they override one another.
fn lanes() -> impl Iterator<Item = &'static Lane> {
    CLIMB
        .iter()
        .chain(&STEPS)
        .chain(&LAKESHORE)
        .chain(&WOLFRUN)
        .chain(&HOLLOW)
        .chain(&SOUTH)
}

/// Linear rise from 0 at `edge[0]` to 1024 at `edge[1]` (Q10).
fn rise_q10(value: i32, edge: [i32; 2]) -> i64 {
    let (value, [start, end]) = (i64::from(value), edge.map(i64::from));
    ((value - start) * 1_024 / (end - start)).clamp(0, 1_024)
}

/// How much of the valley floor shows at `(x, z)` (Q10): 1024 inside the basin, 0 on the rim.
fn floor_q10(x: i32, z: i32) -> i64 {
    // Keep foundations, gates and the village's working lanes on their existing floor.
    if (-3_400..=3_400).contains(&x) && (-1_200..=5_400).contains(&z) {
        return 1_024;
    }
    // Broad, fixed-seed erosion changes the silhouette without changing it on recovery.
    // Fade it out at the waterfall so the spring still meets its plunge pool.
    let weathering = super::value_noise_q10(i64::from(x), i64::from(z), 2_600, 513);
    let strength = i64::from((z - 2_400).abs().min(1_600));
    let radius = 4_530 + (weathering - 512) * strength / 1_820;
    let shoulder = 1_110 + weathering * strength / 16_384;
    let apron = ellipse_floor(x, z - 2_800, radius, 6_800, shoulder);
    let notch = ellipse_floor(x + 1_400, z + 4_000, 3_100, 2_700, 1_160);
    apron.max(notch)
}

/// Elliptical rock face with a weathered shoulder, evaluated entirely in integers.
fn ellipse_floor(x: i32, z: i32, radius_x: i64, radius_z: i64, shoulder: i64) -> i64 {
    let nx = i64::from(x) * 1_024 / radius_x;
    let nz = i64::from(z) * 1_024 / radius_z;
    let distance = (nx * nx + nz * nz).isqrt();
    let rise = ((distance - 1_024) * 1_024 / (shoulder - 1_024)).clamp(0, 1_024);
    // A shallow foot and shoulder frame a steeper middle rock face.
    let rock = if rise < 256 {
        rise / 2
    } else if rise < 768 {
        128 + (rise - 256) * 3 / 2
    } else {
        896 + (rise - 768) / 2
    };
    1_024 - rock
}

/// Broken ridge crests and gullies instead of one uniformly level cliff lip. Existing
/// authored routes, lake/shrine terrain and the farm apron keep their established levels.
fn ridge(x: i32, z: i32, terrace: i32) -> i32 {
    if terrace == 0
        || x.abs() > 11000
        || !(-11000..=12000).contains(&z)
        || (3000..=8200).contains(&x) && (-7600..=-3200).contains(&z)
        || (4800..=10000).contains(&x) && (1200..=9000).contains(&z)
    {
        return 0;
    }
    let radius = |dx: i32, dz: i32, rx: i64, rz: i64| {
        let a = i64::from(dx) * 1024 / rx;
        let b = i64::from(dz) * 1024 / rz;
        (a * a + b * b).isqrt()
    };
    let r = radius(x, z - 2800, 4530, 6800).min(radius(x + 1400, z + 4000, 3100, 2700));
    let belt = (250 - (r - 1170).abs()).max(0);
    if belt == 0 {
        return 0;
    }
    let probe = super::StaticCollider {
        id: 0,
        position: [x, 0, z],
        half_extents: [400, 1, 400],
    };
    if !super::clear_of_roads(&probe) {
        return 0;
    }
    let ribs = super::value_noise_q10(i64::from(x), i64::from(z), 900, 0x513c1);
    let strata = super::value_noise_q10(i64::from(x), i64::from(z), 2100, 0x513c2);
    ((120 + ribs * 420 / 1024 + strata * 180 / 1024) * belt * i64::from(terrace.min(TERRACE_Y))
        / (250 * i64::from(TERRACE_Y))) as i32
}

/// The rim's ground at `(x, z)`, or `None` on the untouched valley floor. Lanes override the
/// rim and the floor alike.
#[must_use]
pub fn height_at(x: i32, z: i32) -> Option<i32> {
    if let Some(lane) = lanes().find(|lane| lane.contains(x, z)) {
        return Some(lane.height(x, z));
    }
    let reach = i32::try_from(x.unsigned_abs().max(z.unsigned_abs())).unwrap_or(i32::MAX);
    let weight = (1_024 - floor_q10(x, z)).min(1_024 - rise_q10(reach, OUTER_SLOPE));
    let terrace = i32::try_from(i64::from(TERRACE_Y) * weight / 1_024).unwrap_or(0);
    // Rounded grassy banks and broken rim crests, outside foundations and route lanes.
    let mound = |cx: i32, cz: i32, radius: i32, high: i32| {
        let d = i64::from(x - cx).pow(2) + i64::from(z - cz).pow(2);
        let radius = i64::from(radius).pow(2);
        let falloff = (radius - d).max(0) * 1_024 / radius;
        (i64::from(high) * falloff * falloff / (1_024 * 1_024)) as i32
    };
    let relief = if x.abs() > 3_500 || z > 5_800 {
        mound(-4_100, 4_600, 1_000, 120)
            + mound(2_300, 7_000, 1_100, 110)
            + mound(-2_400, 7_800, 900, 95)
            + mound(-5_200, 4_000, 1_100, 240)
            + mound(5_000, 6_500, 1_200, 190)
    } else {
        0
    };
    let height = terrace + relief + ridge(x, z, terrace);
    (height > 0).then_some(height)
}
