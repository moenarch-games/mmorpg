//! Seam Test West and Seam Test East (#145, owner decision Q2): two small
//! logical test zones joined by one walking seam, each with a waystone. They
//! exist to exercise the fenced cross-zone transfer between two hosts; they
//! are not a canonical region and Greyhaven Vale has no seam into them. Its
//! outpost waystone reaches them by teleport only.
//!
//! Both zones share one flat layout: a 60 m deep strip between boundary walls
//! with the spawn area in the middle and the waystone to the north (−Z). East
//! coordinates are West coordinates shifted [`SHIFT`] units toward −X. West's
//! seam corridor runs along its east end, East's along its west end, and each
//! crossing strip lands a walker inside the other side's corridor but short
//! of its crossing, so nobody is handed straight back.

use std::sync::{Arc, LazyLock};

use crate::{
    HandoffCorridor, STANDARD_GRAVITY, SpawnGrid, StaticCollider, XzBounds, ZoneAreas, ZoneContent,
    ZoneDefinition, ZoneError, ZoneId,
};

pub const WEST_ZONE: ZoneId = ZoneId::new(901);
pub const EAST_ZONE: ZoneId = ZoneId::new(902);
/// West x minus this is the same place in East's coordinates.
pub const SHIFT: i32 = 10_000;
/// The waystone's XZ centre, the same in both zones.
pub const STONE: [i32; 2] = [0, -2_000];
/// Where travellers to either stone arrive (feet XZ).
pub const ARRIVAL: [i32; 2] = [0, -1_700];

const HALF_DEPTH: i32 = 3_000;
const GRAVEYARD: [i32; 2] = [0, 2_400];
const SPAWN_GRID: SpawnGrid = SpawnGrid {
    origin: [-2_500, -500],
    columns: 32,
    spacing: 150,
};

/// West's corridor x ∈ [3000, 6000) with its crossing at x ∈ [5000, 5400).
const WEST_CORRIDOR: HandoffCorridor = HandoffCorridor {
    neighbor: EAST_ZONE,
    corridor: XzBounds {
        min: [3_000, -HALF_DEPTH],
        max: [6_000, HALF_DEPTH],
    },
    crossing: XzBounds {
        min: [5_000, -HALF_DEPTH],
        max: [5_400, HALF_DEPTH],
    },
    offset: [-SHIFT, 0, 0],
};

/// East's corridor x ∈ [-6000, -3000) with its crossing back at x ∈ [-5800, -5400): a walker
/// arriving at x ∈ [-5000, -4600) stands east of it.
const EAST_CORRIDOR: HandoffCorridor = HandoffCorridor {
    neighbor: WEST_ZONE,
    corridor: XzBounds {
        min: [-6_000, -HALF_DEPTH],
        max: [-3_000, HALF_DEPTH],
    },
    crossing: XzBounds {
        min: [-5_800, -HALF_DEPTH],
        max: [-5_400, HALF_DEPTH],
    },
    offset: [SHIFT, 0, 0],
};

/// Flat ground, the waystone and walls closing `[west, east] × [−3000, 3000]`.
fn definition(west: i32, east: i32) -> Result<ZoneDefinition, ZoneError> {
    let wall = |id, centre: [i32; 2], half: [i32; 2]| StaticCollider {
        id,
        position: [centre[0], 200, centre[1]],
        half_extents: [half[0], 200, half[1]],
    };
    let middle = (west + east) / 2;
    let half_width = (east - west) / 2;
    ZoneDefinition::with_spawn_grid(
        STANDARD_GRAVITY,
        SPAWN_GRID,
        vec![
            StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [7_000, 50, 4_000],
            },
            StaticCollider {
                id: 2,
                position: [STONE[0], 75, STONE[1]],
                half_extents: [50, 75, 50],
            },
            wall(3, [west - 50, 0], [50, HALF_DEPTH + 100]),
            wall(4, [east + 50, 0], [50, HALF_DEPTH + 100]),
            wall(5, [middle, -HALF_DEPTH - 50], [half_width, 50]),
            wall(6, [middle, HALF_DEPTH + 50], [half_width, 50]),
        ],
    )
}

fn content(
    west: i32,
    east: i32,
    corridor: HandoffCorridor,
    waystone: crate::WaystoneId,
) -> Result<Arc<ZoneContent>, ZoneError> {
    let content = ZoneContent::new(
        definition(west, east)?,
        ZoneAreas::default(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        GRAVEYARD,
    )?
    .with_creature_abilities(Vec::new())?
    .with_handoff_corridors(vec![corridor])?
    .with_waystones(vec![waystone])?;
    Ok(Arc::new(content))
}

static WEST: LazyLock<Arc<ZoneContent>> = LazyLock::new(|| {
    content(-4_000, 6_000, WEST_CORRIDOR, crate::SEAM_TEST_WEST)
        .expect("built-in Seam Test West content is valid")
});

static EAST: LazyLock<Arc<ZoneContent>> = LazyLock::new(|| {
    content(-6_000, 4_000, EAST_CORRIDOR, crate::SEAM_TEST_EAST)
        .expect("built-in Seam Test East content is valid")
});

#[must_use]
pub fn west() -> Arc<ZoneContent> {
    Arc::clone(&WEST)
}

#[must_use]
pub fn east() -> Arc<ZoneContent> {
    Arc::clone(&EAST)
}

/// The content a host runs as `zone`: a Seam Test zone by its ID, Greyhaven
/// Vale otherwise.
#[must_use]
pub fn content_for_zone(zone: ZoneId) -> Arc<ZoneContent> {
    match zone {
        WEST_ZONE => west(),
        EAST_ZONE => east(),
        _ => crate::greyhaven_vale::content(),
    }
}
