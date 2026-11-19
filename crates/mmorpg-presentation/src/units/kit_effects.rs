//! Strike and spell effects from asset-tooling's rolling `strike-effects` and `spell-effects`
//! releases (#894), placed for both clients.
//!
//! A basic strike (#885) shows its arc or thrust at the zone's contact, a jumping strike's
//! landing slams the ground and a running strike kicks up dust as it starts. Every hit on a body
//! bursts, a spell missile's end plays its school's impact, Frost Nova rings out, heals rise in
//! motes, and the shots and missiles the zone projects fly as the bundle's meshes. A directed
//! slash that strikes a second unit sweeps the wide cleave fan instead of its arc (#963), and
//! while the viewer's cleave follow-up window is open its sword glints. One-shot
//! effects are pooled here and dropped once their `Play` clip ends; reduced motion skips them.
//! Clients draw each [`Instance`]: the entry `key` under `model`, its `clip` at `time` (a `Loop`
//! clip wraps), recoloured through its material's base and emissive colour by `tint`. Nothing
//! here feeds back into the zone.
use super::{
    effects::Anchor,
    kit::{Frame, IQ, model, qa, qm},
};
use crate::geometry::V;
use mmorpg_core::{
    AbilityId, ComboPhase, EntityRef, ProjectileKind, StrikeSide, StrikeVariant, TICK_HZ,
    WeaponFamily, ZoneEvent, ZoneSnapshot, ability::ids,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    f64::consts::{FRAC_PI_2, PI, TAU},
};

/// The rolling releases, their bundle names and where `scripts/fetch-bundles.sh` places them
/// under `web/public`.
pub const STRIKE_RELEASE: &str = "strike-effects";
pub const STRIKE_BUNDLE: &str = "effects/strike.effects";
pub const SPELL_RELEASE: &str = "spell-effects";
pub const SPELL_BUNDLE: &str = "effects/spell.effects";

pub const ARC_CUT: &str = "strike.effects.strike-arc-cut";
pub const ARC_HEAVY: &str = "strike.effects.strike-arc-heavy";
pub const CLEAVE_SWEEP: &str = "strike.effects.cleave-sweep";
pub const COMBO_GLINT: &str = "strike.effects.combo-glint";
pub const THRUST: &str = "strike.effects.strike-thrust";
pub const IMPACT_BURST: &str = "strike.effects.impact-burst";
pub const GROUND_SLAM: &str = "strike.effects.ground-slam";
pub const LUNGE_DUST: &str = "strike.effects.lunge-dust";
pub const FROST_NOVA: &str = "spell.effects.frost-nova";
pub const BEAM_SEGMENT: &str = "spell.effects.beam-segment";
pub const HEAL_MOTES: &str = "spell.effects.heal-motes";
/// A spent corpse (#935) and its burst's hits.
pub const CORPSE_SPENT: &str = "spell.effects.impact-shadow";
/// A root and a freeze at the held unit's feet (#956, asset-tooling #772): each loop holds
/// while the zone's aura does, and its end plays from the loop's resting pose once it breaks.
pub const ROOTS_HOLD: &str = "spell.effects.roots-hold-loop";
pub const ROOTS_HOLD_END: &str = "spell.effects.roots-hold-end";
pub const FROST_HOLD: &str = "spell.effects.frost-hold-loop";
pub const FROST_HOLD_END: &str = "spell.effects.frost-hold-end";

