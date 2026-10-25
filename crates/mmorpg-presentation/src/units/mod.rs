//! Shared unit presentation (#184 part 2): procedural players, creatures and NPCs,
//! locomotion and action poses, spell visuals, health bars and target rings, and the
//! clip choice for imported models. Both clients feed it sampled (interpolated)
//! units and their latest projection; each client then skins its imported models
//! with its own provider and draws everything else exactly as returned.
//!
//! Units stand on the shared presentation relief: a rendered position is the
//! physics position plus `Scenery::height_at` at its XZ, never fed back.
pub mod animation;
pub mod archer;
pub mod breakables;
pub mod creatures;
pub mod effects;
pub mod imported;
pub mod kit;
pub mod player;

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};

use mmorpg_core::{
    ClassChoice, CreatureFamily, CreatureTemplateId, EntityKind, EntityRef, EntitySnapshot,
    PLAYER_HALF_EXTENTS_UNITS, ProjectileKind, UNITS_PER_METRE, ZoneContent, ZoneEvent,
    ZoneSnapshot,
};
use mmorpg_scenery::Scenery;

use crate::{Draw, geometry::V};
use imported::{Kind, Loadout, Outfit, Playback, Resident};
use kit::{Frame, IQ, Primitive, Resources, offset, qa, qm, shade};
use player::{Hat, Look};

const HEALTH_BAR_WIDTH: f32 = 1.0;
const HEALTH_BAR_BACKGROUND: [f32; 3] = [0.12, 0.12, 0.14];
const HEALTH_BAR_FILL: [f32; 3] = [0.18, 0.9, 0.25];
/// How much a corpse's imported model darkens.
const CORPSE_TINT: f32 = 0.6;
/// The Druid companion's green tint (#296) on the CC0 wolf model hostile Timber Wolves also use.
pub const COMPANION_TINT: [f32; 3] = [0.6, 1.0, 0.45];

/// The linear colour factor of an imported model: a companion is tinted green, a corpse
/// darkened, anything else drawn as authored.
fn imported_tint(dead: bool, companion: bool) -> Option<[f32; 3]> {
    let base = companion.then_some(COMPANION_TINT);
    if dead {
        Some(base.unwrap_or([1.0; 3]).map(|c| c * CORPSE_TINT))
    } else {
        base
    }
}

/// The display name of a minion whose projected appearance is `appearance`: a Necromancer's
/// skeleton (#295) or a Druid's companion (#296).
#[must_use]
pub fn minion_name(appearance: u16) -> &'static str {
    if mmorpg_core::is_companion(mmorpg_core::minion_ability(appearance)) {
        "Wolf Companion"
    } else {
        "Skeleton"
    }
}

/// A unit as sampled for this frame: its record and interpolated rendered pose.
#[derive(Clone, Debug)]
pub struct SampledUnit {
    pub record: EntitySnapshot,
    /// Body centre in metres: physics position plus relief.
    pub centre: [f32; 3],
    /// Radians, 0 facing +Z, turning toward +X.
    pub yaw: f32,
}

/// How the viewer presents themselves: their class look, headwear and an optional
/// character-selection turntable yaw.
#[derive(Clone, Copy, Debug, Default)]
pub struct Viewer {
    /// The previewed choice while choosing a character, else the zone's.
    pub choice: Option<ClassChoice>,
    pub headwear: Option<Hat>,
    pub preview_yaw: Option<f32>,
}

pub struct FrameInput<'a> {
    pub snapshot: &'a ZoneSnapshot,
    pub units: &'a [SampledUnit],
    pub viewer: Viewer,
    /// Presentation clock and the step since the previous frame, in seconds.
    pub seconds: f64,
    pub dt: f64,
    pub reduced_motion: bool,
    /// Camera heading the health bars face.
    pub bar_yaw: f32,
}

