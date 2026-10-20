//! Replicated modular structure stages and disposable cosmetic transitions (#306).
//! No local health, support, repair timer, debris collision or gameplay prediction.
use crate::{
    Draw, Material,
    batching::matrix,
    geometry::{Geometry, box_shape, compose, ry, rz, scale, translate},
};
use mmorpg_core::{PieceId, PieceStage, PieceView, StructurePiece};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy)]
struct Seen {
    stage: PieceStage,
    changed: f64,
}

/// One client-local presenter. Recreate it after a world/interest reset to snap to current state.
pub struct Structures {
    pieces: Vec<StructurePiece>,
    seen: BTreeMap<PieceId, Seen>,
    cube: Arc<Geometry>,
    initialized: bool,
}
impl Structures {
    #[must_use]
    pub fn new(pieces: &[StructurePiece]) -> Self {
        Self {
            pieces: pieces.to_vec(),
            seen: BTreeMap::new(),
            cube: Arc::new(box_shape()),
            initialized: false,
        }
    }

    /// Lift authored pieces and rubble by the same presentation relief as units.
    #[must_use]
    pub fn on_scenery(pieces: &[StructurePiece], scenery: &mmorpg_scenery::Scenery) -> Self {
        let mut pieces = pieces.to_vec();
        for piece in &mut pieces {
            piece.position[1] += scenery.height_at(piece.position[0], piece.position[2]);
            if let Some((position, _)) = &mut piece.rubble {
                position[1] += scenery.height_at(position[0], position[2]);
            }
        }
        Self::new(&pieces)
    }