/// Every entry the clients load: its key, its clip and the clip's length in seconds (the
/// bundle descriptors'); a shot has no clip.
pub const ENTRIES: [(&str, Option<&str>, f64); 27] = [
    (ARC_CUT, Some("Play"), 0.18),
    (ARC_HEAVY, Some("Play"), 0.25),
    (CLEAVE_SWEEP, Some("Play"), 0.3),
    (COMBO_GLINT, Some("Play"), 0.3),
    (THRUST, Some("Play"), 0.15),
    (IMPACT_BURST, Some("Play"), 0.2),
    (GROUND_SLAM, Some("Play"), 0.4),
    (LUNGE_DUST, Some("Play"), 0.35),
    ("spell.effects.arrow", None, 0.),
    ("spell.effects.crossbow-bolt", None, 0.),
    ("spell.effects.throwing-knife", None, 0.),
    ("spell.effects.bolt-fire", Some("Loop"), 0.4),
    ("spell.effects.bolt-shadow", Some("Loop"), 0.6),
    ("spell.effects.bolt-holy", Some("Loop"), 0.5),
    ("spell.effects.bolt-nature", Some("Loop"), 0.6),
    ("spell.effects.impact-fire", Some("Play"), 0.35),
    ("spell.effects.impact-shadow", Some("Play"), 0.4),
    ("spell.effects.impact-holy", Some("Play"), 0.4),
    ("spell.effects.impact-nature", Some("Play"), 0.45),
    ("spell.effects.impact-frost", Some("Play"), 0.35),
    (FROST_NOVA, Some("Play"), 0.5),
    (BEAM_SEGMENT, Some("Loop"), 0.24),
    (HEAL_MOTES, Some("Play"), 0.8),
    (ROOTS_HOLD, Some("Loop"), 1.2),
    (ROOTS_HOLD_END, Some("Play"), 0.35),
    (FROST_HOLD, Some("Loop"), 1.),
    (FROST_HOLD_END, Some("Play"), 0.35),
];

/// The arcs' swing radii (the descriptor's `swingRadius`): the contact lies this far ahead of
/// the striker's chest, so the arc curves back round its body.
const CUT_RADIUS: f64 = 1.15;
const HEAVY_RADIUS: f64 = 1.55;
const CLEAVE_RADIUS: f64 = 1.6;
/// A directed slash's side turns its sweep this far off the facing (the primitive sweep's).
const SIDE_TURN: f64 = std::f64::consts::FRAC_PI_4;
/// Where the sword held in its guard glints, from the body centre: ahead, to the right and up.
const GUARD_BLADE: [f64; 3] = [0.45, 0.3, 0.35];
/// How far ahead of the chest a thrust's point meets its target.
const THRUST_REACH: f64 = 1.1;
/// The chest above the body centre, where strikes contact.
const CHEST: f64 = 0.25;
/// One-shot effects live at once; the oldest goes first.
const POOL: usize = 48;
/// Seconds a pending strike waits for its contact before it is dropped.
const LATE: f64 = 1.;
/// Offsets that keep the ids of shots and beams apart from the pooled effects'.
const SHOT_IDS: u64 = 1 << 48;
const BEAM_IDS: u64 = 2 << 48;
const HOLD_IDS: u64 = 3 << 48;

/// One effect to draw this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// Stable while the effect lives.
    pub id: u64,
    pub key: &'static str,
    /// Column-major model matrix, metres.
    pub model: [f32; 16],
    /// The clip and seconds into it; a `Loop` clip's time wraps in the client.
    pub clip: Option<&'static str>,
    pub time: f32,
    /// 0–1 sRGB channels for the material's base and emissive colour; `None` draws it as authored.
    pub tint: Option<[f32; 3]>,
}

#[derive(Clone, Debug)]
struct Spawned {
    id: u64,
    key: &'static str,
    frame: Frame,
    start: f64,
    duration: f64,
}

/// A strike waiting for its contact.
#[derive(Clone, Copy, Debug)]
struct Pending {
    source: EntityRef,
    key: &'static str,
    backhand: bool,
    slam: bool,
    at: f64,
}

/// A magic missile last seen in flight: its school's impact and where it was.
#[derive(Clone, Copy, Debug)]
struct Flight {
    impact: &'static str,
    position: V,
    yaw: f64,
}

