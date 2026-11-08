//! Summoned terrain (#293): the replicated Ice Walls and Earth Pillars of the shared world.
//! The zone owns position, health and lifetime; this module only draws what the latest
//! projection names, with a short rise and a crumble when an obstacle disappears.
use crate::{
    Draw, Material,
    batching::matrix,
    geometry::{Geometry, box_shape, compose, ry, scale, translate},
};
use mmorpg_core::{SummonedTerrainKind, SummonedView};
use std::{collections::BTreeMap, sync::Arc};

/// How long a new obstacle takes to rise (seconds); a cold first frame is already up.
const RISE_SECONDS: f64 = 0.25;
/// How long a gone obstacle's shards remain (seconds).
const CRUMBLE_SECONDS: f64 = 0.7;

fn radians(yaw: u16) -> f64 {
    f64::from(yaw) / 65_536.0 * std::f64::consts::TAU
}

fn appearance(kind: SummonedTerrainKind) -> ([f32; 3], [f32; 3], f32) {
    match kind {
        SummonedTerrainKind::IceWall => ([0.74, 0.88, 0.98], [0.08, 0.18, 0.28], 0.78),
        SummonedTerrainKind::EarthPillar => ([0.42, 0.33, 0.24], [0.0; 3], 1.0),
    }
}

#[derive(Clone, Copy)]
struct Seen {
    view: SummonedView,
    /// The rendered centre (metres).
    at: [f64; 3],
    risen: f64,
    gone: Option<f64>,
}

/// One client-local presenter of summoned terrain.
#[derive(Default)]
pub struct Summoned {
    cube: Option<Arc<Geometry>>,
    seen: BTreeMap<u32, Seen>,
    initialized: bool,
}

impl Summoned {
    fn cube(&mut self) -> Arc<Geometry> {
        Arc::clone(self.cube.get_or_insert_with(|| Arc::new(box_shape())))
    }

    /// Draws `views` (the latest projection), each given with its rendered centre in metres.
    /// The clock is cosmetic.
    #[must_use]
    pub fn frame(
        &mut self,
        views: &[(SummonedView, [f32; 3])],
        seconds: f64,
        reduced_motion: bool,
    ) -> Vec<Draw> {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        for &(view, at) in views {
            let at = at.map(f64::from);
            let risen = if self.initialized {
                seconds
            } else {
                f64::NEG_INFINITY
            };
            self.seen
                .entry(view.id)
                .and_modify(|seen| {
                    seen.view = view;
                    seen.at = at;
                    seen.gone = None;
                })
                .or_insert(Seen {
                    view,
                    at,
                    risen,
                    gone: None,
                });
        }
        self.initialized = true;
        let current: Vec<u32> = views.iter().map(|(view, _)| view.id).collect();
        for (id, seen) in &mut self.seen {
            if !current.contains(id) && seen.gone.is_none() {
                seen.gone = Some(seconds);
            }
        }
        self.seen.retain(|_, seen| {
            seen.gone
                .is_none_or(|gone| seconds - gone < CRUMBLE_SECONDS)
        });
        let cube = self.cube();
        let mut draws = Vec::new();
        for (id, seen) in &self.seen {
            let half = seen.view.kind.half_extents().map(|v| f64::from(v) / 100.0);
            let (color, emissive, opacity) = appearance(seen.view.kind);
            // Darker as it is worn down.
            let worn = f32::from(seen.view.health_percent) / 100.0;
            let color = color.map(|c| c * (0.55 + 0.45 * worn));
            let yaw = radians(seen.view.facing);
            let material = Material {
                color,
                emissive,
                opacity,
                unlit: false,
                double_sided: false,
                ground: None,
            };
            if let Some(gone) = seen.gone {
                if reduced_motion {
                    continue;
                }
                let age = (seconds - gone).max(0.0);
                for shard in 0..8u32 {
                    let angle = f64::from(shard) * 2.399;
                    let side = f64::from(shard) / 7.0 * 2.0 - 1.0;
                    let at = [
                        seen.at[0] + yaw.cos() * half[0] * side + angle.sin() * age * 1.6,
                        seen.at[1] - half[1] * 0.3 + age * (1.5 - 3.0 * age),
                        seen.at[2] - yaw.sin() * half[0] * side + angle.cos() * age * 1.6,
                    ];
                    draws.push(Draw {
                        id: format!("summoned-{id}-shard-{shard}"),
                        resource_key: "summoned:cube-v1".into(),
                        geometry: Arc::clone(&cube),
                        model: matrix(compose(&[
                            translate(at[0], at[1], at[2]),
                            ry(yaw + angle),
                            scale(0.16, 0.12, 0.16),
                        ])),
                        material: Material {
                            opacity: ((CRUMBLE_SECONDS - age) / 0.25).min(1.0) as f32 * opacity,
                            ..material
                        },
                    });
                }
                continue;
            }
            let age = (seconds - seen.risen).max(0.0);
            let rise = if reduced_motion || age >= RISE_SECONDS {
                1.0
            } else {
                age / RISE_SECONDS
            };
            let height = half[1] * rise;
            draws.push(Draw {
                id: format!("summoned-{id}-body"),
                resource_key: "summoned:cube-v1".into(),
                geometry: Arc::clone(&cube),
                model: matrix(compose(&[
                    translate(seen.at[0], seen.at[1] - half[1] + height, seen.at[2]),
                    ry(yaw),
                    scale(half[0], height, half[2]),
                ])),
                material,
            });
        }
        draws
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(id: u32, percent: u8) -> (SummonedView, [f32; 3]) {
        (
            SummonedView {
                id,
                kind: SummonedTerrainKind::IceWall,
                centre: [0, 0, 400],
                facing: 0,
                health_percent: percent,
            },
            [0.0, 1.5, 4.0],
        )
    }

    #[test]
    fn a_wall_rises_stands_and_crumbles_when_it_is_gone() {
        let mut presenter = Summoned::default();
        let cold = presenter.frame(&[wall(1, 100)], 1.0, false);
        assert_eq!(cold.len(), 1, "a cold frame shows it risen");
        let fresh = presenter.frame(&[wall(1, 100), wall(2, 100)], 2.0, false);
        assert_eq!(fresh.len(), 2);
        assert!(
            presenter
                .frame(&[wall(1, 40), wall(2, 100)], 3.0, false)
                .iter()
                .all(|draw| draw.id.ends_with("-body"))
        );
        let shards = presenter.frame(&[wall(2, 100)], 4.0, false);
        assert_eq!(
            shards
                .iter()
                .filter(|draw| draw.id.starts_with("summoned-1-shard"))
                .count(),
            8
        );
        let settled = presenter.frame(&[wall(2, 100)], 5.0, false);
        assert_eq!(settled.len(), 1, "shards fade after a moment");
    }

    #[test]
    fn reduced_motion_skips_the_crumble() {
        let mut presenter = Summoned::default();
        let _ = presenter.frame(&[wall(1, 100)], 1.0, true);
        assert!(presenter.frame(&[], 2.0, true).is_empty());
    }
}
