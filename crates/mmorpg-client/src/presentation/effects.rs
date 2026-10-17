//! Published spell visuals and viewer actions, observed from authoritative projections.
use super::{
    animation::{Action, Stance},
    kit::*,
};
use crate::world::{SceneDraw, geometry::V};
use mmorpg_core::{AuraKind, EntityRef, ZoneEvent, ZoneSnapshot};
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
};
#[derive(Clone, Copy)]
enum Visual {
    Projectile,
    Arrow,
    Ring,
    Bubble,
    Shards,
    Swing,
}
#[derive(Clone)]
struct Effect {
    id: u64,
    visual: Visual,
    ability: u8,
    source: EntityRef,
    target: Option<EntityRef>,
    ground: Option<V>,
    start: f64,
    duration: Option<f64>,
}
#[derive(Clone, Copy)]
pub(super) struct Anchor {
    pub feet: V,
    pub centre: f64,
    pub yaw: f64,
}
#[derive(Default)]
pub(super) struct Effects {
    effects: Vec<Effect>,
    next: u64,
    action: Option<(Stance, f64, bool)>,
    casting: Option<u8>,
}
fn presentation(id: u8) -> (Option<Stance>, Option<Visual>, u32) {
    match id {
        1 => (Some(Stance::Sword), Some(Visual::Swing), 0xe0a15c),
        2 => (Some(Stance::Sword), None, 0x9fb4c8),
        3 => (None, None, 0x7fd08a),
        4 => (Some(Stance::Sword), Some(Visual::Swing), 0xd9a066),
        5 => (Some(Stance::Bow), Some(Visual::Arrow), 0xc9b36a),
        6 => (Some(Stance::Bow), Some(Visual::Arrow), 0x7fc05a),
        7 => (Some(Stance::Bow), Some(Visual::Arrow), 0xb0a58c),
        8 => (None, None, 0xf2c14e),
        9 => (Some(Stance::Staff), Some(Visual::Projectile), 0xff8a3d),
        10 => (Some(Stance::Staff), Some(Visual::Ring), 0x8fd8ff),
        11 => (Some(Stance::Staff), Some(Visual::Bubble), 0xb69cff),
        12 => (Some(Stance::Staff), Some(Visual::Shards), 0xa8e4ff),
        13 => (None, Some(Visual::Projectile), 0x7d9a4a),
        _ => (None, None, 0xffffff),
    }
}
impl Effects {
    pub fn clear(&mut self) {
        self.effects.clear();
        self.action = None;
        self.casting = None;
    }
    fn add(
        &mut self,
        visual: Visual,
        ability: u8,
        source: EntityRef,
        target: Option<EntityRef>,
        ground: Option<V>,
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
            ground,
            start: now,
            duration,
        });
        if self.effects.len() > 24 {
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
            .any(|e| matches!(e.visual, Visual::Shards))
        {
            return;
        }
        if let Some(a) = target.and_then(|t| anchors.get(&t)) {
            self.add(Visual::Shards, 12, source, target, Some(a.feet), None, now);
        }
    }
    pub fn push(&mut self, s: &ZoneSnapshot, now: f64, anchors: &BTreeMap<EntityRef, Anchor>) {
        let viewer = EntityRef::Player(s.viewer_id);
        for e in &s.events {
            match *e {
                // A directed strike (#154) presents like its ability's swing.
                ZoneEvent::AbilityUsed {
                    source,
                    target,
                    ability,
                }
                | ZoneEvent::StrikeSwept {
                    source,
                    target,
                    ability,
                    ..
                } => {
                    let id = ability.get();
                    let (pose, visual, _) = presentation(id);
                    if source == viewer {
                        if let Some(pose) = pose {
                            self.action = Some((pose, now, self.casting == Some(id)));
                        }
                    }
                    if let Some(visual) = visual {
                        let duration = match visual {
                            Visual::Projectile => Some(0.4),
                            Visual::Arrow => Some(0.22),
                            Visual::Ring => Some(0.65),
                            Visual::Swing => Some(0.28),
                            _ => None,
                        };
                        if let Some(duration) = duration {
                            self.add(visual, id, source, target, None, Some(duration), now);
                        }
                    }
                }
                ZoneEvent::CastStarted {
                    source,
                    target,
                    ability,
                    ..
                } if ability.get() == 12 => self.shards(source, target, now, anchors),
                _ => {}
            }
        }
        self.casting = s.viewer.cast.map(|c| c.ability.get());
        self.sync(s, now, anchors);
    }
    fn sync(&mut self, s: &ZoneSnapshot, now: f64, anchors: &BTreeMap<EntityRef, Anchor>) {
        let channel = s
            .viewer
            .cast
            .is_some_and(|c| c.channel && c.ability.get() == 12);
        let barrier = s
            .auras
            .iter()
            .any(|a| a.ability.get() == 11 && a.kind == AuraKind::Absorb);
        self.effects.retain(|e| {
            e.duration.is_some()
                || if matches!(e.visual, Visual::Shards) {
                    channel
                } else {
                    barrier
                }
        });
        let viewer = EntityRef::Player(s.viewer_id);
        if channel {
            self.shards(viewer, s.viewer.target, now, anchors);
        }
        if barrier
            && !self
                .effects
                .iter()
                .any(|e| matches!(e.visual, Visual::Bubble))
        {
            self.add(Visual::Bubble, 11, viewer, None, None, None, now);
        }
    }
    pub fn action(&self, s: &ZoneSnapshot, now: f64) -> Option<Action> {
        if let Some(c) = s.viewer.cast {
            if let Some(stance) = presentation(c.ability.get()).0 {
                return Some(Action {
                    stance,
                    progress: if c.total > 0 {
                        f64::from(c.elapsed) / f64::from(c.total)
                    } else {
                        1.
                    },
                    hold: true,
                });
            }
        }
        self.action.and_then(|(stance, start, release)| {
            let t = (now - start) / 0.3;
            (t < 1.).then_some(Action {
                stance,
                progress: if release { 0.5 + t / 2. } else { t },
                hold: false,
            })
        })
    }
    pub fn nodes(
        &mut self,
        r: &mut Resources,
        now: f64,
        anchors: &BTreeMap<EntityRef, Anchor>,
    ) -> Vec<SceneDraw> {
        self.effects
            .retain(|e| e.duration.is_none_or(|d| now - e.start < d));
        let mut out = Vec::new();
        for e in &self.effects {
            let age = now - e.start;
            let t = e.duration.map_or(0., |d| (age / d).clamp(0., 1.));
            let source = anchors.get(&e.source);
            let target = e.target.and_then(|t| anchors.get(&t));
            let color = presentation(e.ability).2;
            let prefix = format!("fx-{}", e.id);
            let mut add = |suffix: &str, p: Primitive, c: u32, position: V, q: Q, opacity: f32| {
                let mut d =
                    r.primitive(format!("{prefix}-{suffix}"), p, c, Frame { p: position, q });
                d.material.opacity = opacity;
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
                        add("core", Primitive::Ball(0.2), color, position(t), IQ, 1.);
                        add(
                            "trail",
                            Primitive::Ball(0.12),
                            0xfff2c4,
                            position((t - 0.12).max(0.)),
                            IQ,
                            0.7,
                        );
                    }
                }
                Visual::Arrow => {
                    if let (Some(s), Some(d)) = (source, target) {
                        let from = [s.feet[0], s.centre + 0.2, s.feet[2]];
                        let to = [d.feet[0], d.centre, d.feet[2]];
                        let dir = std::array::from_fn::<_, 3, _>(|i| to[i] - from[i]);
                        let q = qm(
                            qa(1, dir[0].atan2(dir[2])),
                            qa(0, -dir[1].atan2(dir[0].hypot(dir[2]))),
                        );
                        add(
                            "shaft",
                            Primitive::Box([0.05, 0.05, 0.9]),
                            color,
                            std::array::from_fn(|i| lerp(from[i], to[i], t)),
                            q,
                            1.,
                        );
                    }
                }
                Visual::Ring => {
                    if let Some(s) = source {
                        let radius = (8. * (1. - (1. - t) * (1. - t))).max(0.3);
                        for i in 0..20 {
                            let a = i as f64 / 20. * TAU;
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
                            let j = i as f64;
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
                            );
                        }
                    }
                }
                Visual::Swing => {
                    if let Some(s) = source {
                        let sweep = PI * 0.9;
                        for i in 0..6 {
                            let a = s.yaw + sweep / 2. - sweep * t * ((6 - i) as f64 / 6.);
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
                            );
                        }
                    }
                }
            }
        }
        out
    }
}
