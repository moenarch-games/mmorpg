//! Greyhaven's solid dressing (#577): the signposts, barrels, carts, the Lakeshore dock and
//! the loose rocks of the Redbrand Hollow, the highlands and the wetlands.
//!
//! Each [`Piece`] is the one definition of both its collider and its visual: the zone collides
//! with [`Piece::collider`] and `mmorpg-scenery` draws the piece at that box, so nothing that
//! looks solid is walked through. Pieces turn by quarter turns only, so the axis-aligned box
//! fits the drawn body exactly. Foliage (grass, flowers, bushes, reeds, crops) stays
//! presentation only and walk-through.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use super::{clear_of_roads, highlands, ids, inside_walls, marsh, units, vale};
use crate::{StaticCollider, XzBounds};

/// What a solid piece of dressing is.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Lamp,
    Signpost,
    Barrel,
    Cart,
    /// The Lakeshore dock's plank deck, walked on.
    Dock,
    SmallRock,
    MediumRock,
}

impl Kind {
    /// The body's half extents at scale 1 000, before its turn.
    #[must_use]
    pub const fn half_extents(self) -> [i32; 3] {
        match self {
            Self::Lamp => [10, 160, 10],
            Self::Signpost => [8, 110, 8],
            Self::Barrel => [30, 45, 30],
            Self::Cart => [110, 60, 60],
            Self::Dock => [500, 15, 90],
            Self::SmallRock => [40, 25, 35],
            Self::MediumRock => [80, 50, 70],
        }
    }
}

/// One placed solid piece.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Piece {
    /// Its collider ID.
    pub id: u32,
    pub kind: Kind,
    /// Feet XZ in units.
    pub at: [i32; 2],
    /// Quarter turns about +Y (+Z toward +X).
    pub quarters: u8,
    /// Size relative to the kind's base size, in thousandths.
    pub scale_permille: u16,
}

impl Piece {
    /// The body's half extents before its turn.
    #[must_use]
    pub fn half_extents(&self) -> [i32; 3] {
        self.kind.half_extents().map(|value| {
            i32::try_from(i64::from(value) * i64::from(self.scale_permille) / 1_000)
                .unwrap_or(i32::MAX)
                .max(1)
        })
    }

    /// Rotation about +Y, 65 536 steps per turn.
    #[must_use]
    pub const fn yaw(&self) -> u16 {
        (self.quarters as u16 % 4) * crate::trig::YAW_QUARTER_TURN
    }

    /// The XZ half extents after its turn.
    fn turned(&self) -> [i32; 3] {
        let [x, y, z] = self.half_extents();
        if self.quarters.is_multiple_of(2) {
            [x, y, z]
        } else {
            [z, y, x]
        }
    }

    /// Where its feet rest: on level ground at its height, on a slope at its lowest corner,
    /// sunk 0.3 m so nothing floats. The dock rests on the shore under its middle, where its
    /// deck reaches out over the water.
    #[must_use]
    pub fn feet(&self) -> i32 {
        let [hx, _, hz] = self.turned();
        let [x, z] = self.at;
        match self.kind {
            Kind::Dock => super::TERRAIN.height_at(x, z),
            _ => super::footing([x - hx, z - hz], [x + hx, z + hz]),
        }
    }

    /// The box the zone collides with: the drawn body, from its feet up. The dock collides as
    /// its deck, 0.4 m above its feet like the drawn planks.
    #[must_use]
    pub fn collider(&self) -> StaticCollider {
        let [hx, hy, hz] = self.turned();
        let [x, z] = self.at;
        let (bottom, half_height) = match self.kind {
            Kind::Dock => (self.feet() + 30, 5),
            _ => (self.feet(), hy),
        };
        StaticCollider {
            id: self.id,
            position: [x, bottom + half_height, z],
            half_extents: [hx, half_height, hz],
        }
    }
}

/// Hand-placed hub, farm and lake dressing: `(kind, at, quarter turns)`. The Outpost's lamps
/// are its layout's ([`super::layout`]).
const FIXED: [(Kind, [i32; 2], u8); 10] = [
    (Kind::Signpost, [300, 5_300], 0),
    (Kind::Signpost, [-4_600, 2_300], 1),
    (Kind::Signpost, [300, -1_300], 2),
    (Kind::Barrel, [3_080, 300], 0),
    (Kind::Barrel, [3_080, 400], 0),
    (Kind::Barrel, [2_300, 600], 1),
    (Kind::Cart, [1_200, 800], 0),
    (Kind::Cart, [6_900, 3_000], 2),
    (Kind::Barrel, [7_300, 3_300], 0),
    // The dock runs east from the Lakeshore Path into the shallow water.
    (Kind::Dock, [3_800, -4_900], 0),
];

/// SplitMix64 sequence: a fixed integer stream, identical on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn range(&mut self, min: i32, max: i32) -> i32 {
        let span = u64::try_from(i64::from(max) - i64::from(min))
            .unwrap_or(1)
            .max(1);
        min + i32::try_from(self.next() % span).unwrap_or(0)
    }
}

/// Seed of the loose rocks.
const SEED_ROCKS: u64 = 0x524f_434b_5343_4154;

