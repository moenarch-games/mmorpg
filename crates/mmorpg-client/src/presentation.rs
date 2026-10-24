//! Snapshot-only presentation. Local input never changes authoritative positions.
//! Units stand on the shared presentation relief: a rendered position is the
//! physics position plus `Scenery::height_at` at its XZ, never fed back.
//!
//! Units are low-poly models (`unit_models`) fitted to their collision box:
//! humanoid players and NPCs, quadruped or humanoid creatures by template
//! family, and a box with a nose for unknown templates. Creatures are coloured
//! by disposition (hostile red-ish, neutral yellow-ish) and shaded by family,
//! NPCs green-ish; corpses lie flat, and the viewer's target stands on a
//! marker. Names come from the zone content.
mod skinned;

use crate::{
    ClientError,
    camera::CameraView,
    unit_models::{Gait, MODEL_BOX_BUDGET, Placement, UnitModel, append_unit},
};
use mmorpg_core::{
    CreatureFamily, CreatureTemplateId, EntityKind, EntityRef, EntitySnapshot, NpcRole,
    PLAYER_HALF_EXTENTS_UNITS, TICK_HZ, UNITS_PER_METRE, ZoneContent, ZoneSnapshot,
};
use mmorpg_presentation::units::{
    FrameInput, SampledUnit, UnitPresenter, Viewer, imported::Kind, player,
};
use mmorpg_scenery::Scenery;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

/// Longest frame gap the walk cycle advances over.
const MAX_GAIT_STEP_SECONDS: f32 = 0.25;
const SELF_COLOR: [f32; 3] = [0.95, 0.75, 0.25];
const PLAYER_COLOR: [f32; 3] = [0.3, 0.6, 0.95];
const FRIENDLY_COLOR: [f32; 3] = [0.3, 0.62, 0.36];
const GUARD_COLOR: [f32; 3] = [0.22, 0.52, 0.3];
const TAPPED_COLOR: [f32; 3] = [0.5, 0.5, 0.5];
const TARGET_MARKER_COLOR: [f32; 3] = [0.95, 0.82, 0.35];
const HEALTH_BAR_WIDTH: f32 = 1.0;
const HEALTH_BAR_BACKGROUND: [f32; 3] = [0.12, 0.12, 0.14];
const HEALTH_BAR_FILL: [f32; 3] = [0.18, 0.9, 0.25];

/// One model (at most [`MODEL_BOX_BUDGET`] boxes) and two health-bar boxes per
/// projected unit, plus one target marker.
pub const MAX_SCENE_BOXES: usize = (MODEL_BOX_BUDGET + 2) * mmorpg_core::MAX_VISIBLE_ENTITIES + 1;

/// A box rotated by `yaw` radians about +Y (0 keeps local +Z on world +Z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneBox {
    pub position: [f32; 3],
    pub size: [f32; 3],
    pub color: [f32; 3],
    pub yaw: f32,
}

/// Interpolated presentation pose of one unit, in metres and radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerPose {
    /// Physics position lifted by the relief height at its XZ.
    pub position: [f32; 3],
    /// World yaw convention (0 faces +Z, increasing toward +X), within one turn.
    pub yaw: f32,
}

/// An interpolated unit with the record it was sampled from.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitPose {
    pub pose: PlayerPose,
    pub record: EntitySnapshot,
}

pub struct Presentation {
    player_id: u32,
    scenery: Scenery,
    content: Arc<ZoneContent>,
    history: VecDeque<ZoneSnapshot>,
    latest_received: Instant,
    /// Walk-cycle phase and eased swing per visible unit.
    gaits: BTreeMap<EntityRef, Gait>,
    /// Time of the previous `scene` call.
    last_scene: Instant,
    picked_bodies: Vec<(EntityRef, SceneBox)>,
    picked_draws: Vec<(EntityRef, crate::world::SceneDraw)>,
    /// Shared procedural units, poses, spell visuals and imported-model clip choice (#184).
    units_presenter: UnitPresenter,
    /// Native meshes of the shared draws, uploaded once per resource.
    lowering: crate::world::Lowering,
    headwear: Option<player::Hat>,
    /// Character-selection preview of the viewer: class, sex and turntable yaw.
    preview: Option<(mmorpg_core::ClassChoice, f32)>,
    /// Nameplate anchors above each presented unit's head, from the last draw.
    labels: Vec<crate::ui::world_labels::UnitLabel>,
    reduced_motion: bool,
    origin: Instant,
    last_draw: Instant,
    camera_framed: bool,
    /// The walk-in inn's door swing and interior look (#158).
    inn: mmorpg_presentation::buildings::Inn,
    last_inn: Instant,
    /// The Sunken Shrine's kit modules (#431), placed once.
    shrine: Vec<crate::world::SceneDraw>,
    /// The Outpost kit buildings' modules (#420), placed once.
    outpost: Vec<crate::world::SceneDraw>,
}

