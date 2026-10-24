//! The Outpost's kit buildings (#420): where the clients draw asset-tooling's `outpost-kit`
//! modules (`outpost.kit.<part>`) over two of the Greyhaven Outpost's building colliders.
//!
//! The kit's descriptor composes a house on the 8 × 6 m house footprints and a smithy on the
//! 6 × 7 m smithy footprint, in a footprint-base-centre frame whose +Z side has the door; the
//! [`HOUSE`] and [`SMITHY`] tables transcribe those compositions from the pinned release. Each
//! adopted building turns its composition by quarter turns only, so the walls stand exactly on
//! the unchanged collider footprint. The doors hang closed: the buildings are not enterable and
//! their colliders stay solid boxes. Presentation only: the colliders decide.
use mmorpg_core::greyhaven_vale::ids;

use crate::{Prop, Scenery};

/// The kit's rolling release the clients draw.
pub const RELEASE: &str = "outpost-kit";

/// One composed module: part, bundle variant, translation in units and quarter turns about +Y,
/// all in the footprint-base-centre frame.
type Item = (&'static str, &'static str, [i32; 3], u8);

/// The descriptor's `compositions.house`: an 8 × 6 m house, ridge along X, shingle roof.
#[rustfmt::skip]
const HOUSE: [Item; 29] = [
    ("wall-window", "default", [-300, 0, 300], 0),
    ("wall-half", "default", [-150, 0, 300], 0),
    ("wall-door", "default", [0, 0, 300], 0),
    ("wall-half", "default", [150, 0, 300], 0),
    ("wall-window", "default", [300, 0, 300], 0),
    ("wall", "default", [300, 0, -300], 2),
    ("wall-window", "default", [100, 0, -300], 2),
    ("wall-window", "default", [-100, 0, -300], 2),
    ("wall", "default", [-300, 0, -300], 2),
    ("wall", "default", [400, 0, 200], 1),
    ("wall-window", "default", [400, 0, 0], 1),
    ("wall", "default", [400, 0, -200], 1),
    ("gable", "default", [400, 0, 0], 1),
    ("chimney", "default", [400, 0, 0], 1),
    ("wall", "default", [-400, 0, -200], 3),
    ("wall-window", "default", [-400, 0, 0], 3),
    ("wall", "default", [-400, 0, 200], 3),
    ("gable", "default", [-400, 0, 0], 3),
    ("door", "default", [-58, 1, 288], 0),
    ("corner", "default", [400, 0, 300], 0),
    ("corner", "default", [-400, 0, 300], 3),
    ("corner", "default", [-400, 0, -300], 2),
    ("corner", "default", [400, 0, -300], 1),
    ("roof", "default", [-300, 0, 0], 0),
    ("roof", "default", [-100, 0, 0], 0),
    ("roof", "default", [100, 0, 0], 0),
    ("roof", "default", [300, 0, 0], 0),
    ("roof-verge", "default", [400, 0, 0], 0),
    ("roof-verge", "default", [-400, 0, 0], 2),
];

/// The descriptor's `compositions.smithy`: a 6 × 7 m smithy, ridge along Z, slate roof, the
/// door in the front gable wall and the chimney on the back gable.
#[rustfmt::skip]
const SMITHY: [Item; 28] = [
    ("wall-half", "default", [-300, 0, -300], 3),
    ("wall-window", "default", [-300, 0, -150], 3),
    ("wall", "default", [-300, 0, 50], 3),
    ("wall-window", "default", [-300, 0, 250], 3),
    ("wall-window", "default", [300, 0, 250], 1),
    ("wall", "default", [300, 0, 50], 1),
    ("wall-window", "default", [300, 0, -150], 1),
    ("wall-half", "default", [300, 0, -300], 1),
    ("wall", "default", [-200, 0, 350], 0),
    ("wall-door", "default", [0, 0, 350], 0),
    ("wall", "default", [200, 0, 350], 0),
    ("gable", "default", [0, 0, 350], 0),
    ("wall", "default", [200, 0, -350], 2),
    ("wall", "default", [0, 0, -350], 2),
    ("wall", "default", [-200, 0, -350], 2),
    ("gable", "default", [0, 0, -350], 2),
    ("chimney", "default", [0, 0, -350], 2),
    ("door", "default", [-58, 1, 338], 0),
    ("corner", "default", [-300, 0, 350], 3),
    ("corner", "default", [-300, 0, -350], 2),
    ("corner", "default", [300, 0, -350], 1),
    ("corner", "default", [300, 0, 350], 0),
    ("roof", "slate", [0, 0, -250], 3),
    ("roof", "slate", [0, 0, -50], 3),
    ("roof", "slate", [0, 0, 150], 3),
    ("roof-half", "slate", [0, 0, 300], 3),
    ("roof-verge", "slate", [0, 0, 350], 3),
    ("roof-verge", "slate", [0, 0, -350], 1),
];