/// An imported model to skin and draw this frame.
#[derive(Clone, Debug)]
pub struct ImportedUnit {
    pub entity: EntityRef,
    pub id: String,
    pub kind: Kind,
    /// Feet on the ground, turned by the unit's yaw.
    pub root: Frame,
    /// Collision height in metres; the model is scaled to it.
    pub height: f64,
    /// The layered clips to pose (Atlas 6.6.45) and each region's target clip.
    pub animation: imported::Animation,
    /// Linear RGB factor on the model's colours: corpse darkening and the companion's green
    /// (#296); `None` draws the model as authored.
    pub tint: Option<[f32; 3]>,
    /// What the unit carries: a player its class weapons, a guard its sword.
    pub loadout: Option<Loadout>,
    /// A named resident's appearance preset (#418); players and animals have none.
    pub resident: Option<Resident>,
    /// A player's class starter outfit (asset-tooling #379); residents and animals have none.
    pub outfit: Option<Outfit>,
    /// The living viewer wears their headwear on the posed head (see [`UnitPresenter::headwear`]).
    pub headwear: Option<Look>,
}

/// Where one presented unit stands.
#[derive(Clone, Debug)]
pub struct UnitView {
    pub entity: EntityRef,
    pub id: String,
    /// Feet in metres and yaw in radians.
    pub feet: V,
    pub centre_y: f32,
    pub yaw: f64,
    /// Nameplate anchor above the health bar; imported models raise it over their posed head.
    pub label: [f32; 3],
    /// The unit's procedural body draws within [`UnitFrame::draws`] (empty for imported models).
    pub body: Range<usize>,
}

#[derive(Debug, Default)]
pub struct UnitFrame {
    pub draws: Vec<Draw>,
    pub units: Vec<UnitView>,
    pub imported: Vec<ImportedUnit>,
}

/// Per-session unit presentation state: geometry cache, locomotion, clip clocks and spell visuals.
pub struct UnitPresenter {
    player_id: u32,
    content: Arc<ZoneContent>,
    scenery: Arc<Scenery>,
    resources: Resources,
    animators: BTreeMap<EntityRef, animation::Animator>,
    playbacks: BTreeMap<EntityRef, Playback>,
    /// Fall progress of each dead unit; only a witnessed death falls (#27).
    deaths: BTreeMap<EntityRef, Option<f64>>,
    /// The projection tick each bursting flask or bomb (#290) was first seen bursting.
    bursts: BTreeMap<u16, u64>,
    effects: effects::Effects,
    durations: BTreeMap<(Kind, String), f32>,
    ready: BTreeSet<Kind>,
    /// The client draws the residents' appearance presets on the Ranger (#418).
    residents: bool,
    /// The viewer's equipment as last projected: its armour replaces outfit pieces.
    equipment: Option<mmorpg_core::Equipment>,
    structures: crate::structures::Structures,
    /// How the imported models' body regions blend; the Action Lab tunes it (#384).
    blend: imported::Blend,
}

impl UnitPresenter {
    #[must_use]
    pub fn new(player_id: u32, content: Arc<ZoneContent>, scenery: Arc<Scenery>) -> Self {
        let structures = crate::structures::Structures::on_scenery(content.pieces(), &scenery);
        Self {
            structures,
            player_id,
            content,
            scenery,
            resources: Resources::default(),
            animators: BTreeMap::new(),
            playbacks: BTreeMap::new(),
            deaths: BTreeMap::new(),
            bursts: BTreeMap::new(),
            effects: effects::Effects::default(),
            durations: BTreeMap::new(),
            ready: BTreeSet::new(),
            residents: false,
            equipment: None,
            blend: imported::Blend::default(),
        }
    }

    /// Tunes how the imported models' body regions blend (the Action Lab, #384); clamped.
    pub fn set_blend(&mut self, blend: imported::Blend) {
        self.blend = blend.clamped();
    }

    /// The blend in use.
    #[must_use]
    pub const fn blend(&self) -> imported::Blend {
        self.blend
    }

    /// The client has loaded the residents' appearance presets (#418): once the Ranger is ready
    /// too, the named residents are imported Rangers instead of procedural models.
    pub fn residents_ready(&mut self) {
        self.residents = true;
    }

    /// An imported model is loaded: its clip lengths in seconds. Until then its units keep
    /// their procedural models.
    pub fn model_ready(&mut self, kind: Kind, clips: impl IntoIterator<Item = (String, f32)>) {
        for (clip, seconds) in clips {
            self.durations.insert((kind, clip), seconds);
        }
        self.ready.insert(kind);
    }

    /// A new connection epoch: drop every per-unit and visual state.
    pub fn reset(&mut self) {
        self.animators.clear();
        self.playbacks.clear();
        self.deaths.clear();
        self.bursts.clear();
        self.effects.clear();
        self.equipment = None;
        self.resources = Resources::default();
        self.structures =
            crate::structures::Structures::on_scenery(self.content.pieces(), &self.scenery);
    }

