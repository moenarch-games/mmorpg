//! Saved signed-centimetre terrain, sampled only by presentation scenery.

#[derive(Clone, Copy)]
struct HeightField {
    origin: [i32; 2],
    step: i32,
    columns: usize,
    rows: usize,
    heights: &'static [i8],
}

const AUTHORED: HeightField = include!("../assets/outpost-relief/flattened.heights.rs");

impl HeightField {
    fn height_at(self, x: i32, z: i32) -> Option<i32> {
        let dx = i64::from(x) - i64::from(self.origin[0]);
        let dz = i64::from(z) - i64::from(self.origin[1]);
        let step = i64::from(self.step);
        let width = i64::try_from(self.columns - 1).ok()? * step;
        let depth = i64::try_from(self.rows - 1).ok()? * step;
        if !(0..=width).contains(&dx) || !(0..=depth).contains(&dz) {
            return None;
        }
        // Inclusive last samples use the final cell, with a full step offset.
        let column = usize::try_from(dx / step).ok()?.min(self.columns - 2);
        let row = usize::try_from(dz / step).ok()?.min(self.rows - 2);
        let along_x = dx - i64::try_from(column).ok()? * step;
        let along_z = dz - i64::try_from(row).ok()? * step;
        let sample = |column, row| i64::from(self.heights[row * self.columns + column]);
        let numerator = sample(column, row) * (step - along_x) * (step - along_z)
            + sample(column + 1, row) * along_x * (step - along_z)
            + sample(column, row + 1) * (step - along_x) * along_z
            + sample(column + 1, row + 1) * along_x * along_z;
        let denominator = step * step;
        // Integer bilinear sampling, nearest centimetre; half ties toward +Y.
        i32::try_from((numerator + denominator / 2).div_euclid(denominator)).ok()
    }
}

pub(crate) fn height_at(x: i32, z: i32) -> Option<i32> {
    AUTHORED.height_at(x, z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{greyhaven_vale_scenery, unmasked_greyhaven_vale_scenery};

    const SOURCE: HeightField = include!("../assets/outpost-relief/source.heights.rs");

    #[test]
    fn every_saved_sample_has_exact_world_calibration_and_original_capture() {
        assert_eq!(AUTHORED.origin, [-3_500, -1_300]);
        assert_eq!(
            (AUTHORED.step, AUTHORED.columns, AUTHORED.rows),
            (50, 141, 133)
        );
        assert_eq!(AUTHORED.heights.len(), AUTHORED.columns * AUTHORED.rows);
        assert_eq!(SOURCE.heights.len(), AUTHORED.heights.len());
        let before = unmasked_greyhaven_vale_scenery();
        let after = greyhaven_vale_scenery();
        let mut changed = 0;
        for row in 0..AUTHORED.rows {
            for column in 0..AUTHORED.columns {
                let x = AUTHORED.origin[0] + i32::try_from(column).unwrap() * AUTHORED.step;
                let z = AUTHORED.origin[1] + i32::try_from(row).unwrap() * AUTHORED.step;
                let index = row * AUTHORED.columns + column;
                assert_eq!(before.height_at(x, z), i32::from(SOURCE.heights[index]));
                assert_eq!(after.height_at(x, z), i32::from(AUTHORED.heights[index]));
                if AUTHORED.heights[index] != SOURCE.heights[index] {
                    changed += 1;
                }
                if !(2..AUTHORED.rows - 2).contains(&row)
                    || !(2..AUTHORED.columns - 2).contains(&column)
                {
                    assert_eq!(AUTHORED.heights[index], SOURCE.heights[index]);
                }
            }
        }
        assert_eq!(changed, 4_517);
        assert_eq!(
            AUTHORED.height_at(3_500, 5_300),
            AUTHORED.heights.last().map(|height| i32::from(*height))
        );
    }

    #[test]
    fn signed_non_square_sampling_uses_both_axes_and_inclusive_endpoints() {
        let field = HeightField {
            origin: [-50, 100],
            step: 50,
            columns: 3,
            rows: 2,
            heights: &[-40, -20, 0, 0, 20, 40],
        };
        assert_eq!(field.height_at(-50, 100), Some(-40));
        assert_eq!(field.height_at(50, 150), Some(40));
        assert_eq!(field.height_at(-25, 125), Some(-10));
        assert_eq!(field.height_at(25, 125), Some(10));
        for [x, z] in [
            [-51, 100],
            [51, 150],
            [0, 99],
            [0, 151],
            [i32::MIN, i32::MAX],
        ] {
            assert_eq!(field.height_at(x, z), None);
        }
    }

    #[test]
    fn adoption_preserves_other_scenery_and_surrounding_relief_with_centimetre_seams() {
        let mut before = unmasked_greyhaven_vale_scenery();
        crate::outpost_grass::apply(&mut before.props);
        let after = greyhaven_vale_scenery();
        let mut captured_revision = before.clone();
        captured_revision.content_revision = 4;
        assert_eq!(captured_revision.stable_hash(), 0x1de3_341c_9334_49f6);
        assert_eq!(before.props, after.props);
        assert_eq!(before.roads, after.roads);
        assert_eq!(before.water, after.water);
        assert_eq!(before.content_revision, after.content_revision);
        for x in (-60_000..=60_000).step_by(400) {
            for z in (-60_000..=60_000).step_by(400) {
                if AUTHORED.height_at(x, z).is_none() {
                    assert_eq!(after.height_at(x, z), before.height_at(x, z));
                }
            }
        }
        // The untouched boundary samples reconstruct the old edge to at most
        // one centimetre between nodes; adjacent integer coordinates cannot jump.
        for z in -1_300..=5_300 {
            for x in [-3_500, 3_500] {
                assert!((after.height_at(x, z) - before.height_at(x, z)).abs() <= 1);
                for adjacent in [x - 1, x + 1] {
                    assert!((after.height_at(x, z) - after.height_at(adjacent, z)).abs() <= 2);
                }
            }
        }
        for x in -3_500..=3_500 {
            for z in [-1_300, 5_300] {
                assert!((after.height_at(x, z) - before.height_at(x, z)).abs() <= 1);
                for adjacent in [z - 1, z + 1] {
                    assert!((after.height_at(x, z) - after.height_at(x, adjacent)).abs() <= 2);
                }
            }
        }
    }
}