/// The adopted buildings: collider, composition, its footprint half extents (x, z units) and
/// quarter turns. The house south of the plaza keeps its blockout's door toward the plaza
/// (north); the smithy's door is in its 6 m gable wall, so it faces the east road (south).
const BUILDINGS: [(u32, &[Item], [i32; 2], u8); 2] = [
    (105, &HOUSE, [400, 300], 2),
    (ids::SMITHY, &SMITHY, [300, 350], 0),
];

/// Whether collider `id` is drawn by the kit (and its blockout only as the fallback).
#[must_use]
pub fn adopts(id: u32) -> bool {
    BUILDINGS.iter().any(|(collider, ..)| *collider == id)
}

/// One kit module placed in the zone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    /// The adopted building's collider.
    pub collider: u32,
    /// The bundle entry: `outpost.kit.<part>`, with `/<variant>` unless it is `default`.
    pub entry: &'static str,
    /// The module origin in units, lifted by the presentation relief at the building's centre
    /// so every module of one building shares one base.
    pub position: [i32; 3],
    /// Rotation about +Y, 65 536 steps per turn, 0 keeping the module's +Z front on +Z.
    pub yaw: u16,
}

fn entry(part: &str, variant: &str) -> &'static str {
    const ENTRIES: [&str; 14] = [
        "outpost.kit.chimney",
        "outpost.kit.corner",
        "outpost.kit.door",
        "outpost.kit.gable",
        "outpost.kit.roof",
        "outpost.kit.roof/slate",
        "outpost.kit.roof-half",
        "outpost.kit.roof-half/slate",
        "outpost.kit.roof-verge",
        "outpost.kit.roof-verge/slate",
        "outpost.kit.wall",
        "outpost.kit.wall-door",
        "outpost.kit.wall-half",
        "outpost.kit.wall-window",
    ];
    ENTRIES
        .into_iter()
        .find(|entry| {
            let (key, entry_variant) = entry.split_once('/').unwrap_or((entry, "default"));
            key.strip_prefix("outpost.kit.") == Some(part) && entry_variant == variant
        })
        .unwrap_or_else(|| panic!("the outpost kit has {part}/{variant}"))
}

/// Turns `[x, z]` by `quarters` quarter turns about +Y (+Z toward +X).
const fn turn([x, z]: [i32; 2], quarters: u8) -> [i32; 2] {
    match quarters % 4 {
        0 => [x, z],
        1 => [z, -x],
        2 => [-x, -z],
        _ => [-z, x],
    }
}

/// Every kit module of the adopted buildings on `scenery`'s props, in a fixed order.
///
/// # Panics
///
/// If an adopted collider has no prop or its footprint no longer matches its composition.
#[must_use]
pub fn placements(scenery: &Scenery) -> Vec<Placement> {
    let mut placed = Vec::new();
    for (collider, items, half, quarters) in BUILDINGS {
        let prop: &Prop = scenery
            .props
            .iter()
            .find(|prop| prop.collider == Some(collider))
            .unwrap_or_else(|| panic!("the outpost kit's collider {collider} has a prop"));
        let [hx, _, hz] = prop.half_extents;
        let footprint = turn(half, quarters).map(i32::abs);
        assert_eq!([hx, hz], footprint, "collider {collider}'s footprint");
        let [cx, cy, cz] = prop.position;
        let cy = cy + scenery.height_at(cx, cz);
        for &(part, variant, [x, y, z], yaw) in items {
            let [dx, dz] = turn([x, z], quarters);
            placed.push(Placement {
                collider,
                entry: entry(part, variant),
                position: [cx + dx, cy + y, cz + dz],
                yaw: u16::from((yaw + quarters) % 4) * mmorpg_core::trig::YAW_QUARTER_TURN,
            });
        }
    }
    placed
}

/// The kit entries the placements use, sorted and once each.
#[must_use]
pub fn entries(scenery: &Scenery) -> Vec<&'static str> {
    let mut entries: Vec<_> = placements(scenery)
        .iter()
        .map(|placement| placement.entry)
        .collect();
    entries.sort_unstable();
    entries.dedup();
    entries
}
