//! The character-selection stage: what the window shows before Enter World,
//! the native counterpart of the browser's selection stage. A floor, two
//! columns on plinths and a backdrop stand in afternoon light around the
//! previewed character; nothing of the zone is loaded or connected yet.
//!
//! The stage is presentation only. Its one-unit projection is local: the
//! previewed character standing at the origin, addressed to no real player,
//! never sent and never mistaken for the zone's (the window keeps it in its
//! own [`Presentation`], apart from the zone's).
use crate::{ClientError, camera::CameraView, presentation::Presentation, world::WorldScene};
use mmorpg_core::{
    DAY_TICKS, EntityFlags, EntityKind, EntitySnapshot, StaticCollider, TargetDetail, ViewerState,
    ZONE_START_TIME, ZoneContent, ZoneId, ZoneSnapshot,
};
use mmorpg_scenery::{PropKind, Scenery};
use std::{sync::Arc, time::Instant};

/// The stage's local viewer, never a real player's ID on the wire.
const VIEWER: u32 = 0;
/// The stage's own boxes; they visualise no zone collider.
const FIRST_BOX: u32 = 0x5747_0000;

/// The previewed character's yaw facing the stage camera, before the turntable's turn.
pub const FACING_CAMERA: f32 = std::f32::consts::PI;

/// The stage's fixed camera: close on the character, which stands a little left of centre
/// between the screen's panels, as on the browser's stage.
#[must_use]
pub fn camera(field_of_view_degrees: f32) -> CameraView {
    CameraView {
        field_of_view_degrees,
        eye: [-0.6, 1.4, -4.3],
        target: [-0.6, 0.85, 0.0],
    }
}

/// The stage's boxes in units (centimetres): feet of the character at the origin,
/// the camera before it looking along +Z, so the columns and backdrop stand beyond.
fn boxes() -> Vec<(StaticCollider, PropKind)> {
    let solid = |position, half_extents, kind| (position, half_extents, kind);
    [
        // A wide floor hiding the ground, then the round floor's two tiers, each just above
        // the last.
        solid([0, -14, 200], [900, 15, 900], PropKind::GrayboxWalkway),
        solid([0, -13, 60], [300, 15, 300], PropKind::GrayboxWalkway),
        solid([0, -11, 60], [200, 15, 200], PropKind::GrayboxWalkway),
        // Two columns on plinths and the backdrop wall.
        solid([-420, 35, 320], [85, 35, 85], PropKind::GrayboxWall),
        solid([420, 35, 320], [85, 35, 85], PropKind::GrayboxWall),
        solid([-420, 290, 320], [55, 220, 55], PropKind::GrayboxTower),
        solid([420, 290, 320], [55, 220, 55], PropKind::GrayboxTower),
        solid([0, 300, 560], [900, 300, 15], PropKind::GrayboxWall),
    ]
    .into_iter()
    .zip(FIRST_BOX..)
    .map(|((position, half_extents, kind), id)| {
        (
            StaticCollider {
                id,
                position,
                half_extents,
            },
            kind,
        )
    })
    .collect()
}

/// The stage scenery: flat ground and the stage's boxes, for `content`'s revision.
#[must_use]
pub fn scenery(content: &ZoneContent) -> Scenery {
    Scenery::graybox(content.revision(), &boxes(), &[], &[])
}

/// The stage's static geometry, cheap beside a zone's.
#[must_use]
pub fn world(scenery: &Scenery) -> WorldScene {
    WorldScene::new(scenery)
}

/// The tick at which the zone clock reads mid-afternoon, the browser stage's light.
fn afternoon() -> u64 {
    let day = u64::from(DAY_TICKS);
    let target = day * 15 / 24;
    (target + day - u64::from(ZONE_START_TIME) % day) % day
}

/// The stage's local projection: only the previewed character, at the origin.
fn projection(content: &ZoneContent, zone_id: ZoneId) -> ZoneSnapshot {
    ZoneSnapshot {
        content_revision: content.revision(),
        acknowledged_sequence: 0,
        viewer_id: VIEWER,
        zone_id,
        tick: afternoon(),
        viewer: ViewerState::default(),
        cooldowns: Vec::new(),
        auras: Vec::new(),
        target_of_target: None,
        target_detail: TargetDetail::default(),
        inventory_revision: 0,
        inventory: None,
        equipment: None,
        quests: None,
        loot: None,
        events: Vec::new(),
        chat: Vec::new(),
        entities: vec![EntitySnapshot {
            kind: EntityKind::Player,
            id: VIEWER,
            appearance: 0,
            // Projections place a unit by its body centre; its feet rest on the floor tiers.
            position: [0, mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1] + 3, 0],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        }],
        doors: Vec::new(),
        projectiles: Vec::new(),
        debris: Vec::new(),
        pieces: Vec::new(),
        summoned: Vec::new(),
        traps: Vec::new(),
        raid: None,
        party: None,
        depths_loot: None,
    }
}

/// The stage's own presentation of its local projection.
///
/// # Errors
/// The stage scenery and `content` disagree (they cannot: the scenery takes
/// `content`'s revision).
pub fn presentation(
    scenery: Scenery,
    content: Arc<ZoneContent>,
    zone_id: ZoneId,
    now: Instant,
) -> Result<Presentation, ClientError> {
    let snapshot = projection(&content, zone_id);
    let mut presentation = Presentation::new(VIEWER, scenery, content, now)?;
    presentation.push(snapshot, now)?;
    Ok(presentation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_presents_only_the_previewed_character_in_the_afternoon() {
        let content = mmorpg_core::content_for_zone(ZoneId::new(1));
        let scenery = scenery(&content);
        assert_eq!(scenery.props.len(), boxes().len());
        assert!(!scenery.greyhaven());
        let presentation = presentation(scenery, content, ZoneId::new(1), Instant::now()).unwrap();
        let latest = presentation.latest().unwrap();
        assert_eq!(latest.entities.len(), 1);
        let hour = crate::environment::zone_hour(latest.tick);
        assert!((hour - 15.0).abs() < 0.01, "{hour}");
    }
}
