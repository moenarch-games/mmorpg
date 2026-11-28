//! The minimap both clients draw (#1280): its zoom radii, which class of dot each projected
//! unit shows as, how a world offset turns into a map offset with the camera heading or north
//! up, a night raid's disturbance circle and how far discovered landmarks show.
//!
//! Every value comes from the received projection, the zone content and local camera state;
//! the clients only paint them.
use mmorpg_core::{EntityKind, EntityRef, EntitySnapshot, ZoneContent, ZoneSnapshot};

/// Zoom levels: metres from the centre to the rim, nearest first.
pub const RADII_METRES: [f32; 4] = [30.0, 45.0, 70.0, 100.0];

/// The zoom level a minimap opens at, an index into [`RADII_METRES`].
pub const DEFAULT_ZOOM: usize = 1;

/// How far from the viewer discovered landmarks show on the minimap, in metres.
pub const LANDMARK_RANGE_METRES: f32 = 160.0;

const UNITS_PER_METRE: f32 = 100.0;

/// The class of dot a projected unit shows as; each client picks its colour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DotClass {
    /// Another player.
    Player,
    /// An NPC.
    Friendly,
    /// A creature that attacks players on sight.
    Hostile,
    /// A creature that does not.
    Neutral,
    /// A summoned skeleton or other minion (#295).
    Minion,
    /// Any dead unit.
    Dead,
}

/// The dot class of `entity`: dead units first, then by kind and a creature's hostility.
#[must_use]
pub const fn dot_class(entity: &EntitySnapshot) -> DotClass {
    if entity.flags.dead {
        return DotClass::Dead;
    }
    match entity.kind {
        EntityKind::Player => DotClass::Player,
        EntityKind::Npc => DotClass::Friendly,
        EntityKind::Creature if entity.flags.hostile => DotClass::Hostile,
        EntityKind::Creature => DotClass::Neutral,
        EntityKind::Minion => DotClass::Minion,
    }
}

/// Whether `entity` is the viewer of `snapshot`, who shows as the arrow instead of a dot.
#[must_use]
pub fn is_viewer(snapshot: &ZoneSnapshot, entity: &EntitySnapshot) -> bool {
    entity.kind == EntityKind::Player && entity.id == snapshot.viewer_id
}

/// Every projected unit but the viewer with its dot class, in projection order.
#[must_use]
pub fn dots(snapshot: &ZoneSnapshot) -> Vec<(EntityRef, DotClass)> {
    snapshot
        .entities
        .iter()
        .filter(|entity| !is_viewer(snapshot, entity))
        .map(|entity| (entity.entity(), dot_class(entity)))
        .collect()
}

/// The camera heading of an XZ `forward` vector: radians, 0 toward +Z, increasing toward +X.
#[must_use]
pub fn heading_of(forward: [f32; 2]) -> f32 {
    forward[0].atan2(forward[1])
}

/// The screen rotation (radians, clockwise positive with screen y down) that [`offset`]
/// applies: the camera `heading` points up when `heading_up`, otherwise north (−Z) does.
#[must_use]
pub fn rotation(heading: f32, heading_up: bool) -> f32 {
    if heading_up {
        heading - std::f32::consts::PI
    } else {
        0.0
    }
}

/// The map offset (x right, y down) of a world offset `(dx, dz)` in metres, at
/// `pixels_per_metre`, for a camera `heading` ([`heading_of`]).
#[must_use]
pub fn offset(dx: f32, dz: f32, heading: f32, heading_up: bool, pixels_per_metre: f32) -> [f32; 2] {
    let (sin, cos) = rotation(heading, heading_up).sin_cos();
    [
        (dx * cos - dz * sin) * pixels_per_metre,
        (dx * sin + dz * cos) * pixels_per_metre,
    ]
}

/// Keeps a mark inside the rim: an offset beyond `radius` moves onto it. Units, quest areas and
/// raid areas off the map sit on the rim pointing at them; the north letter stands just inside.
#[must_use]
pub fn clamp_to_rim(offset: [f32; 2], radius: f32) -> [f32; 2] {
    let length = offset[0].hypot(offset[1]);
    if length <= radius || length == 0.0 {
        offset
    } else {
        [offset[0] / length * radius, offset[1] / length * radius]
    }
}