/// The pooled one-shot effects and the strikes awaiting contact.
#[derive(Debug, Default)]
pub struct KitEffects {
    /// The client draws the bundles: shots, beams and Frost Nova leave their primitives.
    drawn: bool,
    spawned: Vec<Spawned>,
    pending: Vec<Pending>,
    flights: BTreeMap<u16, Flight>,
    /// The units each running directed slash has struck so far (#909).
    struck: BTreeMap<EntityRef, u8>,
    /// The viewer whose cleave follow-up window (#910) is open, and when its glint next plays.
    glint: Option<(EntityRef, f64)>,
    next: u64,
    last_tick: Option<u64>,
}

/// The arc or thrust of `family`'s `variant`: the thrust for spears and daggers, the heavy arc
/// for finishers, jumping strikes, maces and staves, the cut arc for the other chain hits.
#[must_use]
pub fn strike_key(family: WeaponFamily, variant: StrikeVariant) -> &'static str {
    match (family, variant) {
        (WeaponFamily::Spear | WeaponFamily::Dagger, _) => THRUST,
        (_, StrikeVariant::Finisher | StrikeVariant::Jumping)
        | (WeaponFamily::Mace | WeaponFamily::Staff, _) => ARC_HEAVY,
        _ => ARC_CUT,
    }
}

/// The flying mesh of a projectile kind and its school's impact; flasks and bombs have none.
#[must_use]
pub fn shot_keys(kind: ProjectileKind) -> Option<(&'static str, Option<&'static str>)> {
    Some(match kind {
        ProjectileKind::Arrow => ("spell.effects.arrow", None),
        ProjectileKind::Bolt => ("spell.effects.crossbow-bolt", None),
        ProjectileKind::Knife => ("spell.effects.throwing-knife", None),
        ProjectileKind::Missile => ("spell.effects.bolt-fire", Some("spell.effects.impact-fire")),
        ProjectileKind::Shadow => (
            "spell.effects.bolt-shadow",
            Some("spell.effects.impact-shadow"),
        ),
        ProjectileKind::Holy => ("spell.effects.bolt-holy", Some("spell.effects.impact-holy")),
        ProjectileKind::Nature => (
            "spell.effects.bolt-nature",
            Some("spell.effects.impact-nature"),
        ),
        ProjectileKind::Flask | ProjectileKind::Bomb => return None,
    })
}

/// The school impact an instant spell's hits play instead of the plain burst.
fn spell_impact(ability: AbilityId) -> Option<&'static str> {
    match ability {
        ids::FROST_NOVA | ids::FROST_BREATH => Some("spell.effects.impact-frost"),
        ids::FIRE_BLAST => Some("spell.effects.impact-fire"),
        ids::CORPSE_BURST => Some(CORPSE_SPENT),
        _ => None,
    }
}

fn entry(key: &str) -> (Option<&'static str>, f64) {
    ENTRIES
        .iter()
        .find(|(entry, ..)| *entry == key)
        .map_or((None, 0.), |&(_, clip, duration)| (clip, duration))
}

/// Facing `yaw` (0 toward +Z, turning toward +X) as a rotation of +Z.
fn heading(yaw: f64) -> [f64; 4] {
    qa(1, yaw)
}

impl KitEffects {
    /// Whether the client draws the bundles (both clients once they hold them).
    pub fn set_drawn(&mut self, drawn: bool) {
        self.drawn = drawn;
    }

    #[must_use]
    pub fn drawn(&self) -> bool {
        self.drawn
    }

    pub fn clear(&mut self) {
        *self = Self {
            drawn: self.drawn,
            next: self.next,
            ..Self::default()
        };
    }

    fn spawn(&mut self, key: &'static str, frame: Frame, now: f64) {
        let (_, duration) = entry(key);
        self.next += 1;
        self.spawned.push(Spawned {
            id: self.next,
            key,
            frame,
            start: now,
            duration,
        });
        if self.spawned.len() > POOL {
            self.spawned.remove(0);
        }
    }

