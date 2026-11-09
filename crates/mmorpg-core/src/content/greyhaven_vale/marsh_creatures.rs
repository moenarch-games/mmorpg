//! The Marsh-Lake Wetlands' creatures (#506, follows #426): Mirefin Lurkers in the lake and the
//! two swimmable pools, Young Boars on the hummocks and a Timber Wolf pack along the Marsh Trail
//! and the Heron Path, on the existing templates, AI, leash, respawn and loot rules.
//!
//! The templates' level ranges are inherited, so the region sits beyond the Vale only at their
//! top end: wolves and boars roll level 2, lurkers their only level, 3. Spawn IDs 1100–1199 are
//! the region's, after the Highlands garrison.
//!
//! Lurkers swim as at Stillwater and the Sunken Shrine (#274): each wander disc lies wholly in
//! water deep enough to float in, off the basins' slopes (physics-engine #394), so they never
//! touch the bed. Three float beside the Causeway, clear of its raised way, and the approach and
//! the island hold nothing else: the drowned watchtower looks dangerous without the walk being
//! blocked. A viewer anywhere in the region keeps room for other players inside the protocol's
//! 35-entity packet budget (`tests/marsh_creatures.rs`).

use crate::content::CreatureSpawn;
use crate::{CreatureId, CreatureTemplateId};

use super::units::WANDER_RADIUS_UNITS;
use super::units::templates::{MIREFIN_LURKER, TIMBER_WOLF, YOUNG_BOAR};

/// Lurkers idle within 2 m of their spawn point, staying over deep water.
pub const LURKER_WANDER_RADIUS_UNITS: i32 = 200;

/// The templates' top levels: `[2, 2]` for wolves and boars.
const TOP: [u8; 2] = [2, 2];
/// A Mirefin Lurker's only level.
const LURKER: [u8; 2] = [3, 3];

/// Yaw 0 faces +Z (south), 32 768 north, 16 384 east and 49 152 west.
const SOUTH: u16 = 0;
const EAST: u16 = 16_384;
const NORTH: u16 = 32_768;
const WEST: u16 = 49_152;

type Row = (u32, CreatureTemplateId, [i32; 2], u16, [u8; 2], i32);

#[rustfmt::skip]
const SPAWNS: [Row; 31] = {
    const W: i32 = WANDER_RADIUS_UNITS;
    const L: i32 = LURKER_WANDER_RADIUS_UNITS;
    [
        // Marsh Lake: north of the island …
        (1100, MIREFIN_LURKER, [43_500, -5_300], SOUTH, LURKER, L),
        (1101, MIREFIN_LURKER, [46_000, -5_400], SOUTH, LURKER, L),
        (1102, MIREFIN_LURKER, [48_500, -5_400], WEST, LURKER, L),
        // … south of it …
        (1103, MIREFIN_LURKER, [43_500, 1_300], NORTH, LURKER, L),
        (1104, MIREFIN_LURKER, [46_000, 1_400], NORTH, LURKER, L),
        (1105, MIREFIN_LURKER, [48_500, 1_400], WEST, LURKER, L),
        // … and off its eastern shelf.
        (1106, MIREFIN_LURKER, [50_500, -2_800], WEST, LURKER, L),
        (1107, MIREFIN_LURKER, [50_500, -1_200], WEST, LURKER, L),
        // Along the Causeway's edges, outside its raised way.
        (1110, MIREFIN_LURKER, [41_500, -3_050], SOUTH, LURKER, L),
        (1111, MIREFIN_LURKER, [42_900, -3_050], SOUTH, LURKER, L),
        (1112, MIREFIN_LURKER, [42_200, -950], NORTH, LURKER, L),
        // The two swimmable pools: the Cottage Pool …
        (1120, MIREFIN_LURKER, [32_500, 11_000], EAST, LURKER, L),
        (1121, MIREFIN_LURKER, [33_500, 11_000], WEST, LURKER, L),
        // … and Southmere.
        (1122, MIREFIN_LURKER, [54_800, -15_000], EAST, LURKER, L),
        (1123, MIREFIN_LURKER, [55_200, -15_000], WEST, LURKER, L),
        // Young Boars on the hummocks: west by the pass …
        (1140, YOUNG_BOAR, [27_500, 8_500], SOUTH, TOP, W),
        (1141, YOUNG_BOAR, [29_800, 10_900], EAST, TOP, W),
        (1142, YOUNG_BOAR, [27_900, -4_200], NORTH, TOP, W),
        (1143, YOUNG_BOAR, [31_500, -3_600], WEST, TOP, W),
        // … and north, south and east of the lake.
        (1144, YOUNG_BOAR, [39_500, 14_500], SOUTH, TOP, W),
        (1145, YOUNG_BOAR, [46_500, 15_500], WEST, TOP, W),
        (1146, YOUNG_BOAR, [43_600, -14_000], NORTH, TOP, W),
        (1147, YOUNG_BOAR, [59_800, 4_300], WEST, TOP, W),
        // Timber Wolves roaming the Marsh Trail, short of the Causeway approach …
        (1160, TIMBER_WOLF, [27_000, 2_000], EAST, TOP, W),
        (1161, TIMBER_WOLF, [27_900, 3_100], SOUTH, TOP, W),
        (1162, TIMBER_WOLF, [30_200, 3_500], NORTH, TOP, W),
        (1163, TIMBER_WOLF, [30_900, 4_700], SOUTH, TOP, W),
        // … and the Heron Path.
        (1164, TIMBER_WOLF, [36_200, 7_300], NORTH, TOP, W),
        (1165, TIMBER_WOLF, [36_700, 8_500], WEST, TOP, W),
        (1166, TIMBER_WOLF, [44_800, 9_000], EAST, TOP, W),
        (1167, TIMBER_WOLF, [45_700, 10_200], SOUTH, TOP, W),
    ]
};

/// The wetlands' content spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    SPAWNS
        .iter()
        .map(
            |&(id, template, position, facing, levels, wander_radius)| CreatureSpawn {
                id: CreatureId::new(id),
                template,
                position,
                facing,
                wander_radius,
                levels: Some(levels),
            },
        )
        .collect()
}
