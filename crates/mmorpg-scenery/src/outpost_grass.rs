//! Frozen cosmetic placements lowered by the pinned authoring package.

use crate::{Prop, PropKind};

type Placement = (&'static str, [i32; 3], u16, u16, [i32; 3]);
const SELECTED: &[Placement] = &include!("../assets/outpost-grass/selected.props.rs");

fn prop((_, position, yaw, scale_permille, half_extents): Placement) -> Prop {
    Prop {
        kind: PropKind::GrassTuft,
        position,
        yaw,
        scale_permille,
        collider: None,
        half_extents,
    }
}

/// Saved instance IDs and ground anchors for the selected Outpost grass.
/// Relief is applied by clients exactly as for other scenery props.
#[must_use]
pub fn outpost_grass_placements() -> impl ExactSizeIterator<Item = (&'static str, Prop)> {
    SELECTED
        .iter()
        .copied()
        .map(|placement| (placement.0, prop(placement)))
}

fn in_family(prop: &Prop) -> bool {
    prop.kind == PropKind::GrassTuft
        && prop.collider.is_none()
        && (-3_500..=3_500).contains(&prop.position[0])
        && (-1_300..=5_300).contains(&prop.position[2])
}

pub(crate) fn apply(props: &mut Vec<Prop>) {
    props.retain(|prop| !in_family(prop));
    props.extend(outpost_grass_placements().map(|(_, prop)| prop));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Scenery, unmasked_greyhaven_vale_scenery};

    const ACCEPTED: &[Placement] = &include!("../assets/outpost-grass/accepted.props.rs");

    #[test]
    fn saved_family_is_the_exact_original_acceptance_and_other_scenery_is_unchanged() {
        let before = unmasked_greyhaven_vale_scenery();
        let mut captured_revision = before.clone();
        captured_revision.content_revision = 4;
        assert_eq!(captured_revision.stable_hash(), 0xbfbc_7757_2f3a_f7fd);
        let mut previous_revision = before.clone();
        previous_revision.content_revision = 3;
        assert_eq!(previous_revision.stable_hash(), 0x8495_49d6_874e_b332);
        let original: Vec<_> = before
            .props
            .iter()
            .filter(|prop| in_family(prop))
            .copied()
            .collect();
        assert_eq!(
            original,
            ACCEPTED.iter().copied().map(prop).collect::<Vec<_>>()
        );
        assert_eq!(original.len(), 118);
        let unrelated = |scenery: &Scenery| {
            scenery
                .props
                .iter()
                .filter(|prop| !in_family(prop))
                .copied()
                .collect::<Vec<_>>()
        };
        let mut after = before.clone();
        apply(&mut after.props);
        assert_eq!(unrelated(&before), unrelated(&after));
        assert_eq!(after.roads, before.roads);
        assert_eq!(after.water, before.water);
        assert_eq!(after.terrain_grid(400), before.terrain_grid(400));
        assert_eq!(
            after.far_terrain_grid(2_000),
            before.far_terrain_grid(2_000)
        );
        let placed: Vec<_> = after
            .props
            .iter()
            .filter(|prop| in_family(prop))
            .copied()
            .collect();
        assert_eq!(
            placed,
            SELECTED.iter().copied().map(prop).collect::<Vec<_>>()
        );
        assert_eq!(placed.len(), 55);
        for selected in placed {
            assert!(original.contains(&selected));
        }
    }
}
