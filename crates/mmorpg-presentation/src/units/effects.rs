//! Spell visuals and actor actions, observed from authoritative projections.
//!
//! An effect starts from a projection event (or from the viewer's own cast, aura or guard
//! state), lives for a fixed time or while its cast, aura or guard lasts, and expires on its own.
//! Every visible actor (except NPCs) plays its own attempts: swings, casts that hold while they
//! run, and releases. Death and interrupts end an action. Nothing here feeds back into the zone.
use super::{
    animation::{Action, Stance},
    kit::*,
};
use crate::{Draw, geometry::V};
use mmorpg_core::{
    AuraKind, EntityRef, StrikeAim, StrikeHeight, StrikeSide, TICK_HZ, ZoneEvent, ZoneSnapshot,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    f64::consts::{PI, TAU},
};

#[derive(Clone, Copy, PartialEq)]
enum Visual {
    Projectile,
    Ring,
    Bubble,
    Shards,
    Swing,
    /// Frost Breath (#292): a spray fanning out along the caster's facing.
    Cone,
    /// Lightning Beam (#292): held along the caster's facing while it channels.
    Beam,
    /// Chain Lightning (#292): arcs from the caster through each victim in turn.
    Chain,
    /// Entangling Roots (#304): vines that rise and coil around the rooted target.
    Roots,
    /// Drain Life (#301): a tether from the target back to the caster while it channels.
    Drain,
    /// A creature special's danger footprint (#276, Atlas 6.6.29–6.6.31): the true ring it will
    /// strike, held while the zone's wind-up lasts, then a brief burst when it resolves.
    Danger {
        radius: f64,
        burst: bool,
    },
    Impact {
        critical: bool,
    },
    Sweep {
        aim: StrikeAim,
        hit: bool,
    },
    /// The viewer's raised shield: its lane and whether it protects, waits or is broken.
    Guard {
        lane: StrikeHeight,
        state: GuardState,
    },
    /// A blow met a guard in `lane`: blocked, or overpowering it. The source is the defender.
    Block {
        lane: StrikeHeight,
        broken: bool,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum GuardState {
    Protecting,
    Suspended,
    Broken,
}

#[derive(Clone)]
struct Effect {
    id: u64,
    visual: Visual,
    ability: u8,
    source: EntityRef,
    target: Option<EntityRef>,
    ground: Option<V>,
    /// The units a chain struck, in order; for a beam, its farthest victim.
    chain: Vec<EntityRef>,
    start: f64,
    /// Seconds a timed effect lives; `None` while it follows a cast, aura or guard.
    duration: Option<f64>,
}

#[derive(Clone, Copy)]
pub struct Anchor {
    pub feet: V,
    pub centre: f64,
    pub yaw: f64,
}

#[derive(Clone, Copy)]
struct ActorAction {
    stance: Stance,
    start: f64,
    duration: f64,
    hold: bool,
    ability: u8,
    release: bool,
    /// The viewer's own projected cast progress, which wins over the clock.
    projected: Option<f64>,
}

const MAX_EFFECTS: usize = 24;
const ACTION_SECONDS: f64 = 0.3;
const SWEEP_SECONDS: f64 = 0.34;
const BLOCK_SECONDS: f64 = 0.45;
const IMPACT_SECONDS: f64 = 0.32;
const DANGER_BURST_SECONDS: f64 = 0.35;
const BLIZZARD: u8 = 12;
const LIGHTNING_BEAM: u8 = 22;
/// The beam's reach and the cone's, metres (the zone's 20 m and 8 m).
const BEAM_METRES: f64 = 20.;
const CONE_METRES: f64 = 8.;
const CONE_HALF_ANGLE: f64 = PI / 6.;
const BARRIER: u8 = 11;
const DRAIN_LIFE: u8 = 28;
const BONE_SHIELD: u8 = 29;
const HOLY_BEAM: u8 = 36;
const DIVINE_SHIELD: u8 = 37;
const BARKSKIN: u8 = 41;
/// The zone's guard coverage is centred 30° toward the shield (left) side of the facing.
const GUARD_SIDE: f64 = PI / 6.;
const SWEEP_HALF_WIDTH: f64 = PI / 6.;

#[derive(Default)]
pub struct Effects {
    effects: Vec<Effect>,
    next: u64,
    actions: BTreeMap<EntityRef, ActorAction>,
    casts: BTreeMap<EntityRef, ActorAction>,
    dead: BTreeSet<EntityRef>,
    last_tick: Option<u64>,
    session: Option<(u32, u32, u64)>,
}

/// Each ability's caster pose, visual and accent colour.
fn presentation(id: u8) -> (Option<Stance>, Option<Visual>, u32) {
    match id {
        1 => (Some(Stance::Sword), Some(Visual::Swing), 0xe0a15c),
        2 => (Some(Stance::Sword), None, 0x9fb4c8),
        3 => (None, None, 0x7fd08a),
        4 => (Some(Stance::Sword), Some(Visual::Swing), 0xd9a066),
        // The zone's own arrows fly for the shots (#286).
        5 => (Some(Stance::Bow), None, 0xc9b36a),
        6 => (Some(Stance::Bow), None, 0x7fc05a),
        7 => (Some(Stance::Bow), None, 0xb0a58c),
        8 => (None, None, 0xf2c14e),
        9 => (Some(Stance::Staff), Some(Visual::Projectile), 0xff8a3d),
        10 => (Some(Stance::Staff), Some(Visual::Ring), 0x8fd8ff),
        11 => (Some(Stance::Staff), Some(Visual::Bubble), 0xb69cff),
        12 => (Some(Stance::Staff), Some(Visual::Shards), 0xa8e4ff),
        13 => (Some(Stance::Staff), Some(Visual::Projectile), 0x7d9a4a),
        14 => (Some(Stance::Staff), None, 0xe7dcc0),
        15 => (Some(Stance::Bow), None, 0x8a6a4a),
        18 => (Some(Stance::Sword), None, 0x8a7a5a),
        21 => (Some(Stance::Staff), Some(Visual::Cone), 0x9fdcff),
        22 => (Some(Stance::Staff), Some(Visual::Beam), 0xc8dcff),
        23 => (Some(Stance::Staff), Some(Visual::Chain), 0xb9a8ff),
        24 => (Some(Stance::Sword), None, 0xd8452c),
        // The Necromancer (#301): shadow violet bolts, a green curse and drain, a bone ward.
        26 => (Some(Stance::Staff), Some(Visual::Projectile), 0x8a4dff),
        27 => (Some(Stance::Staff), Some(Visual::Projectile), 0x7dff8a),
        28 => (Some(Stance::Staff), Some(Visual::Drain), 0x6fe07a),
        29 => (Some(Stance::Staff), Some(Visual::Bubble), 0xe6dfc8),
        // The Rogue (#302): steel and poison-green dagger swings, a gouge and a rush.
        30 => (Some(Stance::Sword), Some(Visual::Swing), 0xc4c9cc),
        31 => (Some(Stance::Sword), Some(Visual::Swing), 0x8fd14f),
        32 => (Some(Stance::Sword), None, 0xd9b26a),
        33 => (None, None, 0xf2d24e),
        // The Cleric (#303): a gold smite and beam, a soft green-gold renew, a white-gold ward.
        34 => (Some(Stance::Staff), Some(Visual::Projectile), 0xffd65a),
        35 => (Some(Stance::Staff), None, 0xd8f0a0),
        36 => (Some(Stance::Staff), Some(Visual::Beam), 0xffe9a0),
        37 => (Some(Stance::Staff), Some(Visual::Bubble), 0xfff3c8),
        // The Druid (#304): a green wrath, brown-green roots, a leafy rejuvenation, a bark ward.
        38 => (Some(Stance::Staff), Some(Visual::Projectile), 0x7fd65a),
        39 => (Some(Stance::Staff), Some(Visual::Roots), 0x6b8a3a),
        40 => (Some(Stance::Staff), None, 0x9ee07a),
        41 => (Some(Stance::Staff), Some(Visual::Bubble), 0xa07a4a),
        // The Rogue's throws (#290): the zone's own knives, flasks and bombs fly.
        42 => (Some(Stance::Sword), None, 0xc4c9cc),
        43 => (Some(Stance::Sword), None, 0x8fe05a),
        44 => (Some(Stance::Sword), None, 0xffa23a),
        // Raise Skeleton (#295): a bone-white raising cast.
        45 => (Some(Stance::Staff), None, 0xe6dfc8),
        _ => (None, None, 0xffffff),
    }
}

/// The self-centred creature specials with a reliable ring footprint, in metres: Stomp's 3.5 m
/// and Redbrand Whirl's 4 m (the zone's radii).
fn danger_radius(id: u8) -> Option<f64> {
    match id {
        18 => Some(3.5),
        24 => Some(4.),
        _ => None,
    }
}

fn timed(visual: Visual) -> Option<f64> {
    match visual {
        Visual::Projectile => Some(0.4),
        Visual::Ring => Some(0.65),
        Visual::Swing => Some(0.28),
        Visual::Cone => Some(0.45),
        Visual::Chain => Some(0.5),
        Visual::Roots => Some(2.),
        _ => None,
    }
}

/// Blade height above the feet (metres) per aimed band; the zone's bands are 0–0.6, 0.6–1.3, 1.3–2 m.
fn lane_height(lane: StrikeHeight) -> f64 {
    match lane {
        StrikeHeight::Low => 0.35,
        StrikeHeight::Middle => 0.95,
        StrikeHeight::High => 1.65,
    }
}

impl Effects {
    pub fn clear(&mut self) {
        *self = Self {
            next: self.next,
            ..Self::default()
        };
    }

    fn add(
        &mut self,
        visual: Visual,
        ability: u8,
        source: EntityRef,
        target: Option<EntityRef>,
        duration: Option<f64>,
        now: f64,
    ) {
        self.next += 1;
        self.effects.push(Effect {
            id: self.next,
            visual,
            ability,
            source,
            target,
            ground: None,
            chain: Vec::new(),
            start: now,
            duration,
        });
        if self.effects.len() > MAX_EFFECTS {
            self.effects.remove(0);
        }
    }

    fn shards(
        &mut self,
        source: EntityRef,
        target: Option<EntityRef>,
        now: f64,
        anchors: &BTreeMap<EntityRef, Anchor>,
    ) {
        if self
            .effects
            .iter()
            .any(|e| e.visual == Visual::Shards && e.source == source)
        {
            return;
        }
        if let Some(a) = target.and_then(|t| anchors.get(&t)) {
            self.add(Visual::Shards, BLIZZARD, source, target, None, now);
            if let Some(effect) = self.effects.last_mut() {
                effect.ground = Some(a.feet);
            }
        }
    }

    /// Lightning Beam or Holy Beam (#303), in the ability's own colour.
    fn beam(&mut self, ability: u8, source: EntityRef, now: f64) {
        if !self
            .effects
            .iter()
            .any(|e| e.visual == Visual::Beam && e.source == source)
        {
            self.add(Visual::Beam, ability, source, None, None, now);
        }
    }

    fn drain(&mut self, source: EntityRef, target: Option<EntityRef>, now: f64) {
        if target.is_some()
            && !self
                .effects
                .iter()
                .any(|e| e.visual == Visual::Drain && e.source == source)
        {
            self.add(Visual::Drain, DRAIN_LIFE, source, target, None, now);
        }
    }

    /// Consumes one received projection; identities never share animation state.
    pub fn push(&mut self, s: &ZoneSnapshot, now: f64, anchors: &BTreeMap<EntityRef, Anchor>) {
        let session = (s.zone_id.get(), s.viewer_id, s.content_revision);
        if self.session != Some(session) || self.last_tick.is_some_and(|tick| s.tick < tick) {
            self.clear();
            self.session = Some(session);
        }
        if self.last_tick != Some(s.tick) {
            self.last_tick = Some(s.tick);
            self.spawn(s, now, anchors);
        }
        self.sync(s, now, anchors);
    }

    fn spawn(&mut self, s: &ZoneSnapshot, now: f64, anchors: &BTreeMap<EntityRef, Anchor>) {
        let visible: BTreeSet<_> = s.entities.iter().map(|e| e.entity()).collect();
        self.dead = s
            .entities
            .iter()
            .filter(|e| e.flags.dead)
            .map(|e| e.entity())
            .collect();
        let gone = |e: &EntityRef| !visible.contains(e);
        for actions in [&mut self.actions, &mut self.casts] {
            actions.retain(|e, _| !gone(e));
        }
        self.effects
            .retain(|e| !gone(&e.source) && e.target.is_none_or(|t| !gone(&t)));
        for event in &s.events {
            match *event {
                ZoneEvent::AttackPerformed { source, .. } => {
                    self.actions.insert(
                        source,
                        ActorAction {
                            stance: Stance::Sword,
                            start: now,
                            duration: ACTION_SECONDS,
                            hold: false,
                            ability: 0,
                            release: false,
                            projected: None,
                        },
                    );
                }
                ZoneEvent::CastStarted {
                    source,
                    target,
                    ability,
                    ticks,
                } => {
                    let id = ability.get();
                    if let Some(stance) = presentation(id).0 {
                        self.casts.insert(
                            source,
                            ActorAction {
                                stance,
                                start: now,
                                duration: f64::from(ticks) / f64::from(TICK_HZ),
                                hold: true,
                                ability: id,
                                release: false,
                                projected: None,
                            },
                        );
                    }
                    if id == BLIZZARD {
                        self.shards(source, target, now, anchors);
                    }
                    if let Some(radius) = danger_radius(id) {
                        let warning = Visual::Danger {
                            radius,
                            burst: false,
                        };
                        self.add(warning, id, source, None, None, now);
                    }
                    if matches!(id, LIGHTNING_BEAM | HOLY_BEAM) {
                        self.beam(id, source, now);
                    }
                    if id == DRAIN_LIFE {
                        self.drain(source, target, now);
                    }
                }
                ZoneEvent::AbilityUsed {
                    source,
                    target,
                    ability,
                } => {
                    let id = ability.get();
                    let (stance, visual, _) = presentation(id);
                    let release = self.casts.get(&source).is_some_and(|c| c.ability == id);
                    if release {
                        self.casts.remove(&source);
                    }
                    if let Some(stance) = stance {
                        self.actions.insert(
                            source,
                            ActorAction {
                                stance,
                                start: now,
                                duration: ACTION_SECONDS,
                                hold: false,
                                ability: id,
                                release,
                                projected: None,
                            },
                        );
                    }
                    if let Some(visual) = visual
                        && let Some(duration) = timed(visual)
                    {
                        self.add(visual, id, source, target, Some(duration), now);
                    }
                    if let Some(radius) = danger_radius(id) {
                        let burst = Visual::Danger {
                            radius,
                            burst: true,
                        };
                        self.add(burst, id, source, None, Some(DANGER_BURST_SECONDS), now);
                    }
                }
                ZoneEvent::StrikeSwept {
                    source,
                    target,
                    ability,
                    aim,
                } => {
                    // The zone's outcome, not the input, drives the sweep: its aim, and hit or whiff.
                    self.actions.insert(
                        source,
                        ActorAction {
                            stance: Stance::Sword,
                            start: now,
                            duration: SWEEP_SECONDS,
                            hold: false,
                            ability: ability.get(),
                            release: false,
                            projected: None,
                        },
                    );
                    self.add(
                        Visual::Sweep {
                            aim,
                            hit: target.is_some(),
                        },
                        ability.get(),
                        source,
                        target,
                        Some(SWEEP_SECONDS),
                        now,
                    );
                }
                ZoneEvent::Blocked {
                    source,
                    target,
                    lane,
                    ..
                }
                | ZoneEvent::GuardBroken {
                    source,
                    target,
                    lane,
                    ..
                } => {
                    // The defender's shield meets the blow at the zone's lane, facing the attacker.
                    let broken = matches!(event, ZoneEvent::GuardBroken { .. });
                    self.add(
                        Visual::Block { lane, broken },
                        0,
                        target,
                        Some(source),
                        Some(BLOCK_SECONDS),
                        now,
                    );
                }
                ZoneEvent::Interrupted {
                    target, ability, ..
                } => {
                    let id = ability.get();
                    if self.casts.get(&target).is_some_and(|c| c.ability == id) {
                        self.casts.remove(&target);
                    }
                    if self.actions.get(&target).is_some_and(|a| a.ability == id) {
                        self.actions.remove(&target);
                    }
                }
                ZoneEvent::DamageDealt {
                    source,
                    target,
                    amount,
                    critical,
                }
                | ZoneEvent::DamageTaken {
                    source,
                    target,
                    amount,
                    critical,
                }
                | ZoneEvent::DamageObserved {
                    source,
                    target,
                    amount,
                    critical,
                } if amount > 0 => {
                    self.add(
                        Visual::Impact { critical },
                        0,
                        source,
                        Some(target),
                        Some(IMPACT_SECONDS),
                        now,
                    );
                }
                ZoneEvent::Died { entity, .. } => {
                    self.dead.insert(entity);
                }
                _ => {}
            }
        }
        // Chain and beam victims, in the zone's order: every unit their caster damaged this tick.
        for event in &s.events {
            if let ZoneEvent::DamageDealt {
                source,
                target,
                amount,
                ..
            }
            | ZoneEvent::DamageObserved {
                source,
                target,
                amount,
                ..
            } = *event
                && amount > 0
                && let Some(effect) = self.effects.iter_mut().rev().find(|e| {
                    e.source == source
                        && ((e.visual == Visual::Chain && e.start == now)
                            || e.visual == Visual::Beam)
                })
            {
                if effect.visual == Visual::Beam {
                    effect.chain = vec![target];
                } else if !effect.chain.contains(&target) {
                    effect.chain.push(target);
                }
            }
        }
        // Death wins even when the same projection also contains an accepted attempt.
        for entity in &self.dead {
            self.actions.remove(entity);
            self.casts.remove(entity);
        }
    }

    /// Only the viewer's continuously projected cast, auras and guard reconstruct bound effects.
    fn sync(&mut self, s: &ZoneSnapshot, now: f64, anchors: &BTreeMap<EntityRef, Anchor>) {
        let viewer = EntityRef::Player(s.viewer_id);
        let cast = s.viewer.cast;
        match cast.and_then(|c| presentation(c.ability.get()).0.map(|stance| (c, stance))) {
            Some((c, stance)) if !s.viewer.dead => {
                self.casts.insert(
                    viewer,
                    ActorAction {
                        stance,
                        start: now - f64::from(c.elapsed) / f64::from(TICK_HZ),
                        duration: f64::from(c.total) / f64::from(TICK_HZ),
                        hold: true,
                        ability: c.ability.get(),
                        release: false,
                        projected: Some(if c.total > 0 {
                            f64::from(c.elapsed) / f64::from(c.total)
                        } else {
                            1.
                        }),
                    },
                );
            }
            _ => {
                self.casts.remove(&viewer);
            }
        }
        // Arcane Barrier, Bone Shield (#301), Divine Shield (#303) or Barkskin (#304): the viewer's
        // absorb ward, in its own colour.
        let ward = if s.viewer.dead {
            None
        } else {
            s.auras
                .iter()
                .find(|a| {
                    matches!(
                        a.ability.get(),
                        BARRIER | BONE_SHIELD | DIVINE_SHIELD | BARKSKIN
                    ) && a.kind == AuraKind::Absorb
                })
                .map(|a| a.ability.get())
        };
        let casts = &self.casts;
        self.effects.retain(|e| {
            if e.duration.is_some() || matches!(e.visual, Visual::Guard { .. }) {
                return true;
            }
            if e.visual == Visual::Shards {
                return casts.get(&e.source).is_some_and(|c| c.ability == BLIZZARD);
            }
            if e.visual == Visual::Beam {
                return casts.get(&e.source).is_some_and(|c| c.ability == e.ability);
            }
            if e.visual == Visual::Drain {
                return casts
                    .get(&e.source)
                    .is_some_and(|c| c.ability == DRAIN_LIFE);
            }
            e.source == viewer && ward == Some(e.ability)
        });
        if cast.is_some_and(|c| c.channel && c.ability.get() == BLIZZARD) && !s.viewer.dead {
            self.shards(viewer, s.viewer.target, now, anchors);
        }
        if let Some(c) = cast
            && c.channel
            && matches!(c.ability.get(), LIGHTNING_BEAM | HOLY_BEAM)
            && !s.viewer.dead
        {
            self.beam(c.ability.get(), viewer, now);
        }
        if cast.is_some_and(|c| c.channel && c.ability.get() == DRAIN_LIFE) && !s.viewer.dead {
            self.drain(viewer, s.viewer.target, now);
        }
        // The raised shield follows the zone's guard: its lane, and protecting, suspended or broken.
        let shown = if s.viewer.dead {
            None
        } else {
            s.viewer.guard.raised.map(|lane| {
                let state = if s.viewer.guard.broken > 0 {
                    GuardState::Broken
                } else if s.viewer.guard.protecting {
                    GuardState::Protecting
                } else {
                    GuardState::Suspended
                };
                Visual::Guard { lane, state }
            })
        };
        match (
            shown,
            self.effects
                .iter_mut()
                .find(|e| matches!(e.visual, Visual::Guard { .. })),
        ) {
            (None, _) => self
                .effects
                .retain(|e| !matches!(e.visual, Visual::Guard { .. })),
            (Some(visual), Some(raised)) => raised.visual = visual,
            (Some(visual), None) => self.add(visual, 0, viewer, None, None, now),
        }
        if let Some(ward) = ward
            && !self
                .effects
                .iter()
                .any(|e| e.visual == Visual::Bubble && e.source == viewer)
        {
            self.add(Visual::Bubble, ward, viewer, None, None, now);
        }
    }

    /// The action `entity` plays at `now`, if any; NPCs and the dead play none.
    #[must_use]
    pub fn action_for(&self, entity: EntityRef, now: f64) -> Option<Action> {
        if matches!(entity, EntityRef::Npc(_)) || self.dead.contains(&entity) {
            return None;
        }
        for action in [self.actions.get(&entity), self.casts.get(&entity)]
            .into_iter()
            .flatten()
        {
            if let Some(progress) = action.projected {
                return Some(Action {
                    stance: action.stance,
                    progress: progress.min(1.),
                    hold: true,
                });
            }
            let elapsed = now - action.start;
            if elapsed >= action.duration {
                continue;
            }
            let progress = (elapsed / action.duration.max(0.001)).clamp(0., 1.);
            return Some(Action {
                stance: action.stance,
                progress: if action.release {
                    0.5 + progress / 2.
                } else {
                    progress
                },
                hold: action.hold,
            });
        }
        None
    }

    /// The viewer's own cast, draw or swing over its locomotion.
    #[must_use]
    pub fn action(&self, s: &ZoneSnapshot, now: f64) -> Option<Action> {
        if s.viewer.dead {
            return None;
        }
        if let Some(c) = s.viewer.cast
            && let Some(stance) = presentation(c.ability.get()).0
        {
            return Some(Action {
                stance,
                progress: if c.total > 0 {
                    (f64::from(c.elapsed) / f64::from(c.total)).min(1.)
                } else {
                    1.
                },
                hold: true,
            });
        }
        self.action_for(EntityRef::Player(s.viewer_id), now)
    }

    /// Draws of every live effect at `now`; stable IDs, only transforms change.
    pub fn nodes(
        &mut self,
        r: &mut Resources,
        now: f64,
        anchors: &BTreeMap<EntityRef, Anchor>,
    ) -> Vec<Draw> {
        for actions in [&mut self.actions, &mut self.casts] {
            actions.retain(|_, a| a.projected.is_some() || now - a.start < a.duration);
        }
        let casts = &self.casts;
        self.effects.retain(|e| match e.duration {
            Some(d) => now - e.start < d,
            None => {
                casts.get(&e.source).is_some_and(|c| c.ability == e.ability)
                    || matches!(e.visual, Visual::Bubble | Visual::Guard { .. })
            }
        });
        let mut out = Vec::new();
        for e in &self.effects {
            let age = now - e.start;
            let t = e.duration.map_or(0., |d| (age / d).clamp(0., 1.));
            let source = anchors.get(&e.source);
            let target = e.target.and_then(|t| anchors.get(&t));
            let color = presentation(e.ability).2;
            let prefix = format!("fx-{}", e.id);
            let mut add = |suffix: &str,
                           p: Primitive,
                           c: u32,
                           position: V,
                           q: Q,
                           opacity: f32,
                           unlit: bool| {
                let mut d =
                    r.primitive(format!("{prefix}-{suffix}"), p, c, Frame { p: position, q });
                d.material.opacity = opacity;
                d.material.unlit = unlit;
                out.push(d);
            };
            let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
            match e.visual {
                Visual::Projectile => {
                    if let (Some(s), Some(d)) = (source, target) {
                        let position = |t: f64| {
                            [
                                lerp(s.feet[0], d.feet[0], t),
                                lerp(s.centre, d.centre, t) + (PI * t).sin() * 0.5,
                                lerp(s.feet[2], d.feet[2], t),
                            ]
                        };
                        add(
                            "core",
                            Primitive::Ball(0.2),
                            color,
                            position(t),
                            IQ,
                            1.,
                            false,
                        );
                        add(
                            "trail",
                            Primitive::Ball(0.12),
                            0xfff2c4,
                            position((t - 0.12).max(0.)),
                            IQ,
                            0.7,
                            false,
                        );
                    }
                }
                Visual::Ring => {
                    if let Some(s) = source {
                        let radius = (8. * (1. - (1. - t) * (1. - t))).max(0.3);
                        for i in 0..20 {
                            let a = f64::from(i) / 20. * TAU;
                            add(
                                &format!("ring-{i}"),
                                Primitive::Box([0.5, 0.14, 0.18]),
                                color,
                                [
                                    s.feet[0] + a.sin() * radius,
                                    s.feet[1] + 0.2,
                                    s.feet[2] + a.cos() * radius,
                                ],
                                qa(1, a + PI / 2.),
                                0.8,
                                false,
                            );
                        }
                    }
                }
                Visual::Danger { radius, burst } => {
                    if let Some(s) = source {
                        // The warning pulses gently; the burst flares and fades at the edge.
                        let opacity = if burst {
                            0.9 * (1. - t) as f32
                        } else {
                            (0.45 + 0.15 * (age * 6.).sin()) as f32
                        };
                        for i in 0..28 {
                            let a = f64::from(i) / 28. * TAU;
                            add(
                                &format!("danger-{i}"),
                                Primitive::Box([0.5, 0.05, 0.12]),
                                color,
                                [
                                    s.feet[0] + a.sin() * radius,
                                    s.feet[1] + 0.06,
                                    s.feet[2] + a.cos() * radius,
                                ],
                                qa(1, a + PI / 2.),
                                opacity,
                                true,
                            );
                        }
                    }
                }
                Visual::Bubble => {
                    if let Some(s) = source {
                        add(
                            "bubble",
                            Primitive::Ball(1.25 * (1. + 0.03 * (age * 3.).sin())),
                            color,
                            [s.feet[0], s.centre, s.feet[2]],
                            IQ,
                            0.28,
                            false,
                        );
                    }
                }
                Visual::Shards => {
                    if let Some(g) = e.ground {
                        let scatter = |i: f64, salt: f64| {
                            let v = (i * 127.1 + salt * 311.7).sin() * 43758.5453;
                            v - v.floor()
                        };
                        for i in 0..12 {
                            let j = f64::from(i);
                            let a = scatter(j, 1.) * TAU;
                            let radius = scatter(j, 2.).sqrt() * 6.;
                            let fall = (age * (0.9 + scatter(j, 3.) * 0.7) + scatter(j, 4.)) % 1.;
                            add(
                                &format!("shard-{i}"),
                                Primitive::Box([0.09, 0.6, 0.09]),
                                color,
                                [
                                    g[0] + a.sin() * radius,
                                    g[1] + 6.5 - fall * 6.4,
                                    g[2] + a.cos() * radius,
                                ],
                                IQ,
                                0.85,
                                false,
                            );
                        }
                    }
                }
                Visual::Cone => {
                    if let Some(s) = source {
                        let reach = CONE_METRES * (1. - (1. - t) * (1. - t));
                        for i in 0..7 {
                            let a =
                                s.yaw - CONE_HALF_ANGLE + CONE_HALF_ANGLE * 2. * f64::from(i) / 6.;
                            for k in 0..4 {
                                let d = reach * (f64::from(k) + 1.) / 4.;
                                add(
                                    &format!("cone-{i}-{k}"),
                                    Primitive::Box([0.22 + d * 0.06, 0.12, 0.22 + d * 0.06]),
                                    if k == 3 { 0xffffff } else { color },
                                    [
                                        s.feet[0] + a.sin() * d,
                                        s.centre + 0.2 - d * 0.04,
                                        s.feet[2] + a.cos() * d,
                                    ],
                                    qa(1, a),
                                    (0.85 * (1. - t)) as f32,
                                    true,
                                );
                            }
                        }
                    }
                }
                Visual::Beam => {
                    if let Some(s) = source {
                        // As far as its farthest victim, or its full reach.
                        let length = e
                            .chain
                            .first()
                            .and_then(|v| anchors.get(v))
                            .map_or(BEAM_METRES, |v| {
                                (v.feet[0] - s.feet[0]).hypot(v.feet[2] - s.feet[2])
                            });
                        let flicker = 0.85 + 0.15 * (age * 40.).sin();
                        for k in 0..10 {
                            let d = length * (f64::from(k) + 0.5) / 10.;
                            add(
                                &format!("beam-{k}"),
                                Primitive::Box([
                                    0.14 * flicker,
                                    0.14 * flicker,
                                    length / 10. + 0.05,
                                ]),
                                if k % 3 == 0 { 0xffffff } else { color },
                                [
                                    s.feet[0] + s.yaw.sin() * d,
                                    s.centre + 0.3,
                                    s.feet[2] + s.yaw.cos() * d,
                                ],
                                qa(1, s.yaw),
                                0.9,
                                true,
                            );
                        }
                    }
                }
                Visual::Chain => {
                    if let Some(s) = source {
                        let mut from = [s.feet[0], s.centre + 0.3, s.feet[2]];
                        for (j, victim) in e.chain.iter().enumerate() {
                            let Some(v) = anchors.get(victim) else {
                                continue;
                            };
                            let to = [v.feet[0], v.centre, v.feet[2]];
                            let yaw = (to[0] - from[0]).atan2(to[2] - from[2]);
                            for k in 0..8 {
                                let u = (f64::from(k) + 0.5) / 8.;
                                let jitter = ((f64::from(k) * 1.7 + age * 30.).sin()) * 0.25;
                                add(
                                    &format!("chain-{j}-{k}"),
                                    Primitive::Box([0.1, 0.1, 0.5]),
                                    if k % 2 == 0 { 0xffffff } else { color },
                                    [
                                        lerp(from[0], to[0], u) + yaw.cos() * jitter,
                                        lerp(from[1], to[1], u) + jitter * 0.5,
                                        lerp(from[2], to[2], u) - yaw.sin() * jitter,
                                    ],
                                    qa(1, yaw),
                                    (0.95 * (1. - t)) as f32,
                                    true,
                                );
                            }
                            from = to;
                        }
                    }
                }
                Visual::Roots => {
                    if let Some(v) = target {
                        // Six vines grow out of the ground around the victim's feet and coil
                        // inward, then sink back as the burst ends.
                        let grow = (t * 4.).min(1.) * (1. - ((t - 0.8) * 5.).max(0.));
                        for i in 0..6 {
                            let a = f64::from(i) / 6. * TAU + t * 1.5;
                            for k in 0..4 {
                                let h = f64::from(k) / 4.;
                                if h > grow {
                                    break;
                                }
                                let radius = 0.55 - 0.25 * h;
                                let turn = a + h * 1.6;
                                add(
                                    &format!("vine-{i}-{k}"),
                                    Primitive::Box([0.09, 0.32, 0.09]),
                                    if k == 3 { 0x7fd65a } else { color },
                                    [
                                        v.feet[0] + turn.sin() * radius,
                                        v.feet[1] + 0.16 + h * 1.1,
                                        v.feet[2] + turn.cos() * radius,
                                    ],
                                    qa(1, turn),
                                    1.,
                                    false,
                                );
                            }
                        }
                    }
                }
                Visual::Drain => {
                    if let (Some(s), Some(v)) = (source, target) {
                        // Motes stream from the victim back into the caster's hands.
                        let from = [v.feet[0], v.centre, v.feet[2]];
                        let to = [s.feet[0], s.centre + 0.3, s.feet[2]];
                        for k in 0..10 {
                            let u = (f64::from(k) / 10. + age * 0.8) % 1.;
                            let sway = (u * PI * 3. + age * 6.).sin() * 0.18;
                            add(
                                &format!("drain-{k}"),
                                Primitive::Ball(0.07 + 0.04 * (1. - u)),
                                if k % 3 == 0 { 0x8a4dff } else { color },
                                [
                                    lerp(from[0], to[0], u) + sway,
                                    lerp(from[1], to[1], u) + (PI * u).sin() * 0.35,
                                    lerp(from[2], to[2], u) - sway,
                                ],
                                IQ,
                                0.9,
                                true,
                            );
                        }
                    }
                }
                Visual::Swing => {
                    if let Some(s) = source {
                        let sweep = PI * 0.9;
                        for i in 0..6 {
                            let a = s.yaw + sweep / 2. - sweep * t * (f64::from(6 - i) / 6.);
                            add(
                                &format!("arc-{i}"),
                                Primitive::Box([0.7, 0.06, 0.16]),
                                if i == 0 { 0xffffff } else { color },
                                [
                                    s.feet[0] + a.sin() * 1.5,
                                    s.centre + 0.25,
                                    s.feet[2] + a.cos() * 1.5,
                                ],
                                qa(1, a + PI / 2.),
                                0.75,
                                false,
                            );
                        }
                    }
                }
                Visual::Impact { critical } => {
                    if let Some(d) = target {
                        let glow = if critical { 0xffe6a3 } else { 0xffb376 };
                        let fade = (0.85 * (1. - t)) as f32;
                        add(
                            "flash",
                            Primitive::Ball(0.08 + 0.2 * t),
                            glow,
                            [d.feet[0], d.centre, d.feet[2]],
                            IQ,
                            fade,
                            true,
                        );
                        for i in 0..6 {
                            let a = f64::from(i) / 6. * TAU;
                            let radius = 0.12 + 0.85 * t;
                            add(
                                &format!("spark-{i}"),
                                Primitive::Box([0.06, 0.06, 0.18]),
                                glow,
                                [
                                    d.feet[0] + a.cos() * radius,
                                    d.centre + a.sin() * radius,
                                    d.feet[2] + (a * 2.).sin() * radius * 0.3,
                                ],
                                qa(1, a),
                                (0.9 * (1. - t)) as f32,
                                true,
                            );
                        }
                    }
                }
                Visual::Sweep { aim, hit } => {
                    if let Some(s) = source {
                        let side = match aim.side {
                            StrikeSide::Left => PI / 4.,
                            StrikeSide::Centre => 0.,
                            StrikeSide::Right => -PI / 4.,
                        };
                        let centre = s.yaw + side;
                        let height = s.feet[1] + lane_height(aim.height);
                        let tint = if hit { 0xffd36b } else { 0x9fb4c8 };
                        // The blade crosses the covered arc from the left edge to the right; the trail lags.
                        for i in 0..8 {
                            let lag = f64::from(i) / 8.;
                            let reach = (t * 1.25 - lag * 0.35).clamp(0., 1.);
                            let a = centre + SWEEP_HALF_WIDTH - 2. * SWEEP_HALF_WIDTH * reach;
                            add(
                                &format!("sweep-{i}"),
                                Primitive::Box([0.9, 0.05, 0.12]),
                                if i == 0 { 0xffffff } else { tint },
                                [s.feet[0] + a.sin() * 1.6, height, s.feet[2] + a.cos() * 1.6],
                                qa(1, a + PI / 2.),
                                (0.85 * (1. - t) * (1. - lag * 0.6)) as f32,
                                true,
                            );
                        }
                    }
                }
                Visual::Guard { lane, state } => {
                    if let Some(s) = source {
                        // The shield covers the front and the shield (left) side: it sits 30° toward it.
                        let a = s.yaw + GUARD_SIDE;
                        let (tint, opacity) = match state {
                            GuardState::Protecting => (0x9cc8ff, 0.55),
                            GuardState::Broken => (0xff6a4d, 0.25),
                            GuardState::Suspended => (0x9fb4c8, 0.25),
                        };
                        add(
                            "shield",
                            Primitive::Box([0.62, 0.5, 0.07]),
                            tint,
                            [
                                s.feet[0] + a.sin() * 0.55,
                                s.feet[1] + lane_height(lane),
                                s.feet[2] + a.cos() * 0.55,
                            ],
                            qa(1, a),
                            opacity,
                            true,
                        );
                    }
                }
                Visual::Block { lane, broken } => {
                    if let (Some(s), Some(d)) = (source, target) {
                        let a = (d.feet[0] - s.feet[0]).atan2(d.feet[2] - s.feet[2]);
                        // A block flashes and holds; a broken guard flares red and drops away.
                        let drop = if broken { 0.5 * t * t } else { 0. };
                        add(
                            "plate",
                            Primitive::Box([0.7 + 0.25 * t, 0.55 + 0.2 * t, 0.06]),
                            if broken { 0xff5a3c } else { 0xe8f3ff },
                            [
                                s.feet[0] + a.sin() * 0.7,
                                s.feet[1] + lane_height(lane) - drop,
                                s.feet[2] + a.cos() * 0.7,
                            ],
                            qa(1, a),
                            (0.9 * (1. - t)) as f32,
                            true,
                        );
                    }
                }
            }
        }
        out
    }
}