    /// Observes a newly received projection; spell visuals start from its events.
    pub fn push(&mut self, snapshot: &ZoneSnapshot, seconds: f64) {
        let anchors = snapshot
            .entities
            .iter()
            .map(|e| {
                let half = self.half(e.entity(), e);
                let p = self.rendered(e.position);
                (
                    e.entity(),
                    effects::Anchor {
                        feet: [f64::from(p[0]), f64::from(p[1] - half[1]), f64::from(p[2])],
                        centre: f64::from(p[1]),
                        yaw: f64::from(e.facing) / 65536. * std::f64::consts::TAU,
                    },
                )
            })
            .collect();
        self.effects.push(snapshot, seconds, &anchors);
        // The sheet comes only on change ticks; it holds until the next one.
        if let Some(equipment) = snapshot.equipment {
            self.equipment = Some(equipment);
        }
        // Accepted strikes start their clips (Atlas 6.6.45): the event, never a clip, is the
        // attack. Every received projection is read, so a frame spanning several ticks keeps them.
        for event in &snapshot.events {
            let (source, ability) = match *event {
                ZoneEvent::AttackPerformed { source, .. }
                | ZoneEvent::StrikeSwept { source, .. } => (source, None),
                ZoneEvent::AbilityUsed {
                    source, ability, ..
                } => (source, Some(ability)),
                _ => continue,
            };
            let Some(record) = snapshot.entities.iter().find(|e| e.entity() == source) else {
                continue;
            };
            let Some(kind) = self.imported_kind(source, record) else {
                continue;
            };
            let class = match source {
                EntityRef::Player(id) if id == snapshot.viewer_id => snapshot.viewer.class,
                EntityRef::Player(_) => ClassChoice::from_appearance(record.appearance),
                _ => None,
            };
            let mut clip = kind.strike(class.map(|choice| choice.class), ability);
            if self.duration(kind, clip) <= 0. {
                clip = kind.clips().attack;
            }
            let duration = self.duration(kind, clip);
            self.playbacks
                .entry(source)
                .or_default()
                .strike(clip, duration, seconds);
        }
    }

    /// Half extents in metres of a unit's body.
    #[must_use]
    pub fn half(&self, entity: EntityRef, record: &EntitySnapshot) -> [f32; 3] {
        let character = metres(PLAYER_HALF_EXTENTS_UNITS);
        match entity {
            EntityRef::Creature(_) => self
                .content
                .creature_template(CreatureTemplateId::new(record.appearance))
                .map_or(character, |template| {
                    metres(template.body_half_extents(record.facing))
                }),
            EntityRef::Minion(_) => metres(mmorpg_core::minion_half_extents(
                mmorpg_core::minion_ability(record.appearance),
            )),
            EntityRef::Player(_) | EntityRef::Npc(_) => character,
        }
    }

    /// Metres, lifted by the presentation relief under the unit.
    #[must_use]
    pub fn rendered(&self, position: [i32; 3]) -> [f32; 3] {
        let relief = self.scenery.height_at(position[0], position[2]);
        let [x, y, z] = metres(position);
        [x, y + relief as f32 / UNITS_PER_METRE as f32, z]
    }

    fn duration(&self, kind: Kind, clip: &str) -> f32 {
        self.durations
            .get(&(kind, clip.to_owned()))
            .copied()
            .unwrap_or(0.)
    }