impl Presentation {
    /// Presents snapshots of the content revision the scenery was built from;
    /// `content` names and sizes the units of that revision.
    pub fn new(
        player_id: u32,
        scenery: Scenery,
        content: Arc<ZoneContent>,
        now: Instant,
    ) -> Result<Self, ClientError> {
        if content.revision() != scenery.content_revision {
            return Err("scenery and zone content revisions differ".into());
        }
        mmorpg_presentation::units::archer::validate()?;
        let inn = mmorpg_presentation::buildings::Inn::new(&scenery);
        let shrine = mmorpg_presentation::shrine::parts(&scenery)
            .iter()
            .flat_map(|part| {
                skinned::shrine_draws(part.key)
                    .iter()
                    .map(|draw| crate::world::SceneDraw {
                        model: three_d_animation::Mat4 {
                            elements: part.model,
                        },
                        ..draw.clone()
                    })
            })
            .collect();
        let outpost = mmorpg_presentation::outpost_kit::parts(&scenery)
            .iter()
            .flat_map(|part| {
                skinned::outpost_draws(part.entry)
                    .iter()
                    .map(|draw| crate::world::SceneDraw {
                        model: three_d_animation::Mat4 {
                            elements: part.model,
                        },
                        ..draw.clone()
                    })
            })
            .collect();
        let mut units_presenter =
            UnitPresenter::new(player_id, Arc::clone(&content), Arc::new(scenery.clone()));
        for kind in Kind::ALL {
            units_presenter.model_ready(kind, skinned::asset(kind).model.clip_durations());
        }
        Ok(Self {
            player_id,
            scenery,
            content,
            history: VecDeque::new(),
            latest_received: now,
            gaits: BTreeMap::new(),
            last_scene: now,
            picked_bodies: Vec::new(),
            picked_draws: Vec::new(),
            units_presenter,
            lowering: crate::world::Lowering::default(),
            headwear: None,
            preview: None,
            labels: Vec::new(),
            reduced_motion: false,
            origin: now,
            last_draw: now,
            camera_framed: false,
            inn,
            last_inn: now,
            shrine,
            outpost,
        })
    }

    /// Start a new connection epoch without blending with obsolete samples.
    pub fn reset(&mut self, now: Instant) {
        self.camera_framed = false;
        self.picked_bodies.clear();
        self.picked_draws.clear();
        self.labels.clear();
        self.gaits.clear();
        self.units_presenter.reset();
        self.lowering = crate::world::Lowering::default();
        self.last_draw = now;
        self.last_scene = now;
        self.history.clear();
        self.latest_received = now;
    }

    pub fn push(&mut self, snapshot: ZoneSnapshot, now: Instant) -> Result<bool, ClientError> {
        if snapshot.content_revision != self.scenery.content_revision {
            return Err("snapshot content revision mismatch".into());
        }
        if snapshot.viewer_id != self.player_id {
            return Err("snapshot is addressed to another player".into());
        }
        if snapshot.entities.len() > mmorpg_core::MAX_VISIBLE_ENTITIES {
            return Err("snapshot exceeds visible entity capacity".into());
        }
        let mut entities = std::collections::BTreeSet::new();
        if snapshot
            .entities
            .iter()
            .any(|entity| !entities.insert((entity.kind, entity.id)))
        {
            return Err("duplicate entity in snapshot".into());
        }
        if let Some(latest) = self.history.back() {
            if snapshot.zone_id != latest.zone_id {
                return Err("zone changed without resetting presentation".into());
            }
            if snapshot.tick <= latest.tick {
                return Ok(false);
            }
            if snapshot.tick - latest.tick > u64::from(TICK_HZ) * 30 {
                self.history.clear();
            }
        }
        self.units_presenter.push(
            &snapshot,
            now.saturating_duration_since(self.origin).as_secs_f64(),
        );
        self.latest_received = now;
        self.history.push_back(snapshot);
        if self.history.len() > 32 {
            self.history.pop_front();
        }
        Ok(true)
    }

    /// The newest projection, for input decisions and status text.
    #[must_use]
    pub fn latest(&self) -> Option<&ZoneSnapshot> {
        self.history.back()
    }

