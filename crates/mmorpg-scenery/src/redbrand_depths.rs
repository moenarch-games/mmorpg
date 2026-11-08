//! Scenery of the Redbrand Depths (#404): stand-in rock boxes for every wall, one prop per
//! collider. Presentation only; it comes straight from the depths' colliders so what is drawn
//! is what collides.

use mmorpg_core::redbrand_depths;

use crate::{Prop, PropKind, Scenery};

/// The floor collider's ID, drawn as the ground rather than a prop.
const FLOOR_COLLIDER: u32 = 1;

#[must_use]
pub fn redbrand_depths_scenery() -> Scenery {
    let content = redbrand_depths::content();
    let mut scenery = Scenery::flat(content.revision());
    let mut props: Vec<_> = content
        .definition()
        .colliders()
        .iter()
        .filter(|collider| collider.id != FLOOR_COLLIDER)
        .map(|collider| {
            let half = collider.half_extents;
            Prop {
                kind: PropKind::Cliff,
                position: [
                    collider.position[0],
                    collider.position[1] - half[1],
                    collider.position[2],
                ],
                yaw: 0,
                scale_permille: 1_000,
                collider: Some(collider.id),
                half_extents: half,
            }
        })
        .collect();
    props.sort_by_key(|prop| (prop.position[0], prop.position[2], prop.collider));
    scenery.props = props;
    scenery
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::ZoneId;

    #[test]
    fn every_depths_slot_presents_the_depths_with_one_prop_per_wall() {
        let scenery = crate::scenery_for_zone(ZoneId::new(redbrand_depths::ZONE_IDS.start));
        let colliders = redbrand_depths::content().definition().colliders().len();
        assert_eq!(scenery.props.len(), colliders - 1);
        assert_eq!(
            scenery.content_revision,
            redbrand_depths::content().revision()
        );
    }
}