/// A night raid's approximate disturbance area (#530), centre and radius in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Disturbance {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
}

/// The disturbance circle while a night raid's assault runs: the area of the raid site whose
/// signal reaches the viewer (#1019), or none.
#[must_use]
pub fn disturbance(snapshot: &ZoneSnapshot, content: &ZoneContent) -> Option<Disturbance> {
    if !snapshot.raid.as_ref().is_some_and(|raid| raid.phase > 0) {
        return None;
    }
    let viewer = snapshot
        .entities
        .iter()
        .find(|entity| is_viewer(snapshot, entity))?;
    let ([x, z], radius) = content
        .raid_near([viewer.position[0], viewer.position[2]])?
        .1
        .area();
    Some(Disturbance {
        x: x as f32 / UNITS_PER_METRE,
        z: z as f32 / UNITS_PER_METRE,
        radius: radius as f32 / UNITS_PER_METRE,
    })
}

/// Whether a discovered landmark at `landmark` (metres) shows for a viewer at `at`.
#[must_use]
pub fn landmark_in_range(landmark: [f32; 2], at: [f32; 2]) -> bool {
    (landmark[0] - at[0]).hypot(landmark[1] - at[1]) <= LANDMARK_RANGE_METRES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;
    use mmorpg_core::EntityFlags;

    fn unit(kind: EntityKind, id: u32, flags: EntityFlags) -> EntitySnapshot {
        EntitySnapshot {
            kind,
            id,
            appearance: 0,
            position: [0, 0, 0],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags,
        }
    }

    #[test]
    fn dots_class_every_unit_but_the_viewer() {
        let mut view = snapshot(1, 5);
        let hostile = EntityFlags {
            hostile: true,
            ..EntityFlags::default()
        };
        let dead = EntityFlags {
            dead: true,
            hostile: true,
            ..EntityFlags::default()
        };
        view.entities = vec![
            unit(EntityKind::Player, 1, EntityFlags::default()),
            unit(EntityKind::Player, 2, EntityFlags::default()),
            unit(EntityKind::Npc, 3, EntityFlags::default()),
            unit(EntityKind::Creature, 4, hostile),
            unit(EntityKind::Creature, 5, EntityFlags::default()),
            unit(EntityKind::Minion, 6, EntityFlags::default()),
            unit(EntityKind::Creature, 7, dead),
        ];
        let classes: Vec<_> = dots(&view).into_iter().map(|(_, class)| class).collect();
        assert_eq!(
            classes,
            [
                DotClass::Player,
                DotClass::Friendly,
                DotClass::Hostile,
                DotClass::Neutral,
                DotClass::Minion,
                DotClass::Dead,
            ]
        );
    }

    #[test]
    fn the_heading_points_up_and_north_up_keeps_the_world_axes() {
        let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        // Facing +Z, a unit 10 m ahead is straight up and one 10 m toward +X is to the left.
        let heading = heading_of([0.0, 1.0]);
        assert!(close(offset(0.0, 10.0, heading, true, 2.0), [0.0, -20.0]));
        assert!(close(offset(10.0, 0.0, heading, true, 2.0), [-20.0, 0.0]));
        // Facing +X, a unit ahead is up too.
        assert!(close(offset(10.0, 0.0, heading_of([1.0, 0.0]), true, 1.0), [0.0, -10.0]));
        // North up: +X right, +Z down, whatever the heading.
        assert!(close(offset(3.0, 4.0, heading, false, 1.0), [3.0, 4.0]));
        assert!(close(clamp_to_rim([30.0, 40.0], 10.0), [6.0, 8.0]));
        assert!(close(clamp_to_rim([3.0, 4.0], 10.0), [3.0, 4.0]));
    }

    #[test]
    fn landmarks_show_within_their_range() {
        assert!(landmark_in_range([160.0, 0.0], [0.0, 0.0]));
        assert!(!landmark_in_range([120.0, 120.0], [0.0, 0.0]));
    }
}