    /// Two ticks of interpolation delay; packet loss holds the latest pose.
    /// Facing interpolates along the shorter arc. Units appear and vanish at
    /// sample ticks.
    #[must_use]
    pub fn units(&self, now: Instant) -> BTreeMap<EntityRef, UnitPose> {
        let Some(latest) = self.history.back() else {
            return BTreeMap::new();
        };
        let elapsed = now
            .saturating_duration_since(self.latest_received)
            .as_secs_f64()
            * f64::from(TICK_HZ);
        let delay = (2.0 - elapsed).max(0.0);
        let mut before = &self.history[0];
        for after in self.history.iter().skip(1) {
            // Subtract integer ticks before conversion, preserving precision at u64::MAX.
            let after_age = (latest.tick - after.tick) as f64;
            if after_age < delay {
                let before_age = (latest.tick - before.tick) as f64;
                let alpha =
                    ((before_age - delay) / (before_age - after_age)).clamp(0.0, 1.0) as f32;
                return before
                    .entities
                    .iter()
                    .map(|unit| {
                        let pose = self.pose(unit);
                        let next = after
                            .entities
                            .iter()
                            .find(|next| (next.kind, next.id) == (unit.kind, unit.id));
                        let pose = next.map_or(pose, |next| {
                            let target = self.rendered(next.position);
                            // Wrapping u16 difference, read as signed, is the shorter arc.
                            let arc = f32::from(next.facing.wrapping_sub(unit.facing) as i16);
                            PlayerPose {
                                position: std::array::from_fn(|axis| {
                                    pose.position[axis]
                                        + (target[axis] - pose.position[axis]) * alpha
                                }),
                                yaw: yaw_radians(f32::from(unit.facing) + arc * alpha),
                            }
                        });
                        (
                            unit.entity(),
                            UnitPose {
                                pose,
                                record: unit.clone(),
                            },
                        )
                    })
                    .collect();
            }
            before = after;
        }
        before
            .entities
            .iter()
            .map(|unit| {
                (
                    unit.entity(),
                    UnitPose {
                        pose: self.pose(unit),
                        record: unit.clone(),
                    },
                )
            })
            .collect()
    }

    /// Interpolated poses of the visible players.
    #[must_use]
    pub fn players(&self, now: Instant) -> BTreeMap<u32, PlayerPose> {
        self.units(now)
            .into_iter()
            .filter_map(|(entity, unit)| match entity {
                EntityRef::Player(id) => Some((id, unit.pose)),
                EntityRef::Creature(_) | EntityRef::Npc(_) => None,
            })
            .collect()
    }

    /// Per-frame unit models, living-unit health bars, plus a marker under
    /// the viewer's target. Static scenery is uploaded once (`world::WorldScene`).
    #[must_use]
    pub fn scene(&mut self, now: Instant, view: CameraView) -> Vec<SceneBox> {
        let target = self.latest().and_then(|latest| latest.viewer.target);
        let bar_yaw = (view.target[0] - view.eye[0]).atan2(view.target[2] - view.eye[2]);
        // A stall must not fast-forward the limbs.
        let dt = now
            .saturating_duration_since(self.last_scene)
            .as_secs_f32()
            .min(MAX_GAIT_STEP_SECONDS);
        self.last_scene = self.last_scene.max(now);
        let units = self.units(now);
        self.gaits.retain(|entity, _| units.contains_key(entity));
        let mut boxes = Vec::new();
        self.picked_bodies.clear();
        for (entity, unit) in units {
            let (color, half) = self.unit_look(entity, &unit.record);
            let size = half.map(|value| value * 2.0);
            let pose = unit.pose;
            // Rendered positions are body centres; the feet are half a body lower.
            let feet = pose.position[1] - half[1];
            if target == Some(entity) {
                boxes.push(SceneBox {
                    position: [pose.position[0], feet + 0.02, pose.position[2]],
                    size: [size[0] + 0.7, 0.04, size[2] + 0.7],
                    color: TARGET_MARKER_COLOR,
                    yaw: pose.yaw,
                });
            }
            let corpse = unit.record.flags.dead && entity.kind() == EntityKind::Creature;
            // Horizontal speed in m/s of the sample this pose interpolates from.
            let [vx, _, vz] = unit.record.velocity;
            let speed = (vx as f32).hypot(vz as f32) / UNITS_PER_METRE as f32 * TICK_HZ as f32;
            let state = self.gaits.entry(entity).or_insert(Gait {
                phase: (unit.record.id % 64) as f32,
                swing: 0.0,
            });
            *state = state.step(speed, dt);
            let gait = *state;
            let start = boxes.len();
            append_unit(
                &mut boxes,
                self.unit_model(entity, &unit.record),
                color,
                Placement {
                    centre: pose.position,
                    half,
                    yaw: pose.yaw,
                },
                gait,
                corpse,
            );
            self.picked_bodies
                .extend(boxes[start..].iter().map(|part| (entity, *part)));
            if !unit.record.flags.dead && entity.kind() != EntityKind::Npc {
                append_health_bar(
                    &mut boxes,
                    [
                        pose.position[0],
                        pose.position[1] + half[1] + 0.3,
                        pose.position[2],
                    ],
                    unit.record.health_percent,
                    bar_yaw,
                );
            }
        }
        boxes
    }

