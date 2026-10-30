//! Scenery of the Seam Test zones (#145): flat ground, the zone's waystone and palisades
//! along its walls. Across the seam a player sees the neighbouring zone's landscape (owner
//! decision Q3 = A): its stone and its walls, drawn in this zone's coordinates, but never
//! its units. So the walls drawn enclose both zones together, and the wall that ends this
//! zone behind its seam (which nobody reaches: the crossing hands them over first) is not
//! drawn.

use mmorpg_core::seam_test::{EAST_ZONE, SHIFT, STONE, WEST_ZONE};
use mmorpg_core::{ZoneId, content_for_zone};

use crate::{Prop, PropKind, Scenery};

/// Half the zones' depth, as in `mmorpg_core::seam_test`.
const HALF_DEPTH: i32 = 3_000;
const WALL_HALF_HEIGHT: i32 = 200;
const WALL_HALF_THICKNESS: i32 = 50;
/// The stone's collider in both zones.
const STONE_COLLIDER: u32 = 2;
const STONE_HALF_EXTENTS: [i32; 3] = [50, 75, 50];

/// The scenery of Seam Test West or East.
///
/// # Panics
///
/// If `zone` is neither Seam Test zone.
#[must_use]
pub fn seam_test_scenery(zone: ZoneId) -> Scenery {
    // This zone's x extent, and where the neighbour's stone stands in its coordinates.
    let (west, east, neighbour_stone) = match zone {
        WEST_ZONE => (-4_000, 6_000 + SHIFT - 2_000, STONE[0] + SHIFT),
        EAST_ZONE => (-6_000 - SHIFT + 2_000, 4_000, STONE[0] - SHIFT),
        _ => panic!("zone {} is not a Seam Test zone", zone.get()),
    };
    let wall = |centre: [i32; 2], half: [i32; 2]| Prop {
        kind: PropKind::PalisadeSegment,
        position: [centre[0], 0, centre[1]],
        yaw: 0,
        scale_permille: 1_000,
        collider: None,
        half_extents: [half[0], WALL_HALF_HEIGHT, half[1]],
    };
    let stone = |x: i32, collider| Prop {
        kind: PropKind::Waystone,
        position: [x, 0, STONE[1]],
        yaw: 0,
        scale_permille: 1_000,
        collider,
        half_extents: STONE_HALF_EXTENTS,
    };
    let middle = (west + east) / 2;
    let half_width = (east - west) / 2;
    let side = WALL_HALF_THICKNESS;
    let mut props = vec![
        stone(STONE[0], Some(STONE_COLLIDER)),
        stone(neighbour_stone, None),
        wall([west - side, 0], [side, HALF_DEPTH + 2 * side]),
        wall([east + side, 0], [side, HALF_DEPTH + 2 * side]),
        wall([middle, -HALF_DEPTH - side], [half_width, side]),
        wall([middle, HALF_DEPTH + side], [half_width, side]),
    ];
    props.sort_by_key(|prop| (prop.position[0], prop.position[2]));
    let mut scenery = Scenery::flat(content_for_zone(zone).revision());
    scenery.props = props;
    scenery
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_zone_sees_its_own_stone_and_the_neighbours_across_the_seam() {
        for (zone, other) in [(WEST_ZONE, SHIFT), (EAST_ZONE, -SHIFT)] {
            let scenery = seam_test_scenery(zone);
            assert_eq!(scenery.content_revision, content_for_zone(zone).revision());
            assert!(!scenery.greyhaven(), "no Greyhaven kits in a test zone");
            let stones: Vec<_> = scenery
                .props
                .iter()
                .filter(|prop| prop.kind == PropKind::Waystone)
                .map(|prop| (prop.position[0], prop.collider))
                .collect();
            assert!(stones.contains(&(STONE[0], Some(STONE_COLLIDER))));
            assert!(stones.contains(&(STONE[0] + other, None)));
        }
    }
}