    /// Reads one received projection's events at presentation time `now`; `anchors` are where
    /// its units stand and `rendered` places a zone position on the drawn ground.
    pub fn push(
        &mut self,
        s: &ZoneSnapshot,
        now: f64,
        anchors: &BTreeMap<EntityRef, Anchor>,
        rendered: impl Fn([i32; 3]) -> [f32; 3],
    ) {
        if !self.drawn {
            return;
        }
        if self.last_tick.is_some_and(|tick| s.tick < tick) {
            self.clear();
        }
        if self.last_tick == Some(s.tick) {
            return;
        }
        self.last_tick = Some(s.tick);
        // The viewer's sword glints while its cleave follow-up window is open (#910).
        self.glint = (s.viewer.combo == Some(ComboPhase::CleaveOpen)).then(|| {
            (
                EntityRef::Player(s.viewer_id),
                self.glint.map_or(now, |(_, next)| next),
            )
        });
        // Missiles gone since the last projection ended: each plays its school's impact where it
        // was last seen, and its hit shows no plain burst.
        let mut landed = BTreeSet::new();
        let mut flights = BTreeMap::new();
        for projectile in &s.projectiles {
            if let Some((_, Some(impact))) = shot_keys(projectile.kind) {
                flights.insert(
                    projectile.id,
                    Flight {
                        impact,
                        position: rendered(projectile.position).map(f64::from),
                        yaw: f64::from(projectile.yaw) / 65_536. * TAU,
                    },
                );
            }
        }
        for (id, flight) in std::mem::replace(&mut self.flights, flights) {
            if self.flights.contains_key(&id) {
                continue;
            }
            if let Some((entity, _)) = anchors.iter().find(|(_, a)| {
                (a.feet[0] - flight.position[0]).hypot(a.feet[2] - flight.position[2]) < 2.
            }) {
                landed.insert(*entity);
            }
            self.spawn(
                flight.impact,
                Frame {
                    p: flight.position,
                    q: heading(flight.yaw),
                },
                now,
            );
        }
        let schools: BTreeMap<EntityRef, &'static str> = s
            .events
            .iter()
            .filter_map(|event| match *event {
                ZoneEvent::AbilityUsed {
                    source, ability, ..
                } => spell_impact(ability).map(|impact| (source, impact)),
                _ => None,
            })
            .collect();
        for event in &s.events {
            match *event {
                ZoneEvent::StrikeBegan {
                    source,
                    family,
                    variant,
                    contact,
                } => {
                    let Some(a) = anchors.get(&source) else {
                        continue;
                    };
                    if variant == StrikeVariant::Running {
                        self.spawn(
                            LUNGE_DUST,
                            Frame {
                                p: a.feet,
                                q: heading(a.yaw),
                            },
                            now,
                        );
                    }
                    self.pending.push(Pending {
                        source,
                        key: strike_key(family, variant),
                        backhand: variant == StrikeVariant::Second,
                        slam: variant == StrikeVariant::Jumping,
                        at: now + f64::from(contact) / f64::from(TICK_HZ),
                    });
                }
                ZoneEvent::DamageDealt {
                    source,
                    target,
                    amount,
                    ..
                }
                | ZoneEvent::DamageTaken {
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
                } if amount > 0 && !landed.contains(&target) => {
                    let (Some(from), Some(to)) = (anchors.get(&source), anchors.get(&target))
                    else {
                        continue;
                    };
                    let yaw = (to.feet[0] - from.feet[0]).atan2(to.feet[2] - from.feet[2]);
                    self.spawn(
                        schools.get(&source).copied().unwrap_or(IMPACT_BURST),
                        Frame {
                            p: [to.feet[0], to.centre + CHEST, to.feet[2]],
                            q: heading(yaw),
                        },
                        now,
                    );
                }
                // A directed slash (#909) counts the units it strikes; the second turns its
                // sweep into the cleave's wide fan.
                ZoneEvent::SwingBegan { source, .. } => {
                    self.struck.insert(source, 0);
                }
                ZoneEvent::StrikeSwept {
                    source,
                    target: Some(_),
                    aim,
                    ..
                } => {
                    let struck = self.struck.entry(source).or_default();
                    *struck = struck.saturating_add(1);
                    if *struck != 2 {
                        continue;
                    }
                    let Some(a) = anchors.get(&source) else {
                        continue;
                    };
                    let yaw = a.yaw
                        + match aim.side {
                            StrikeSide::Left => SIDE_TURN,
                            StrikeSide::Centre => 0.,
                            StrikeSide::Right => -SIDE_TURN,
                        };
                    // Like an arc: local −X back at the striker, +Z across the body.
                    let frame = Frame {
                        p: [
                            a.feet[0] + yaw.sin() * CLEAVE_RADIUS,
                            a.centre + CHEST,
                            a.feet[2] + yaw.cos() * CLEAVE_RADIUS,
                        ],
                        q: heading(yaw - FRAC_PI_2),
                    };
                    self.spawn(CLEAVE_SWEEP, frame, now);
                }
                ZoneEvent::AbilityUsed {
                    source, ability, ..
                } if ability == ids::FROST_NOVA => {
                    if let Some(a) = anchors.get(&source) {
                        self.spawn(FROST_NOVA, Frame { p: a.feet, q: IQ }, now);
                    }
                }
                // A spent corpse (#935) flares where it lies as it rises or bursts.
                ZoneEvent::AbilityUsed {
                    source,
                    target,
                    ability,
                } if ability == ids::RAISE_DEAD || ability == ids::CORPSE_BURST => {
                    if let Some(a) = target
                        .and_then(|corpse| anchors.get(&corpse))
                        .or_else(|| anchors.get(&source))
                    {
                        self.spawn(CORPSE_SPENT, Frame { p: a.feet, q: IQ }, now);
                    }
                }
                ZoneEvent::Healed { target, amount, .. } if amount > 0 => {
                    if let Some(a) = anchors.get(&target) {
                        self.spawn(HEAL_MOTES, Frame { p: a.feet, q: IQ }, now);
                    }
                }
                _ => {}
            }
        }
    }