    /// The same starter class appearance selected by browser character creation.
    /// Later explicit headwear customization, including none, takes precedence.
    pub fn set_default_appearance(&mut self, class: mmorpg_core::PlayerClass) {
        use mmorpg_core::PlayerClass;
        self.headwear = Some(match class {
            PlayerClass::Warden => player::Hat::Helm,
            PlayerClass::Ranger => player::Hat::Cap,
            PlayerClass::Arcanist
            | PlayerClass::Necromancer
            | PlayerClass::Rogue
            | PlayerClass::Cleric
            | PlayerClass::Druid => player::Hat::Hood,
        });
    }

    /// Current headwear is local presentation customization; unknown package styles fail closed.
    pub fn set_headwear(&mut self, headwear: Option<&str>) -> Result<(), ClientError> {
        self.headwear = match headwear {
            None => None,
            Some("ironcrest-helm") => Some(player::Hat::Helm),
            Some("ranger-cap") => Some(player::Hat::Cap),
            Some("wayfarer-hood") => Some(player::Hat::Hood),
            Some(_) => return Err("unknown headwear style".into()),
        };
        Ok(())
    }

    /// While a character is being chosen, the viewer shows the previewed class
    /// and sex turned to `yaw` (presentation only; the zone's facing is unchanged).
    pub fn set_preview(&mut self, preview: Option<(mmorpg_core::ClassChoice, f32)>) {
        self.preview = preview;
    }

    /// The viewer's class and sex as presented: the preview, else the zone's choice.
    fn viewer_choice(&self, snapshot: &ZoneSnapshot) -> Option<mmorpg_core::ClassChoice> {
        self.preview
            .map(|(choice, _)| choice)
            .or(snapshot.viewer.class)
    }

