//! Simulated livestock (#1019): the zone's strays and watched flocks are NPCs moved by its flock step,
//! projected like any NPC, and both clients draw them with asset-tooling's
//! `greyhaven-farm-animals` (#849) sheep instead of a resident's figure. A sheep walks (`Walk`)
//! while its projection moves, grazes (`Eat`, or `Idle` on a model without it) while it stands,
//! and lies on its side, still, once it fell in a watch (its projected health at the floor).
//!
//! Poses are a pure function of the sampled units and the presentation clock, like the ambient
//! animals' ([`crate::ambient`]), so both clients agree. Ambient sheep elsewhere stay as they are.

use mmorpg_core::{EntityKind, EntityRef, NpcId, ZoneContent};
use mmorpg_scenery::Scenery;

use crate::ambient::Pose;
use crate::batching::matrix;
use crate::geometry::{compose, ry, rz, translate};
use crate::units::SampledUnit;

/// The farm sheep both clients load from [`crate::ambient::FARM_RELEASE`].
pub const SHEEP: &str = "greyhaven.farm.sheep";
/// The clip indices of [`crate::ambient::CLIPS`].
const IDLE: usize = 0;
const WALK: usize = 1;
const EAT: usize = 2;

/// Whether `entity` is livestock the clients draw as a farm animal, not a figure.
#[must_use]
pub fn is_livestock(content: &ZoneContent, entity: EntityRef) -> bool {
    matches!(entity, EntityRef::Npc(npc) if content.is_livestock(npc))
}

/// The livestock among `units` in their order, posed at presentation `seconds` on `scenery`'s
/// relief; without `animate` (reduced motion) each holds `Idle`.
#[must_use]
pub fn poses(
    content: &ZoneContent,
    scenery: &Scenery,
    units: &[SampledUnit],
    seconds: f64,
    animate: bool,
) -> Vec<Pose> {
    units
        .iter()
        .filter(|unit| {
            unit.record.kind == EntityKind::Npc && content.is_livestock(NpcId::new(unit.record.id))
        })
        .map(|unit| pose(scenery, unit, seconds, animate))
        .collect()
}

fn pose(scenery: &Scenery, unit: &SampledUnit, seconds: f64, animate: bool) -> Pose {
    let [x, _, z] = unit.centre.map(f64::from);
    #[allow(clippy::cast_possible_truncation)]
    let lift = f64::from(scenery.surface_at(
        (x * f64::from(mmorpg_core::UNITS_PER_METRE)).round() as i32,
        (z * f64::from(mmorpg_core::UNITS_PER_METRE)).round() as i32,
    )) / f64::from(mmorpg_core::UNITS_PER_METRE);
    let fallen = unit.record.health_percent <= 1;
    let moving = unit.record.velocity != [0, 0, 0];
    // Each sheep grazes on its own phase.
    let phase = f64::from(unit.record.id % 7) * 0.37;
    let (clip, time) = match (animate, fallen, moving) {
        (false, ..) | (true, true, _) => (IDLE, 0.),
        (true, false, true) => (WALK, seconds + phase),
        (true, false, false) => (EAT, seconds + phase),
    };
    let mut parts = vec![translate(x, lift, z), ry(f64::from(unit.yaw))];
    if fallen {
        // On its side, its back toward its left.
        parts.push(translate(0., 0.25, 0.));
        parts.push(rz(std::f64::consts::FRAC_PI_2));
    }
    Pose {
        key: SHEEP,
        model: matrix(compose(&parts)),
        clip,
        time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{EntityFlags, EntitySnapshot};
    use mmorpg_scenery::greyhaven_vale_scenery;

    fn unit(id: u32, velocity: [i8; 3], health_percent: u8) -> SampledUnit {
        SampledUnit {
            record: EntitySnapshot {
                kind: EntityKind::Npc,
                id,
                appearance: id,
                position: [1_500_000, 0, 4_620_000],
                velocity,
                facing: 0,
                level: 1,
                health_percent,
                flags: EntityFlags::default(),
            },
            centre: [150., 8., 462.],
            yaw: 0.,
        }
    }

    #[test]
    fn the_flock_and_the_stray_draw_as_farm_sheep_that_walk_graze_and_fall() {
        let content = mmorpg_core::greyhaven_vale::content();
        let scenery = greyhaven_vale_scenery();
        let lise = mmorpg_core::greyhaven_vale::flock::SHEPHERD_LISE.get();
        let ewe = mmorpg_core::greyhaven_vale::flock::STRAY_EWE.get();
        let [first, ..] = mmorpg_core::greyhaven_vale::flock::FLOCK.map(NpcId::get);
        let units = [
            unit(lise, [0; 3], 100),
            unit(ewe, [0, 0, 4], 100),
            unit(first, [0; 3], 100),
            unit(first, [0; 3], 1),
        ];
        let poses = poses(&content, &scenery, &units, 2., true);
        // Lise is a resident figure, not livestock.
        assert_eq!(poses.len(), 3);
        assert!(poses.iter().all(|pose| pose.key == SHEEP));
        assert_eq!(
            poses.iter().map(|pose| pose.clip).collect::<Vec<_>>(),
            [WALK, EAT, IDLE]
        );
        // The fallen one lies on its side: its model's up axis turned sideways.
        assert!(poses[2].model[5].abs() < 1e-6, "{:?}", poses[2].model);
        assert!(!is_livestock(&content, EntityRef::Npc(NpcId::new(lise))));
        assert!(is_livestock(&content, EntityRef::Npc(NpcId::new(ewe))));
        // Reduced motion holds them still.
        assert!(
            super::poses(&content, &scenery, &units, 2., false)
                .iter()
                .all(|pose| pose.clip == IDLE)
        );
    }
}
