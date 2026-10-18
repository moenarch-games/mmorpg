//! The Greyfang cave (#159): an enclosed rock chamber set into the Greyfang Ridge hillside,
//! east of Redbrand Hollow. A 4 m mouth in its south wall opens on the flat foot of the ridge;
//! inside, an outer cavern joins a smaller inner chamber through a 4 m passage.
//!
//! Everything is authored as [`Cave`] data so a later cave reuses it unchanged; #160 grows this
//! one into Redbrand Hollow's shared outer cavern. Rock walls and one roof slab are fixed
//! boxes standing on y = 0. The terrain is flattened to y = 0 over every [`Cave::floors`] rect
//! (each edge on the 4 m terrain grid), so the hillside rises behind the walls while the floor
//! inside stays flat. Clients cut the roof away while the viewer stands under it, like the inn.

use crate::StaticCollider;

/// A cave of authored rock boxes: `(min, max)` XZ footprints in units, standing on y = 0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cave {
    /// Walkable floor rects (chambers, passages, the mouth), flattened and roofed.
    pub floors: &'static [([i32; 2], [i32; 2])],
    /// Rock walls around the floors, leaving the mouth open.
    pub walls: &'static [([i32; 2], [i32; 2])],
    /// The roof slab's footprint: every floor and wall.
    pub roof: ([i32; 2], [i32; 2]),
    /// Clear height under the roof.
    pub height: i32,
    /// Roof slab thickness.
    pub roof_thickness: i32,
}

impl Cave {
    /// Whether the terrain is flattened at `(x, z)`: on or inside a floor rect.
    #[must_use]
    pub fn flattens(&self, x: i32, z: i32) -> bool {
        self.floors
            .iter()
            .any(|(min, max)| (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z))
    }

    /// The walls, then the roof, as colliders with IDs from `first_id`.
    #[must_use]
    pub fn colliders(&self, first_id: u32) -> Vec<StaticCollider> {
        let block = |id, (min, max): ([i32; 2], [i32; 2]), base: i32, height: i32| {
            let half = [(max[0] - min[0]) / 2, height / 2, (max[1] - min[1]) / 2];
            StaticCollider {
                id,
                position: [min[0] + half[0], base + half[1], min[1] + half[2]],
                half_extents: half,
            }
        };
        self.walls
            .iter()
            .zip(first_id..)
            .map(|(&footprint, id)| block(id, footprint, 0, self.height))
            .chain(std::iter::once(block(
                first_id + u32::try_from(self.walls.len()).unwrap_or(u32::MAX),
                self.roof,
                self.height,
                self.roof_thickness,
            )))
            .collect()
    }
}

/// The Greyfang cave: walls 4 m thick and 4.5 m tall under a 1 m roof.
#[rustfmt::skip]
pub const GREYFANG: Cave = Cave {
    floors: &[
        // The outer cavern.
        ([4_800, -14_400], [6_800, -13_200]),
        // The passage to the inner chamber.
        ([6_800, -14_000], [7_200, -13_600]),
        // The inner chamber.
        ([7_200, -14_400], [8_000, -13_200]),
        // The mouth, through the south wall.
        ([5_600, -13_200], [6_000, -12_800]),
    ],
    walls: &[
        // North, under the hillside.
        ([4_400, -14_800], [8_400, -14_400]),
        // South, either side of the mouth.
        ([4_400, -13_200], [5_600, -12_800]),
        ([6_000, -13_200], [8_400, -12_800]),
        // West and east.
        ([4_400, -14_400], [4_800, -13_200]),
        ([8_000, -14_400], [8_400, -13_200]),
        // Between the chambers, either side of the passage.
        ([6_800, -14_400], [7_200, -14_000]),
        ([6_800, -13_600], [7_200, -13_200]),
    ],
    roof: ([4_400, -14_800], [8_400, -12_800]),
    height: 450,
    roof_thickness: 100,
};