    /// Where each presented unit's nameplate hangs, from the last unit draw.
    #[must_use]
    pub fn labels(&self) -> &[crate::ui::world_labels::UnitLabel] {
        &self.labels
    }

    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
    }

    /// Actual submitted unit bodies, excluding health bars, target markers and effects.
    #[must_use]
    pub fn picked_draws(&self) -> &[(EntityRef, crate::world::SceneDraw)] {
        &self.picked_draws
    }

    /// The same CPU geometry/material/model payload consumed by rendering and picking.
    #[must_use]
    pub fn unit_draws(&mut self, now: Instant, view: CameraView) -> Vec<crate::world::SceneDraw> {
        let Some(snapshot) = self.latest().cloned() else {
            self.picked_draws.clear();
            self.labels.clear();
            return Vec::new();
        };
        let dt = now
            .saturating_duration_since(self.last_draw)
            .as_secs_f64()
            .min(0.25);
        self.last_draw = self.last_draw.max(now);
        let seconds = now.saturating_duration_since(self.origin).as_secs_f64();
        let units: Vec<_> = self
            .units(now)
            .into_values()
            .map(|unit| SampledUnit {
                record: unit.record,
                centre: unit.pose.position,
                yaw: unit.pose.yaw,
            })
            .collect();
        let frame = self.units_presenter.frame(&FrameInput {
            snapshot: &snapshot,
            units: &units,
            viewer: Viewer {
                choice: self.viewer_choice(&snapshot),
                headwear: self.headwear,
                preview_yaw: self.preview.map(|(_, yaw)| yaw),
            },
            seconds,
            dt,
            reduced_motion: self.reduced_motion,
            bar_yaw: (view.target[0] - view.eye[0]).atan2(view.target[2] - view.eye[2]),
        });
        let mut draws: Vec<_> = frame
            .draws
            .iter()
            .map(|draw| self.lowering.draw(draw))
            .collect();
        self.picked_draws.clear();
        self.labels.clear();
        for unit in &frame.units {
            self.picked_draws.extend(
                draws[unit.body.clone()]
                    .iter()
                    .cloned()
                    .map(|draw| (unit.entity, draw)),
            );
        }
        for imported in &frame.imported {
            let asset = &skinned::asset(imported.kind).model;
            let placement = asset.placement(imported.root, imported.height);
            // The viewer's chosen headwear (#374): an authored item on `head.armor`, the
            // Ranger's own hood, or none; everyone else keeps the bundle's hooded default.
            let hat = imported.headwear.map(|look| look.hat);
            let hooded = matches!(hat, None | Some(Some(player::Hat::Hood)));
            let hidden: &[String] =
                if imported.kind == mmorpg_presentation::units::imported::Kind::Ranger && !hooded {
                    skinned::hood_nodes()
                } else {
                    &[]
                };
            let mut body = asset.draws(
                &imported.id,
                imported.root,
                imported.height,
                imported.clip,
                imported.time,
                imported.tint,
                hidden,
            );
            if imported.kind == mmorpg_presentation::units::imported::Kind::Ranger
                && hat.is_some_and(|hat| !skinned::hides_hair(hat))
            {
                body.extend(skinned::Equipment::hair().draws(
                    &format!("{}-hair", imported.id),
                    asset,
                    placement,
                    imported.clip,
                    imported.time,
                    imported.tint,
                ));
            }
            if let Some(items) = hat.flatten().and_then(skinned::Equipment::headwear) {
                body.extend(items.draws(
                    &format!("{}-head", imported.id),
                    asset,
                    placement,
                    imported.clip,
                    imported.time,
                    imported.tint,
                ));
            }
            if let Some(weapon) = imported.loadout.and_then(skinned::Equipment::loadout) {
                body.extend(weapon.draws(
                    &imported.id,
                    asset,
                    placement,
                    imported.clip,
                    imported.time,
                    imported.tint,
                ));
            }
            let head = asset
                .head(imported.clip, imported.time)
                .map(|(centre, radius)| {
                    let m = placement.elements;
                    let p: [f64; 3] = std::array::from_fn(|r| {
                        f64::from(
                            m[r] * centre[0]
                                + m[4 + r] * centre[1]
                                + m[8 + r] * centre[2]
                                + m[12 + r],
                        )
                    });
                    let scale = f64::from(m[0].hypot(m[1]).hypot(m[2]));
                    (p, f64::from(radius) * scale)
                });
            if let (Some(unit), Some((p, radius))) = (
                frame
                    .units
                    .iter()
                    .find(|unit| unit.entity == imported.entity),
                head,
            ) {
                // Above the posed head, or the health bar when that is higher.
                self.labels.push(crate::ui::world_labels::UnitLabel {
                    entity: unit.entity,
                    anchor: [
                        unit.label[0],
                        unit.label[1].max((p[1] + radius) as f32 + 0.15),
                        unit.label[2],
                    ],
                });
            }
            self.picked_draws
                .extend(body.iter().cloned().map(|draw| (imported.entity, draw)));
            draws.extend(body);
        }
        self.lowering.retain_used(&draws);
        for unit in &frame.units {
            if !self.labels.iter().any(|label| label.entity == unit.entity) {
                self.labels.push(crate::ui::world_labels::UnitLabel {
                    entity: unit.entity,
                    anchor: unit.label,
                });
            }
        }
        draws
    }

    /// Sample the static world and units with one presentation clock.
    #[must_use]
    pub fn frame(
        &mut self,
        world: &crate::world::WorldScene,
        now: Instant,
        view: CameraView,
    ) -> crate::world::WorldFrame {
        let viewer = self
            .players(now)
            .get(&self.player_id)
            .map(|pose| pose.position.map(f64::from));
        let mut frame = world.frame(
            now.saturating_duration_since(self.origin).as_secs_f64(),
            !self.reduced_motion,
            view.eye,
            viewer,
        );
        // The sky shows the zone's time of day; clouds keep drifting on the client clock.
        if let Some(mut tick) = self.latest().map(|snapshot| snapshot.tick) {
            // Reduced motion steps the sky once per zone hour instead of drifting.
            if self.reduced_motion {
                let hour = u64::from(mmorpg_core::HOUR_TICKS);
                tick -= (tick + u64::from(mmorpg_core::ZONE_START_TIME)) % hour;
            }
            let clouds = frame.environment.cloud_offset;
            frame.environment = crate::environment::Environment::at(
                crate::environment::zone_sky_seconds(tick),
                true,
            );
            frame.environment.cloud_offset = clouds;
        }
        frame.draws.extend(self.unit_draws(now, view));
        self.append_inn(&mut frame.draws, now);
        frame.draws.extend(self.shrine.iter().cloned());
        frame.draws.extend(self.outpost.iter().cloned());
        self.append_ambient(&mut frame.draws, now);
        frame
    }

    /// Greyhaven's cosmetic cat, dog and rodent (#419), skinned on their authored loops.
    fn append_ambient(&self, draws: &mut Vec<crate::world::SceneDraw>, now: Instant) {
        let seconds = now.saturating_duration_since(self.origin).as_secs_f64();
        for pose in
            mmorpg_presentation::ambient::poses(&self.scenery, seconds, !self.reduced_motion)
        {
            let Some((model, clips)) = skinned::ambient_model(pose.key) else {
                continue;
            };
            let clip = mmorpg_presentation::ambient::CLIPS[pose.clip];
            let length = f64::from(clips.get(clip).copied().unwrap_or(0.));
            #[allow(clippy::cast_possible_truncation)]
            let time = if length > 0. {
                pose.time.rem_euclid(length) as f32
            } else {
                0.
            };
            draws.extend(model.draws_placed(
                pose.key,
                three_d_animation::Mat4 {
                    elements: pose.model,
                },
                clip,
                time,
                None,
            ));
        }
    }

    /// The inn's bundle entries with the door at its eased swing; roof and ceiling are hidden
    /// while the viewer stands inside (#158).
    fn append_inn(&mut self, draws: &mut Vec<crate::world::SceneDraw>, now: Instant) {
        let dt = now.saturating_duration_since(self.last_inn).as_secs_f64();
        self.last_inn = self.last_inn.max(now);
        let open = self.door_open(mmorpg_presentation::buildings::INN_DOOR);
        let viewer = self
            .players(now)
            .get(&self.player_id)
            .map(|pose| pose.position.map(f64::from));
        let parts = self.inn.frame(open, viewer, dt, self.reduced_motion);
        for (part, (_, entry)) in parts.iter().zip(skinned::inn_draws()) {
            if !part.visible {
                continue;
            }
            draws.extend(entry.iter().map(|draw| crate::world::SceneDraw {
                model: three_d_animation::Mat4 {
                    elements: part.model,
                },
                ..draw.clone()
            }));
        }
    }

    /// Whether the latest snapshot shows `door` open.
    #[must_use]
    pub fn door_open(&self, door: mmorpg_core::DoorId) -> bool {
        self.latest()
            .is_some_and(|s| s.doors.iter().any(|view| view.door == door && view.open))
    }

    /// The `UseDoor` toggling the nearest content door the viewer can reach, if any.
    #[must_use]
    pub fn reachable_door(&self) -> Option<mmorpg_core::ZoneCommand> {
        let snapshot = self.latest()?;
        let feet = snapshot
            .entities
            .iter()
            .find(|e| e.entity() == EntityRef::Player(self.player_id))?
            .position;
        let door = self
            .content
            .doors()
            .iter()
            .find(|door| door.within_reach(feet))?;
        Some(self.toggle_door(door.id))
    }

    /// The `Repair` of the first damaged destructible piece (#305) the viewer can reach, if any.
    #[must_use]
    pub fn reachable_repair(&self) -> Option<mmorpg_core::ZoneCommand> {
        let snapshot = self.latest()?;
        let feet = snapshot
            .entities
            .iter()
            .find(|e| e.entity() == EntityRef::Player(self.player_id))?
            .position;
        let piece = snapshot.pieces.iter().find(|view| {
            self.content
                .pieces()
                .iter()
                .any(|piece| piece.id == view.piece && piece.within_repair_reach(feet))
        })?;
        Some(mmorpg_core::ZoneCommand::Repair {
            piece: Some(piece.piece),
        })
    }

    /// The `UseDoor` that flips `door` from its latest presented state.
    #[must_use]
    pub fn toggle_door(&self, door: mmorpg_core::DoorId) -> mmorpg_core::ZoneCommand {
        mmorpg_core::ZoneCommand::UseDoor {
            door,
            open: !self.door_open(door),
        }
    }

    /// Body geometry from the last rendered scene, with MMO identity retained.
    #[must_use]
    pub fn picked_bodies(&self) -> &[(EntityRef, SceneBox)] {
        &self.picked_bodies
    }

    #[must_use]
    pub fn content(&self) -> &ZoneContent {
        &self.content
    }

    #[must_use]
    pub fn camera_target(&self, now: Instant) -> [f32; 3] {
        self.players(now)
            .get(&self.player_id)
            .map_or([0.0, 0.5, 0.0], |pose| pose.position)
    }

    /// Frame the first actual viewer once per session, then preserve user orbit.
    /// Only scenery-backed structural colliders participate in startup framing.
    pub fn camera_view(
        &mut self,
        camera: &mut crate::camera::OrbitCamera,
        world: &crate::world::WorldScene,
        now: Instant,
    ) -> crate::camera::CameraView {
        let viewer = self.players(now).get(&self.player_id).copied();
        let focus = viewer.map_or([0.0, 0.5, 0.0], |pose| pose.position);
        let ground = |x, z| world.surface_y(x, z);
        if !self.camera_framed
            && let Some(viewer) = viewer
        {
            self.camera_framed = true;
            let colliders: Vec<_> = self
                .scenery
                .props
                .iter()
                .filter_map(|prop| {
                    let id = prop.collider?;
                    self.content
                        .definition()
                        .colliders()
                        .iter()
                        .find(|c| c.id == id)
                })
                .collect();
            let mut clear = false;
            for step in [0, 2, -2, 1, -1, 3, -3, 4] {
                camera.look_along(viewer.yaw + step as f32 * std::f32::consts::FRAC_PI_4);
                let view = camera.view_above_ground(focus, ground);
                let blocked = [0.35, 0.6, 0.8, 1.0].into_iter().any(|t| {
                    colliders.iter().any(|c| {
                        (0..3).all(|axis| {
                            let point =
                                view.target[axis] + (view.eye[axis] - view.target[axis]) * t;
                            let centre = c.position[axis] as f32 / 100.0;
                            let half = c.half_extents[axis] as f32 / 100.0;
                            let low = centre - half - if axis == 1 { 0.0 } else { 0.4 };
                            let high = centre + half + 0.4;
                            point >= low && point <= high
                        })
                    })
                });
                if !blocked {
                    clear = true;
                    break;
                }
            }
            if !clear {
                camera.look_along(viewer.yaw);
            }
        }
        camera.view_above_ground(focus, ground)
    }

    #[must_use]
    pub fn is_stalled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.latest_received) > Duration::from_secs(1)
    }

    /// Tab targeting: the nearest living attackable creature in the latest
    /// projection; while one of them is selected, the next one outward.
    #[must_use]
    pub fn next_tab_target(&self) -> Option<EntityRef> {
        let latest = self.latest()?;
        let viewer = latest.entities.first()?;
        let mut candidates: Vec<_> = latest
            .entities
            .iter()
            .filter(|entity| entity.kind == EntityKind::Creature && entity.flags.attackable)
            .map(|entity| {
                let dx = i64::from(entity.position[0] - viewer.position[0]);
                let dz = i64::from(entity.position[2] - viewer.position[2]);
                (dx * dx + dz * dz, entity.entity())
            })
            .collect();
        candidates.sort_unstable();
        let current = candidates
            .iter()
            .position(|(_, entity)| Some(*entity) == latest.viewer.target);
        let next = current.map_or(0, |index| (index + 1) % candidates.len());
        candidates.get(next).map(|(_, entity)| *entity)
    }

    /// A unit's display name from the zone content; "you" for the viewer.
    #[must_use]
    pub fn unit_name(&self, entity: EntityRef) -> String {
        match entity {
            EntityRef::Player(id) if id == self.player_id => "you".into(),
            EntityRef::Player(id) => format!("Player {id}"),
            EntityRef::Npc(id) => self
                .content
                .npc(id)
                .map_or_else(|| format!("NPC {}", id.get()), |npc| npc.name.clone()),
            EntityRef::Creature(_) => self
                .latest()
                .and_then(|latest| latest.entities.iter().find(|unit| unit.entity() == entity))
                .and_then(|unit| {
                    self.content
                        .creature_template(CreatureTemplateId::new(unit.appearance))
                })
                .map_or_else(|| "a creature".into(), |template| template.name.clone()),
        }
    }

    /// The viewer's health, combat state and target, or how to release a
    /// dead spirit; `None` before the first projection.
    #[must_use]
    pub fn status(&self) -> Option<String> {
        let latest = self.latest()?;
        let viewer = latest.viewer;
        if viewer.dead {
            return Some("dead — R releases your spirit".into());
        }
        let mut parts = vec![format!("HP {}/{}", viewer.health, viewer.max_health)];
        if let Some(resource) = viewer.resource {
            parts.push(format!(
                "{} {}/{}",
                resource.kind.name(),
                resource.value,
                resource.max
            ));
        }
        if let Some(cast) = viewer.cast {
            let name = mmorpg_core::ability_by_id(cast.ability).map_or("ability", |a| a.name);
            parts.push(format!("casting {name} {}/{}", cast.elapsed, cast.total));
        }
        if viewer.in_combat {
            parts.push("in combat".into());
        }
        if let Some(target) = viewer.target {
            let detail = latest
                .entities
                .iter()
                .find(|unit| unit.entity() == target)
                .map_or_else(
                    || "out of sight".into(),
                    |unit| {
                        if unit.flags.dead {
                            format!("L{}, dead", unit.level)
                        } else {
                            format!("L{}, {}%", unit.level, unit.health_percent)
                        }
                    },
                );
            parts.push(format!("target {} ({detail})", self.unit_name(target)));
        }
        if viewer.auto_attacking {
            parts.push("attacking".into());
        }
        Some(parts.join(" · "))
    }

    /// The model a unit is drawn with, from the zone content.
    fn unit_model(&self, entity: EntityRef, record: &EntitySnapshot) -> UnitModel {
        let template = (entity.kind() == EntityKind::Creature)
            .then(|| {
                self.content
                    .creature_template(CreatureTemplateId::new(record.appearance))
            })
            .flatten();
        let role = match entity {
            EntityRef::Npc(id) => self.content.npc(id).map(|npc| npc.role),
            _ => None,
        };
        UnitModel::choose(
            entity.kind(),
            template.map(|template| template.family),
            template.is_some_and(|template| template.elite),
            role,
        )
    }

    /// Colour and half extents in metres of a unit.
    fn unit_look(&self, entity: EntityRef, record: &EntitySnapshot) -> ([f32; 3], [f32; 3]) {
        let character = metres(PLAYER_HALF_EXTENTS_UNITS);
        match entity {
            EntityRef::Player(id) if id == self.player_id => (SELF_COLOR, character),
            EntityRef::Player(_) => (PLAYER_COLOR, character),
            EntityRef::Npc(id) => {
                let guard = self
                    .content
                    .npc(id)
                    .is_some_and(|npc| npc.role == NpcRole::Guard);
                (if guard { GUARD_COLOR } else { FRIENDLY_COLOR }, character)
            }
            EntityRef::Creature(_) => {
                let template = self
                    .content
                    .creature_template(CreatureTemplateId::new(record.appearance));
                let half = template.map_or(character, |template| metres(template.half_extents));
                let family = template.map_or(CreatureFamily::Wolf, |template| template.family);
                let color = if record.flags.tapped_by_other {
                    TAPPED_COLOR
                } else {
                    creature_color(family, record.flags.hostile)
                };
                (color, half)
            }
        }
    }

    fn pose(&self, unit: &EntitySnapshot) -> PlayerPose {
        PlayerPose {
            position: self.rendered(unit.position),
            yaw: yaw_radians(f32::from(unit.facing)),
        }
    }

    /// Metres, lifted by the presentation relief under the unit.
    fn rendered(&self, position: [i32; 3]) -> [f32; 3] {
        let relief = self.scenery.height_at(position[0], position[2]);
        let [x, y, z] = metres(position);
        [x, y + relief as f32 / UNITS_PER_METRE as f32, z]
    }
}