    /// The pooled effects alive at `now`, after placing every strike whose contact has come
    /// where its striker stands now (`anchors`); with `reduced` motion none play.
    pub fn frame(
        &mut self,
        now: f64,
        anchors: &BTreeMap<EntityRef, Anchor>,
        reduced: bool,
    ) -> Vec<Instance> {
        if reduced {
            self.spawned.clear();
            self.pending.clear();
            return Vec::new();
        }
        let (due, waiting): (Vec<_>, Vec<_>) =
            self.pending.drain(..).partition(|strike| strike.at <= now);
        self.pending = waiting;
        for strike in due {
            let Some(a) = anchors.get(&strike.source) else {
                continue;
            };
            if now - strike.at > LATE {
                continue;
            }
            let forward = [a.yaw.sin(), 0., a.yaw.cos()];
            let chest = [a.feet[0], a.centre + CHEST, a.feet[2]];
            let ahead = |reach: f64| std::array::from_fn(|i| chest[i] + forward[i] * reach);
            let frame = match strike.key {
                // The thrust's +Z runs along the facing, its trail back toward the striker.
                THRUST => Frame {
                    p: ahead(THRUST_REACH),
                    q: heading(a.yaw),
                },
                // An arc's local −X points back at its swing centre, the striker; its +Z sweeps
                // across the body, the other way on a backhand.
                key => {
                    let q = heading(a.yaw - FRAC_PI_2);
                    Frame {
                        p: ahead(if key == ARC_HEAVY {
                            HEAVY_RADIUS
                        } else {
                            CUT_RADIUS
                        }),
                        q: if strike.backhand { qm(q, qa(0, PI)) } else { q },
                    }
                }
            };
            self.spawn(strike.key, frame, strike.at);
            if strike.slam {
                self.spawn(
                    GROUND_SLAM,
                    Frame {
                        p: a.feet,
                        q: heading(a.yaw),
                    },
                    strike.at,
                );
            }
        }
        // The follow-up window's glint plays again each time the last one ends, at the sword
        // held in the guard.
        if let Some((viewer, next)) = self.glint
            && now >= next
            && let Some(a) = anchors.get(&viewer)
        {
            let [ahead, right, up] = GUARD_BLADE;
            let (sin, cos) = a.yaw.sin_cos();
            let p = [
                a.feet[0] + sin * ahead - cos * right,
                a.centre + up,
                a.feet[2] + cos * ahead + sin * right,
            ];
            self.spawn(
                COMBO_GLINT,
                Frame {
                    p,
                    q: heading(a.yaw),
                },
                now,
            );
            self.glint = Some((viewer, now + entry(COMBO_GLINT).1));
        }
        self.spawned.retain(|e| now - e.start < e.duration);
        self.spawned
            .iter()
            .filter(|e| now >= e.start)
            .map(|e| {
                let (clip, _) = entry(e.key);
                #[allow(clippy::cast_possible_truncation)]
                Instance {
                    id: e.id,
                    key: e.key,
                    model: model(e.frame, [1.; 3]),
                    clip,
                    time: (now - e.start) as f32,
                    tint: None,
                }
            })
            .collect()
    }