    /// `views` is the latest complete projection (intact pieces are omitted). Clock is cosmetic.
    /// A cold first frame snaps; only observed state changes emit debris/rebuild motion.
    #[must_use]
    pub fn frame(
        &mut self,
        views: &[PieceView],
        seconds: f64,
        viewer: Option<[f64; 3]>,
        reduced_motion: bool,
    ) -> Vec<Draw> {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        let mut draws = Vec::new();
        for piece in &self.pieces {
            let stage = views
                .iter()
                .find(|v| v.piece == piece.id)
                .map_or(PieceStage::Intact, |v| v.stage);
            let entry = self.seen.entry(piece.id).or_insert(Seen {
                stage,
                changed: f64::NEG_INFINITY,
            });
            if entry.stage != stage {
                *entry = Seen {
                    stage,
                    changed: if self.initialized {
                        seconds
                    } else {
                        f64::NEG_INFINITY
                    },
                };
            }
            let age = (seconds - entry.changed).max(0.0);
            let p = piece.position.map(|v| f64::from(v) / 100.0);
            let h = piece.half_extents.map(|v| f64::from(v) / 100.0);
            let roof = h[1] < h[0].min(h[2]);
            let inside = roof
                && viewer.is_some_and(|v| {
                    (v[0] - p[0]).abs() < h[0] && (v[2] - p[2]).abs() < h[2] && v[1] < p[1]
                });
            let rebuilding = stage == PieceStage::Intact && age < 0.6 && !reduced_motion;
            if stage != PieceStage::Destroyed && !inside {
                // Authored timber courses: stable, pre-cut segments for every damage stage.
                for plank in 0..7 {
                    if stage == PieceStage::Crumbling && plank % 3 == 1 {
                        continue;
                    }
                    let f = (f64::from(plank) + 0.5) / 7.0;
                    let damaged = stage != PieceStage::Intact;
                    let half = if roof {
                        [h[0] / 7.0 * 0.95, h[1], h[2]]
                    } else {
                        [h[0], h[1] / 7.0 * 0.92, h[2]]
                    };
                    let mut at = p;
                    let axis = if roof { 0 } else { 1 };
                    at[axis] += (f * 2.0 - 1.0) * h[axis];
                    if rebuilding {
                        at[1] -= (1.0 - age / 0.6) * (0.3 + f * 0.3);
                    }
                    let angle = if damaged {
                        (f - 0.5)
                            * if stage == PieceStage::Crumbling {
                                0.13
                            } else {
                                0.035
                            }
                    } else {
                        0.0
                    };
                    draws.push(self.draw(
                        piece.id,
                        plank,
                        at,
                        half,
                        angle,
                        if damaged {
                            [0.25, 0.16, 0.10]
                        } else {
                            [0.43, 0.29, 0.16]
                        },
                        1.0,
                    ));
                }
            }
            if stage == PieceStage::Destroyed {
                // The one gameplay rubble blocker is drawn exactly where content specifies it.
                if let Some((position, extents)) = piece.rubble {
                    draws.push(self.draw(
                        piece.id,
                        20,
                        position.map(|v| f64::from(v) / 100.0),
                        extents.map(|v| f64::from(v) / 100.0),
                        0.0,
                        [0.25, 0.18, 0.12],
                        1.0,
                    ));
                }
                if age < 2.0 && !reduced_motion {
                    for shard in 0..8 {
                        let angle = f64::from(shard) * 2.399;
                        let travel = age.min(0.8);
                        let ground = p[1] - h[1];
                        let at = [
                            p[0] + angle.cos() * travel * 1.4,
                            (p[1] + travel * 1.5 - 4.0 * travel * travel).max(ground + 0.08),
                            p[2] + angle.sin() * travel * 1.4,
                        ];
                        draws.push(self.draw(
                            piece.id,
                            30 + shard,
                            at,
                            [0.22, 0.08, 0.10],
                            angle + travel * 3.0,
                            [0.36, 0.23, 0.13],
                            ((2.0 - age) / 0.4).min(1.0) as f32,
                        ));
                    }
                }
            }
        }
        self.initialized = true;
        draws
    }
    fn draw(
        &self,
        piece: PieceId,
        part: u32,
        at: [f64; 3],
        half: [f64; 3],
        angle: f64,
        color: [f32; 3],
        opacity: f32,
    ) -> Draw {
        Draw {
            id: format!("structure:{}:{part}", piece.get()),
            resource_key: "structure:timber-cube-v1".into(),
            geometry: Arc::clone(&self.cube),
            model: matrix(compose(&[
                translate(at[0], at[1], at[2]),
                ry(angle * 0.3),
                rz(angle),
                scale(half[0], half[1], half[2]),
            ])),
            material: Material {
                color,
                emissive: [0.0; 3],
                opacity,
                unlit: false,
                double_sided: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_changes_drive_bounded_debris_and_rebuild_without_cold_replay() {
        let pieces = mmorpg_core::greyhaven_vale::watch_post();
        let destroyed = [PieceView {
            piece: pieces[0].id,
            stage: PieceStage::Destroyed,
        }];
        let mut presenter = Structures::new(&pieces);
        let initial = presenter.frame(&destroyed, 0.0, None, false);
        assert!(
            initial
                .iter()
                .all(|draw| !draw.id.starts_with("structure:1:3")),
            "cold restore does not replay debris"
        );
        let _ = presenter.frame(&[], 1.0, None, false);
        let debris = presenter.frame(&destroyed, 2.0, None, false);
        assert_eq!(
            debris
                .iter()
                .filter(|draw| draw.id.starts_with("structure:1:3"))
                .count(),
            8
        );
        let settled = presenter.frame(&destroyed, 5.0, None, false);
        assert!(
            settled
                .iter()
                .all(|draw| !draw.id.starts_with("structure:1:3"))
        );
        assert!(
            settled.iter().any(|draw| draw.id == "structure:1:20"),
            "authoritative rubble remains"
        );
        let restoring = presenter.frame(&[], 6.0, None, false);
        let restored = presenter.frame(&[], 7.0, None, false);
        assert_ne!(restoring[0].model, restored[0].model);
        assert!(
            restored
                .iter()
                .all(|draw| Arc::ptr_eq(&draw.geometry, &restored[0].geometry)),
            "all frames share immutable geometry"
        );
    }
    #[test]
    fn missing_deck_stays_missing_and_roof_cutaway_only_hides_standing_deck() {
        let pieces = mmorpg_core::greyhaven_vale::watch_post();
        let mut presenter = Structures::new(&pieces);
        let viewer = Some([7.0, 0.0, -72.0]);
        let inside = presenter.frame(&[], 0.0, viewer, true);
        assert!(
            inside
                .iter()
                .all(|draw| !draw.id.starts_with("structure:3:"))
        );
        let absent = presenter.frame(
            &[PieceView {
                piece: PieceId::new(3),
                stage: PieceStage::Destroyed,
            }],
            1.0,
            None,
            true,
        );
        assert!(
            absent
                .iter()
                .all(|draw| !draw.id.starts_with("structure:3:"))
        );
    }
}