fn append_health_bar(boxes: &mut Vec<SceneBox>, position: [f32; 3], percent: u8, yaw: f32) {
    boxes.push(SceneBox {
        position,
        size: [HEALTH_BAR_WIDTH + 0.04, 0.12, 0.04],
        color: HEALTH_BAR_BACKGROUND,
        yaw,
    });
    let width = HEALTH_BAR_WIDTH * f32::from(percent.min(100)) / 100.0;
    if width > 0.0 {
        // Local +X is screen-left. Shift the fill toward that edge, and
        // toward the eye (-local Z) so the background cannot obscure it.
        let left = (HEALTH_BAR_WIDTH - width) / 2.0;
        let front = -0.035;
        boxes.push(SceneBox {
            position: [
                position[0] + left * yaw.cos() + front * yaw.sin(),
                position[1],
                position[2] + front * yaw.cos() - left * yaw.sin(),
            ],
            size: [width, 0.08, 0.02],
            color: HEALTH_BAR_FILL,
            yaw,
        });
    }
}

/// Hostile creatures red-ish, neutral ones yellow-ish, shaded per family.
fn creature_color(family: CreatureFamily, hostile: bool) -> [f32; 3] {
    let shade = match family {
        CreatureFamily::Wolf => 0.0,
        CreatureFamily::Boar => 0.04,
        CreatureFamily::Vermin => 0.08,
        CreatureFamily::Marauder => 0.12,
        CreatureFamily::Mirefin => 0.16,
        CreatureFamily::Redbrand => 0.2,
        CreatureFamily::Prop => 0.24,
        CreatureFamily::Ogre => 0.28,
    };
    if hostile {
        [0.78 - shade, 0.24 + shade * 0.5, 0.18]
    } else {
        [0.8 - shade * 0.5, 0.68 - shade, 0.2]
    }
}