    /// Every unit draw for one frame, except the imported models' skinned bodies.
    pub fn frame(&mut self, input: &FrameInput<'_>) -> UnitFrame {
        let snapshot = input.snapshot;
        let reduced = input.reduced_motion;
        let seconds = input.seconds;
        let present: BTreeSet<_> = input.units.iter().map(|u| u.record.entity()).collect();
        self.animators.retain(|e, _| present.contains(e));
        self.playbacks.retain(|e, _| present.contains(e));
        self.deaths.retain(|e, _| present.contains(e));
        let action = if reduced {
            None
        } else {
            self.effects.action(snapshot, seconds)
        };
        let mut out = UnitFrame::default();
        let mut anchors = BTreeMap::new();
        for unit in input.units {
            let entity = unit.record.entity();
            let half = self.half(entity, &unit.record);
            let feet = [
                f64::from(unit.centre[0]),
                f64::from(unit.centre[1] - half[1]),
                f64::from(unit.centre[2]),
            ];
            let viewer = entity == EntityRef::Player(self.player_id);
            let yaw = f64::from(
                input
                    .viewer
                    .preview_yaw
                    .filter(|_| viewer)
                    .unwrap_or(unit.yaw),
            );
            let bar_y = unit.centre[1] + half[1] + 0.3;
            let root = Frame {
                p: feet,
                q: qa(1, yaw),
            };
            anchors.insert(
                entity,
                effects::Anchor {
                    feet,
                    centre: f64::from(unit.centre[1]),
                    yaw,
                },
            );
            let v = unit.record.velocity.map(|v| f64::from(v) * 0.3);
            let loc = self.animators.entry(entity).or_default().update(
                [feet[0], feet[2]],
                yaw,
                v,
                input.dt,
                reduced,
            );
            let name = match entity.kind() {
                EntityKind::Player => "player",
                EntityKind::Creature => "creature",
                EntityKind::Npc => "npc",
                EntityKind::Minion => "minion",
            };
            let id = format!("unit-{name}-{}", unit.record.id);
            let dead = unit.record.flags.dead;
            let start = out.draws.len();
            if let Some(kind) = self.imported_kind(entity, &unit.record) {
                let resident = self.resident(entity);
                // Only the viewer's guard is projected; the shield rises once the zone's guard
                // protects (#156, #384), not while slash or landing recovery holds it back.
                let guarding = viewer && snapshot.viewer.guard.protecting;
                // Swimmers (#274) have no swim clips yet: the in-place fallback (Atlas 6.6.41)
                // treads with the idle clip and strokes with the walk, never a jump or fall
                // from ascending or descending. The mode is the zone's, from the projected
                // position and the content's water volumes.
                let swimming = matches!(entity, EntityRef::Player(_))
                    && mmorpg_core::WaterMode::of_player(
                        self.content.waters(),
                        unit.record.position,
                    )
                    .swims();
                let vertical = if swimming { 0. } else { loc.vertical };
                let durations = &self.durations;
                let animation = self.playbacks.entry(entity).or_default().advance(
                    kind.clips(),
                    |clip| {
                        durations
                            .get(&(kind, clip.to_owned()))
                            .copied()
                            .unwrap_or(0.)
                    },
                    imported::Cues {
                        speed: loc.forward.hypot(loc.right),
                        vertical_speed: vertical,
                        dead,
                        guarding,
                        now: seconds,
                        dt: input.dt,
                        reduced,
                    },
                    self.blend,
                );
                let class = if viewer {
                    input.viewer.choice
                } else {
                    ClassChoice::from_appearance(unit.record.appearance)
                };
                out.imported.push(ImportedUnit {
                    entity,
                    id: id.clone(),
                    kind,
                    root,
                    height: f64::from(half[1]) * 2.,
                    animation,
                    // Only a companion minion has an imported model (`Kind::of`).
                    tint: imported_tint(dead, entity.kind() == EntityKind::Minion),
                    loadout: match resident {
                        Some(resident) => resident.loadout(),
                        None if matches!(entity, EntityRef::Player(_)) => {
                            class.map(|c| Loadout::of_class(c.class))
                        }
                        None => None,
                    },
                    resident,
                    outfit: match entity {
                        EntityRef::Player(_) => class.map(|c| {
                            Outfit::new(c.class, self.equipment.as_ref().filter(|_| viewer))
                        }),
                        _ => None,
                    },
                    headwear: (viewer && !dead).then(|| {
                        Look::new(
                            input.viewer.choice.map_or(0, |c| c.class.code()),
                            input.viewer.choice.is_none_or(|c| c.sex.code() == 0),
                            input.viewer.headwear,
                            false,
                        )
                    }),
                });
            } else {
                let life = self.life(entity, dead, input.dt, reduced);
                // The viewer plays its own projected cast; every other actor its observed attempts.
                let action = if dead || reduced {
                    None
                } else if viewer {
                    action
                } else {
                    self.effects.action_for(entity, seconds)
                };
                let body = self.procedural(entity, unit, &id, root, half, loc, action, life, input);
                out.draws.extend(body);
            }
            let body = start..out.draws.len();
            if !dead && entity.kind() != EntityKind::Npc {
                self.health_bar(&mut out.draws, &id, unit, half, input.bar_yaw);
            }
            if entity.kind() != EntityKind::Player && snapshot.viewer.target == Some(entity) {
                let mut ring = self.resources.primitive(
                    format!("{id}-target-ring"),
                    Primitive::Post(f64::from(half[0].max(half[2])) + 0.35, 0.04),
                    0xf2d15f,
                    Frame {
                        p: [feet[0], feet[1] + 0.02, feet[2]],
                        q: IQ,
                    },
                );
                ring.material.opacity = 0.85;
                ring.material.unlit = true;
                out.draws.push(ring);
            }
            out.units.push(UnitView {
                entity,
                id,
                feet,
                centre_y: unit.centre[1],
                yaw,
                label: [unit.centre[0], bar_y + 0.15, unit.centre[2]],
                body,
            });
        }
        if !reduced {
            out.draws
                .extend(self.effects.nodes(&mut self.resources, seconds, &anchors));
        }
        self.projectiles(&mut out.draws, snapshot);
        self.platforms(&mut out.draws, snapshot);
        let viewer = input
            .units
            .iter()
            .find(|unit| unit.record.entity() == EntityRef::Player(self.player_id))
            .map(|unit| {
                let half = self.half(unit.record.entity(), &unit.record);
                [
                    f64::from(unit.centre[0]),
                    f64::from(unit.centre[1] - half[1]),
                    f64::from(unit.centre[2]),
                ]
            });
        out.draws.extend(
            self.structures
                .frame(&snapshot.pieces, seconds, viewer, reduced),
        );
        for view in &snapshot.debris {
            let centre = self.rendered(view.position);
            out.draws
                .extend(breakables::piece(&mut self.resources, view, centre));
        }
        self.resources.retire_unused();
        out
    }