/// The loose rocks' scatters: `(bounds, target, attempts, every nth rock medium)`; zero keeps
/// them all small.
fn rock_fields() -> [(XzBounds, usize, usize, u64); 3] {
    let hollow = super::areas()
        .areas()
        .iter()
        .find(|area| area.id() == vale::REDBRAND_HOLLOW)
        .map(|area| XzBounds {
            min: area.min_xz(),
            max: area.max_xz().map(|value| value + 1),
        })
        .expect("the vale names the Redbrand Hollow");
    let highlands = highlands::BOUNDS;
    let wetlands = marsh::BOUNDS;
    [
        (hollow, 24, 1_000, 3),
        // The highlands' broken high ground (#425), north of the escarpment.
        (
            XzBounds {
                min: [highlands.min[0] + 100, highlands.min[1] + 100],
                max: [highlands.max[0] - 100, highlands::ESCARPMENT[3] - 100],
            },
            220,
            2_000,
            3,
        ),
        // The wetlands' lowland (#426), east of the ridge.
        (
            XzBounds {
                min: [marsh::RIDGE[3] + 100, wetlands.min[1] + 100],
                max: [wetlands.max[0] - 100, wetlands.max[1] - 100],
            },
            80,
            1_000,
            0,
        ),
    ]
}

/// Whether two closed XZ rectangles `[min, max]` overlap with `margin` between them.
const fn near(a: ([i32; 2], [i32; 2]), b: ([i32; 2], [i32; 2]), margin: i32) -> bool {
    a.0[0] - margin < b.1[0]
        && b.0[0] - margin < a.1[0]
        && a.0[1] - margin < b.1[1]
        && b.0[1] - margin < a.1[1]
}

fn rect(collider: &StaticCollider) -> ([i32; 2], [i32; 2]) {
    let [x, _, z] = collider.position;
    let [hx, _, hz] = collider.half_extents;
    ([x - hx, z - hz], [x + hx, z + hz])
}

/// The loose rocks, clear of `colliders` (the zone's other bodies), the roads, the water, the
/// walls and every creature's and NPC's post.
fn rocks(colliders: &[StaticCollider]) -> Vec<Piece> {
    // Grid buckets of 20 m over every other body's footprint; the ground and walls are left
    // to `inside_walls`.
    const BUCKET: i32 = 2_000;
    type Area = ([i32; 2], [i32; 2]);
    fn block(area: Area, buckets: &mut BTreeMap<(i32, i32), Vec<Area>>) {
        for bx in area.0[0].div_euclid(BUCKET) - 1..=area.1[0].div_euclid(BUCKET) + 1 {
            for bz in area.0[1].div_euclid(BUCKET) - 1..=area.1[1].div_euclid(BUCKET) + 1 {
                buckets.entry((bx, bz)).or_default().push(area);
            }
        }
    }
    let mut buckets = BTreeMap::new();
    for collider in colliders.iter().filter(|collider| {
        collider.id != ids::GROUND && !ids::BOUNDARY_WALLS.contains(&collider.id)
    }) {
        block(rect(collider), &mut buckets);
    }
    let posts: Vec<[i32; 2]> = units::creature_spawns()
        .iter()
        .map(|spawn| spawn.position)
        .chain(units::npcs().iter().map(|npc| npc.position))
        .collect();
    let mut rng = Rng(SEED_ROCKS);
    let mut next_id = *ids::LOOSE_ROCKS.start();
    let mut rocks = Vec::new();
    for (bounds, target, attempts, medium) in rock_fields() {
        let mut placed = 0;
        for _ in 0..attempts {
            if placed == target {
                break;
            }
            let at = [
                rng.range(bounds.min[0], bounds.max[0]),
                rng.range(bounds.min[1], bounds.max[1]),
            ];
            let kind = if medium > 0 && rng.next().is_multiple_of(medium) {
                Kind::MediumRock
            } else {
                Kind::SmallRock
            };
            let quarters = u8::try_from(rng.next() % 4).unwrap_or(0);
            let scale = u16::try_from(rng.range(700, 1_300)).unwrap_or(1_000);
            let piece = Piece {
                id: next_id,
                kind,
                at,
                quarters,
                scale_permille: scale,
            };
            let collider = piece.collider();
            let area = rect(&collider);
            let bucket = (at[0].div_euclid(BUCKET), at[1].div_euclid(BUCKET));
            let dry = marsh::WATERS
                .iter()
                .all(|water| water.distance_q10(at[0], at[1]) > 1_150);
            let clear = inside_walls(area.0[0], area.0[1])
                && inside_walls(area.1[0], area.1[1])
                && dry
                && clear_of_roads(&collider)
                && posts.iter().all(|post| !near(area, (*post, *post), 200))
                && buckets
                    .get(&bucket)
                    .is_none_or(|others| others.iter().all(|other| !near(area, *other, 50)));
            if !clear {
                continue;
            }
            block(area, &mut buckets);
            rocks.push(piece);
            next_id += 1;
            placed += 1;
        }
    }
    rocks
}

/// The fixed dressing, then the loose rocks clear of `colliders`.
fn build(colliders: &[StaticCollider]) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = FIXED
        .iter()
        .zip(ids::DRESSING)
        .map(|(&(kind, at, quarters), id)| Piece {
            id,
            kind,
            at,
            quarters,
            scale_permille: 1_000,
        })
        .collect();
    let mut taken = colliders.to_vec();
    taken.extend(pieces.iter().map(Piece::collider));
    pieces.extend(rocks(&taken));
    pieces
}

static PIECES: LazyLock<Vec<Piece>> = LazyLock::new(|| build(&super::undressed_colliders()));

/// Every base piece of dressing, in collider ID order.
#[must_use]
pub fn pieces() -> &'static [Piece] {
    &PIECES
}

/// The Lakeshore dock.
#[must_use]
pub fn dock() -> &'static Piece {
    PIECES
        .iter()
        .find(|piece| piece.kind == Kind::Dock)
        .expect("Greyhaven has its dock")
}

/// The base piece with collider ID `id`.
#[must_use]
pub fn piece(id: u32) -> Option<&'static Piece> {
    PIECES
        .binary_search_by_key(&id, |piece| piece.id)
        .ok()
        .map(|index| &PIECES[index])
}