/// Converts a presentation angle (radians, 0 facing +Z, turning toward +X)
/// into the nearest `u16` yaw intent.
#[must_use]
pub fn yaw_from_radians(radians: f32) -> u16 {
    let turns = (radians / std::f32::consts::TAU).rem_euclid(1.0);
    // A full turn rounds to 65 536, which wraps to yaw 0.
    ((turns * 65_536.0).round() as u32 % 65_536) as u16
}

/// Converts yaw steps (possibly fractional or outside one turn) to radians within one turn.
fn yaw_radians(steps: f32) -> f32 {
    (steps / 65_536.0).rem_euclid(1.0) * std::f32::consts::TAU
}

fn metres(value: [i32; 3]) -> [f32; 3] {
    value.map(|component| component as f32 / UNITS_PER_METRE as f32)
}

#[cfg(test)]
pub(crate) fn observed_imported_draws(count: usize, time: f32) -> Vec<crate::world::SceneDraw> {
    use mmorpg_presentation::units::{imported::Kind, kit::Frame};
    let model = &skinned::asset(Kind::Ranger).model;
    (0..count)
        .flat_map(|unit| {
            model.draws(
                &format!("observed-{unit}"),
                Frame {
                    p: [(unit % 10) as f64 * 2.0, 0.0, (unit / 10) as f64 * 2.0],
                    q: [0.0, 0.0, 0.0, 1.0],
                },
                1.8,
                Kind::Ranger.clips().idle,
                time,
                None,
                &[],
            )
        })
        .collect()
}