    /// Moving platforms (#56) where their schedule puts them at the projection's tick: a
    /// stone pillar with a plank top. Content and tick decide; nothing is simulated here.
    fn platforms(&mut self, draws: &mut Vec<Draw>, snapshot: &ZoneSnapshot) {
        for platform in self.content.platforms() {
            let centre = self
                .rendered(platform.centre_at(snapshot.tick))
                .map(f64::from);
            let [x, y, z] = metres(platform.half_extents).map(f64::from);
            let id = format!("platform-{}", platform.id.get());
            draws.push(self.resources.primitive(
                format!("{id}-pillar"),
                Primitive::Box([2. * x, 2. * y - 0.06, 2. * z]),
                0x8c8478,
                Frame {
                    p: [centre[0], centre[1] - 0.03, centre[2]],
                    q: IQ,
                },
            ));
            draws.push(self.resources.primitive(
                format!("{id}-top"),
                Primitive::Box([2. * x, 0.06, 2. * z]),
                0x7a5a3a,
                Frame {
                    p: [centre[0], centre[1] + y - 0.03, centre[2]],
                    q: IQ,
                },
            ));
        }
    }

    /// Arrows and bolts where the zone says they are (#286): a shaft behind the tip along its
    /// heading and pitch, and a dark head. Placeholder shapes until asset-tooling's bow and
    /// arrow bundle (asset-tooling #277) lands. A magic missile (#289) is a fiery ball with a
    /// short trail; a shadow missile (#301) is a violet one with a green core. A Rogue's
    /// throwing knife (#290) is a short dark grip behind a steel blade.
    fn projectiles(&mut self, draws: &mut Vec<Draw>, snapshot: &ZoneSnapshot) {
        self.bursts.retain(|id, _| {
            snapshot
                .projectiles
                .iter()
                .any(|projectile| projectile.burst && projectile.id == *id)
        });
        for projectile in &snapshot.projectiles {
            let (length, width, shaft, head) = match projectile.kind {
                ProjectileKind::Flask | ProjectileKind::Bomb => {
                    self.thrown(draws, projectile, snapshot.tick);
                    continue;
                }
                ProjectileKind::Knife => (0.22, 0.025, 0x2a2420, 0xc4c9cc),
                ProjectileKind::Arrow => (0.8, 0.03, 0xb08a5b, 0x4a4d52),
                ProjectileKind::Bolt => (0.45, 0.045, 0x5c3f28, 0x3a3c40),
                ProjectileKind::Missile => (0.5, 0.08, 0xff8a2a, 0xffd36a),
                ProjectileKind::Shadow => (0.5, 0.08, 0x5a2a8a, 0x9cff7a),
                // A Cleric's holy missile (#303): gold with a white core.
                ProjectileKind::Holy => (0.5, 0.08, 0xffc94a, 0xfff6d8),
                // A Druid's nature missile (#304): leaf green with a pale core.
                ProjectileKind::Nature => (0.5, 0.08, 0x5fbf3a, 0xd8ffb0),
            };
            let turn = |value: f64| value / 65_536. * std::f64::consts::TAU;
            let tip = Frame {
                p: self.rendered(projectile.position).map(f64::from),
                q: qm(
                    qa(1, turn(f64::from(projectile.yaw))),
                    qa(0, -turn(f64::from(projectile.pitch))),
                ),
            };
            let id = format!("projectile-{}", projectile.id);
            if projectile.kind.is_magic() {
                draws.push(self.resources.primitive(
                    format!("{id}-ball"),
                    Primitive::Ball(0.12),
                    head,
                    tip,
                ));
                draws.push(self.resources.primitive(
                    format!("{id}-trail"),
                    Primitive::Box([width, width, length]),
                    shaft,
                    offset(tip, [0., 0., -length / 2.]),
                ));
                continue;
            }
            draws.push(self.resources.primitive(
                format!("{id}-shaft"),
                Primitive::Box([width, width, length]),
                shaft,
                offset(tip, [0., 0., -length / 2.]),
            ));
            draws.push(self.resources.primitive(
                format!("{id}-head"),
                Primitive::Box([width * 2.5, width * 2.5, 0.08]),
                head,
                offset(tip, [0., 0., -0.04]),
            ));
        }
    }