    /// A projected shot or missile `id` of `kind` with its tip at `tip`, on its loop at `now`;
    /// a thrown knife spins end over end.
    #[must_use]
    pub fn shot(id: u16, kind: ProjectileKind, tip: Frame, now: f64) -> Option<Instance> {
        let (key, _) = shot_keys(kind)?;
        let (clip, _) = entry(key);
        let tip = if kind == ProjectileKind::Knife {
            Frame {
                p: tip.p,
                q: qm(tip.q, qa(0, (now * 3.).fract() * TAU)),
            }
        } else {
            tip
        };
        #[allow(clippy::cast_possible_truncation)]
        Some(Instance {
            id: SHOT_IDS + u64::from(id),
            key,
            model: model(tip, [1.; 3]),
            clip,
            time: now as f32,
            tint: None,
        })
    }

    /// Effect `id`'s beam from `origin` along `yaw` for `length` metres: one segment stretched
    /// along it, recoloured `tint`.
    #[must_use]
    pub fn beam(id: u64, origin: V, yaw: f64, length: f64, tint: [f32; 3], now: f64) -> Instance {
        #[allow(clippy::cast_possible_truncation)]
        Instance {
            id: BEAM_IDS + id,
            key: BEAM_SEGMENT,
            model: model(
                Frame {
                    p: origin,
                    q: heading(yaw),
                },
                [1., 1., length],
            ),
            clip: Some("Loop"),
            time: now as f32,
            tint: Some(tint),
        }
    }

    /// Effect `id`'s root or freeze (#956) at `feet`, facing `yaw`: its hold loop `age`
    /// seconds in while the zone holds the unit, or its end `ended` seconds in once it broke.
    #[must_use]
    pub fn hold(id: u64, frost: bool, feet: V, yaw: f64, age: f64, ended: Option<f64>) -> Instance {
        let (key, time) = match (frost, ended) {
            (false, None) => (ROOTS_HOLD, age),
            (true, None) => (FROST_HOLD, age),
            (false, Some(ended)) => (ROOTS_HOLD_END, ended),
            (true, Some(ended)) => (FROST_HOLD_END, ended),
        };
        #[allow(clippy::cast_possible_truncation)]
        Instance {
            id: HOLD_IDS + id * 2 + u64::from(ended.is_some()),
            key,
            model: model(
                Frame {
                    p: feet,
                    q: heading(yaw),
                },
                [1.; 3],
            ),
            clip: entry(key).0,
            time: time as f32,
            tint: None,
        }
    }
}