    /// A Rogue's thrown flask or bomb (#290): a green glass flask with a cork, or a black bomb
    /// with a glowing fuse, resting on its lowest point. Bursting, it is a glowing ball that
    /// swells to the zone's splash (3 m) or blast (4 m) radius and fades.
    fn thrown(
        &mut self,
        draws: &mut Vec<Draw>,
        projectile: &mmorpg_core::ProjectileView,
        tick: u64,
    ) {
        let [x, y, z] = self.rendered(projectile.position).map(f64::from);
        let id = format!("projectile-{}", projectile.id);
        let bomb = projectile.kind == ProjectileKind::Bomb;
        if projectile.burst {
            let first = *self.bursts.entry(projectile.id).or_insert(tick);
            #[allow(clippy::cast_precision_loss)]
            let t = (tick.saturating_sub(first) as f64 / f64::from(mmorpg_core::BURST_TICKS))
                .clamp(0., 1.);
            let (radius, color, glow) = if bomb {
                (4., 0xffa23a, [1.0, 0.55, 0.15])
            } else {
                (3., 0x8fe05a, [0.45, 0.9, 0.25])
            };
            // Six sizes keep the shared mesh cache small.
            let step = (t * 6.).ceil() / 6.;
            let mut flash = self.resources.primitive(
                format!("{id}-burst"),
                Primitive::Ball(radius * (0.3 + 0.7 * step)),
                color,
                Frame {
                    p: [x, y, z],
                    q: IQ,
                },
            );
            flash.material.emissive = glow;
            flash.material.unlit = true;
            #[allow(clippy::cast_possible_truncation)]
            let fade = (1. - t) as f32;
            flash.material.opacity = 0.15 + 0.6 * fade;
            draws.push(flash);
            return;
        }
        let (radius, body) = if bomb {
            (0.13, 0x2b2b2e)
        } else {
            (0.09, 0x5fbf3a)
        };
        let centre = [x, y + radius, z];
        let mut shell = self.resources.primitive(
            format!("{id}-body"),
            Primitive::Ball(radius),
            body,
            Frame { p: centre, q: IQ },
        );
        if !bomb {
            shell.material.opacity = 0.85;
        }
        draws.push(shell);
        if bomb {
            let mut spark = self.resources.primitive(
                format!("{id}-spark"),
                Primitive::Ball(0.04),
                0xffc04a,
                Frame {
                    p: [x, y + 2. * radius + 0.03, z],
                    q: IQ,
                },
            );
            spark.material.emissive = [1.0, 0.7, 0.2];
            spark.material.unlit = true;
            draws.push(spark);
        } else {
            draws.push(self.resources.primitive(
                format!("{id}-cork"),
                Primitive::Box([0.05, 0.06, 0.05]),
                0x8a6a4a,
                Frame {
                    p: [x, y + 2. * radius + 0.02, z],
                    q: IQ,
                },
            ));
        }
    }

    /// Alive, or dead with its fall progress: only a witnessed life-to-death transition falls;
    /// a corpse first seen dead, or reduced motion, lies settled.
    fn life(&mut self, entity: EntityRef, dead: bool, dt: f64, reduced: bool) -> animation::Life {
        let previous = self.deaths.get(&entity).copied();
        if !dead {
            self.deaths.insert(entity, None);
            return animation::Life::Alive;
        }
        let progress = match previous {
            Some(None) if !reduced => 0.,
            Some(Some(progress)) if !reduced => (progress + dt.max(0.) / 0.45).min(1.),
            _ => 1.,
        };
        self.deaths.insert(entity, Some(progress));
        animation::Life::Dead { progress }
    }

    fn imported_kind(&self, entity: EntityRef, record: &EntitySnapshot) -> Option<Kind> {
        let kind = match entity {
            EntityRef::Npc(_) => self.resident(entity).map(|_| Kind::Ranger),
            _ => Kind::of(entity, record, &self.content),
        };
        kind.filter(|kind| self.ready.contains(kind))
    }

    /// The resident preset an NPC wears, once the client draws resident looks (#418).
    fn resident(&self, entity: EntityRef) -> Option<Resident> {
        let EntityRef::Npc(id) = entity else {
            return None;
        };
        if !self.residents {
            return None;
        }
        Resident::of_name(&self.content.npc(id)?.name)
    }

    fn procedural(
        &mut self,
        entity: EntityRef,
        unit: &SampledUnit,
        id: &str,
        root: Frame,
        half: [f32; 3],
        loc: animation::Locomotion,
        action: Option<animation::Action>,
        life: animation::Life,
        input: &FrameInput<'_>,
    ) -> Vec<Draw> {
        let dead = life.dead();
        if let EntityRef::Player(id_num) = entity {
            let other = id_num != self.player_id;
            // Other players are drawn in the viewer's chosen look until per-player looks exist.
            let choice = input.viewer.choice;
            let look = Look::new(
                choice.map_or(0, |c| c.class.code()),
                choice.is_none_or(|c| c.sex.code() == 0),
                input.viewer.headwear,
                other,
            );
            let base = animation::pose(
                if dead {
                    animation::Locomotion::default()
                } else {
                    loc
                },
                look.stance,
            );
            let posed = animation::action(base, action);
            let bow_turn = posed.twist - base.twist;
            // A dead player falls onto its back about its body centre.
            let root = if dead {
                let t = life.fall();
                let s = t * t * (3. - 2. * t);
                let half_y = f64::from(half[1]);
                kit::root(
                    root.p,
                    2. * root.q[1].atan2(root.q[3]),
                    half_y,
                    Some((
                        kit::tilt(-std::f64::consts::FRAC_PI_2 * s, 0., 0.),
                        half_y + (0.18 - half_y) * s,
                    )),
                )
            } else {
                root
            };
            return player::nodes(&mut self.resources, id, root, look, posed, bow_turn);
        }
        let template = (entity.kind() == EntityKind::Creature)
            .then(|| {
                self.content
                    .creature_template(CreatureTemplateId::new(unit.record.appearance))
            })
            .flatten();
        if let Some((kind, t)) = template.and_then(|t| Some((t.prop()?, t))) {
            return breakables::standing(
                &mut self.resources,
                id,
                root,
                kind,
                f64::from(t.half_extents[1]) / f64::from(UNITS_PER_METRE),
            );
        }
        // A summoned skeleton (#295): a deliberately plain bone-white placeholder humanoid until
        // the owner's undead art exists; no final minion art is invented here.
        // A Druid's companion (#296) is drawn as a wolf with a leaf-green accent.
        let companion = entity.kind() == EntityKind::Minion
            && mmorpg_core::is_companion(mmorpg_core::minion_ability(unit.record.appearance));
        if entity.kind() == EntityKind::Minion && !companion {
            return creatures::skeleton(
                &mut self.resources,
                id,
                root,
                half.map(f64::from),
                loc,
                action,
                life,
            );
        }
        let family = if companion {
            Some(CreatureFamily::Wolf)
        } else {
            template.map(|t| t.family)
        };
        let role = match entity {
            EntityRef::Npc(id) => self.content.npc(id).map(|n| n.role),
            _ => None,
        };
        let accent = if companion {
            0x7fc46a
        } else {
            family.map_or(0x4f9d5d, |f| creatures::accent(&unit.record, f))
        };
        let half = half.map(f64::from);
        if let Some(f @ (CreatureFamily::Wolf | CreatureFamily::Boar | CreatureFamily::Vermin)) =
            family
        {
            return creatures::animal(
                &mut self.resources,
                id,
                root,
                half,
                f,
                accent,
                loc,
                action,
                life,
            );
        }
        if family.is_some() || role.is_some() {
            return creatures::humanoid(
                &mut self.resources,
                id,
                root,
                half,
                family,
                role,
                template.is_some_and(|t| t.elite),
                template.is_some_and(|t| self.content.is_shielded(t.id)),
                accent,
                loc,
                // NPCs keep their idle; only creatures act.
                action.filter(|_| entity.kind() == EntityKind::Creature),
                life,
            );
        }
        let c = if dead { shade(accent, 0.45) } else { accent };
        let mut body = vec![self.resources.primitive(
            format!("{id}-body"),
            Primitive::Box(if dead {
                [half[0] * 2., 0.16, half[1] * 2.]
            } else {
                half.map(|v| v * 2.)
            }),
            c,
            offset(root, [0., if dead { 0.08 } else { half[1] }, 0.]),
        )];
        if !dead {
            body.push(self.resources.primitive(
                format!("{id}-nose"),
                Primitive::Box([0.12, 0.12, 0.12]),
                0xead9b5,
                offset(root, [0., half[1] * 1.4, half[2] + 0.06]),
            ));
        }
        body
    }

    fn health_bar(
        &mut self,
        draws: &mut Vec<Draw>,
        id: &str,
        unit: &SampledUnit,
        half: [f32; 3],
        yaw: f32,
    ) {
        let position = [
            unit.centre[0],
            unit.centre[1] + half[1] + 0.3,
            unit.centre[2],
        ];
        let mut bars = vec![(
            position,
            [HEALTH_BAR_WIDTH + 0.04, 0.12, 0.04],
            HEALTH_BAR_BACKGROUND,
        )];
        let width = HEALTH_BAR_WIDTH * f32::from(unit.record.health_percent.min(100)) / 100.0;
        if width > 0.0 {
            // Local +X is screen-left. Shift the fill toward that edge, and
            // toward the eye (-local Z) so the background cannot obscure it.
            let left = (HEALTH_BAR_WIDTH - width) / 2.0;
            let front = -0.035;
            bars.push((
                [
                    position[0] + left * yaw.cos() + front * yaw.sin(),
                    position[1],
                    position[2] + front * yaw.cos() - left * yaw.sin(),
                ],
                [width, 0.08, 0.02],
                HEALTH_BAR_FILL,
            ));
        }
        for (index, (p, size, color)) in bars.into_iter().enumerate() {
            let mut draw = self.resources.draw(
                format!("{id}-health-{index}"),
                "box",
                0xffffff,
                Frame {
                    p: p.map(f64::from),
                    q: qa(1, f64::from(yaw)),
                },
                size.map(|s| f64::from(s) * 0.5),
            );
            draw.material.color = color.map(|v| {
                if v <= 0.003_130_8 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1. / 2.4) - 0.055
                }
            });
            draw.material.unlit = true;
            draws.push(draw);
        }
    }
}

fn metres(value: [i32; 3]) -> [f32; 3] {
    value.map(|component| component as f32 / UNITS_PER_METRE as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_companion_wolf_is_tinted_green_unlike_hostile_wolves() {
        assert_eq!(imported_tint(false, false), None);
        assert_eq!(imported_tint(false, true), Some(COMPANION_TINT));
        assert_eq!(imported_tint(true, false), Some([CORPSE_TINT; 3]));
        assert_eq!(
            imported_tint(true, true),
            Some(COMPANION_TINT.map(|c| c * CORPSE_TINT))
        );
    }
}
