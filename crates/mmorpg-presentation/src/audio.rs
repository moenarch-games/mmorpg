//! Sound cues from authoritative projections (#447, Atlas 6.6.37).
//!
//! Both clients feed every received projection to [`AudioCues::push`] and hand the returned
//! [`AudioOp`]s to their `game-audio` director (kira natively, Web Audio in the browser). This
//! module only decides *which* cue a zone event stands for, where it sounds and when a held
//! wind-up must fade; the director owns mixing, voice limits, cooldowns and key dedup.
//!
//! - Cues come only from the viewer's own projection: its events and the units it contains.
//!   An event about a unit the projection does not show is silent, so audio never reveals an
//!   unseen or out-of-interest enemy.
//! - Each cue key is the zone, tick and event index, so the director's key dedup keeps a
//!   reconnect, replayed or late projection silent. A projection older than the last one of
//!   the same session is ignored here as well.
//! - A consequential creature special's wind-up (Stomp, Redbrand Whirl, Overhead Smash) sounds
//!   on its own voice entity ([`windup_entity`]) and fades on interrupt, death, leaving the
//!   projection or an overdue resolution. Its resolution lets the tail finish.
//!
//! Silence (muted, blocked or missing audio) never changes the visual telegraph, which
//! [`crate::units`] draws independently.
use std::collections::{BTreeMap, BTreeSet};

use mmorpg_core::{EntityRef, EntitySnapshot, TICK_HZ, ZoneEvent, ZoneSnapshot};

/// The asset-tooling bundle the cues play (asset-tooling#297, rolling release `combat-sounds`).
pub const BUNDLE: &str = "combat-sounds";
/// Cue kind of a dangerous special's wind-up; mmorpg adds it to the bundle's manifest.
pub const WINDUP: &str = "danger.windup";

/// Creature specials whose wind-up is consequential enough to sound (Atlas 6.6.37).
const DANGEROUS: [u8; 3] = [
    18, // Stomp
    24, // Redbrand Whirl (Garrick)
    25, // Overhead Smash (Redbrand Brute)
];
/// A wind-up with no resolution this long after its expected end is treated as invalidated.
const OVERDUE_TICKS: u64 = TICK_HZ as u64;
const WINDUP_BIT: u64 = 1 << 40;

/// One sound to start: a manifest cue kind on a unit at its rendered centre (metres).
#[derive(Clone, Debug, PartialEq)]
pub struct SoundCue {
    pub key: String,
    pub kind: &'static str,
    pub entity: u64,
    pub position: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioOp {
    Play(SoundCue),
    /// Fade every voice following this audio entity.
    Stop(u64),
}

/// The director entity of a unit: its kind and id, a safe JavaScript integer.
#[must_use]
pub fn audio_entity(entity: EntityRef) -> u64 {
    let kind = match entity {
        EntityRef::Player(_) => 1,
        EntityRef::Creature(_) => 2,
        EntityRef::Npc(_) => 3,
        EntityRef::Minion(_) => 4,
    };
    (kind << 32) | u64::from(entity.id())
}

/// The separate director entity a unit's wind-up sounds on, so fading it never cuts an impact.
#[must_use]
pub fn windup_entity(entity: EntityRef) -> u64 {
    audio_entity(entity) | WINDUP_BIT
}

/// Adds mmorpg's cues and distance curve to the bundle's `SoundManifest` JSON.
///
/// # Errors
///
/// The bundle manifest is not a JSON object with a `cues` array.
pub fn sound_manifest(bundle: &str) -> Result<String, String> {
    let mut manifest: serde_json::Value =
        serde_json::from_str(bundle).map_err(|error| format!("sound manifest: {error}"))?;
    let object = manifest
        .as_object_mut()
        .ok_or("sound manifest is not an object")?;
    // Units are metres: full volume within arm's reach, silent beyond a long bow shot.
    object.insert(
        "attenuation".into(),
        serde_json::json!({ "min_distance": 3.0, "max_distance": 45.0 }),
    );
    object
        .get_mut("cues")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("sound manifest has no cues")?
        .push(serde_json::json!({
            // Restrained: below an impact, spatial, one per caster. The bundle has no
            // dedicated wind-up recording yet; the counter-open chime stands in (#447).
            "kind": WINDUP,
            "clips": ["combat.counter-open.1"],
            "bus": "effects",
            "priority": 90,
            "max_voices": 2,
            "cooldown_ms": 0,
            "gain_db": -4.0,
            "pitch_jitter": 0.0,
            "spatial": true,
        }));
    serde_json::to_string(&manifest).map_err(|error| error.to_string())
}

#[derive(Clone, Copy)]
struct Windup {
    ability: u8,
    end_tick: u64,
}

/// Per-client cue state: the session, the last projected tick and the held wind-ups.
#[derive(Default)]
pub struct AudioCues {
    session: Option<(u32, u32, u64)>,
    last_tick: Option<u64>,
    visible: BTreeSet<EntityRef>,
    windups: BTreeMap<EntityRef, Windup>,
}

impl AudioCues {
    /// A new connection: forget the session so its first projection is accepted. The director's
    /// remembered keys still silence anything already heard.
    pub fn reset(&mut self) -> Vec<AudioOp> {
        let ops = self
            .windups
            .keys()
            .map(|&entity| AudioOp::Stop(windup_entity(entity)))
            .collect();
        *self = Self::default();
        ops
    }

    /// The cues and fades of one received projection; `centre` is a unit's rendered centre.
    pub fn push(
        &mut self,
        s: &ZoneSnapshot,
        centre: impl Fn(&EntitySnapshot) -> [f32; 3],
    ) -> Vec<AudioOp> {
        let session = (s.zone_id.get(), s.viewer_id, s.content_revision);
        let mut ops = Vec::new();
        if self.session != Some(session) {
            ops = self.reset();
            self.session = Some(session);
        } else if self.last_tick.is_some_and(|tick| s.tick <= tick) {
            return ops;
        }
        self.last_tick = Some(s.tick);
        let units: BTreeMap<EntityRef, &EntitySnapshot> =
            s.entities.iter().map(|e| (e.entity(), e)).collect();
        // Units that left the projection fade everything they play.
        for gone in self.visible.iter().filter(|e| !units.contains_key(e)) {
            ops.push(AudioOp::Stop(audio_entity(*gone)));
            if self.windups.remove(gone).is_some() {
                ops.push(AudioOp::Stop(windup_entity(*gone)));
            }
        }
        self.visible = units.keys().copied().collect();
        let viewer = EntityRef::Player(s.viewer_id);
        let died: BTreeSet<EntityRef> = s
            .events
            .iter()
            .filter_map(|event| match *event {
                ZoneEvent::Died { entity, .. } => Some(entity),
                _ => None,
            })
            .collect();
        let zone = s.zone_id.get();
        let tick = s.tick;
        for (index, event) in s.events.iter().enumerate() {
            let sound = match *event {
                ZoneEvent::DamageDealt { target, amount, .. }
                | ZoneEvent::DamageObserved { target, amount, .. }
                    if amount > 0 =>
                {
                    let kind = if died.contains(&target) {
                        "combat.hit.defeated"
                    } else {
                        "combat.hit"
                    };
                    Some((kind, target, audio_entity(target)))
                }
                ZoneEvent::DamageTaken { target, amount, .. } if amount > 0 => {
                    Some(("combat.hurt", target, audio_entity(target)))
                }
                ZoneEvent::Blocked { target, .. } => {
                    Some(("combat.blocked", target, audio_entity(target)))
                }
                ZoneEvent::GuardBroken { target, .. } => {
                    Some(("combat.guardBroken", target, audio_entity(target)))
                }
                ZoneEvent::Miss { source, .. } if source == viewer => {
                    Some(("combat.whiff", source, audio_entity(source)))
                }
                ZoneEvent::Error { .. } => Some(("world.refused", viewer, audio_entity(viewer))),
                ZoneEvent::Died { entity, .. } if entity == viewer => {
                    Some(("combat.defeat", viewer, audio_entity(viewer)))
                }
                ZoneEvent::CastStarted {
                    source: source @ EntityRef::Creature(_),
                    ability,
                    ticks,
                    ..
                } if DANGEROUS.contains(&ability.get())
                    && !died.contains(&source)
                    && units.contains_key(&source) =>
                {
                    if self.windups.contains_key(&source) {
                        ops.push(AudioOp::Stop(windup_entity(source)));
                    }
                    self.windups.insert(
                        source,
                        Windup {
                            ability: ability.get(),
                            end_tick: tick + u64::from(ticks),
                        },
                    );
                    Some((WINDUP, source, windup_entity(source)))
                }
                ZoneEvent::AbilityUsed {
                    source, ability, ..
                } if self
                    .windups
                    .get(&source)
                    .is_some_and(|w| w.ability == ability.get()) =>
                {
                    // Resolved: the tail is cosmetic and may finish.
                    self.windups.remove(&source);
                    None
                }
                ZoneEvent::Interrupted {
                    target, ability, ..
                } if self
                    .windups
                    .get(&target)
                    .is_some_and(|w| w.ability == ability.get()) =>
                {
                    self.windups.remove(&target);
                    ops.push(AudioOp::Stop(windup_entity(target)));
                    None
                }
                _ => None,
            };
            if let Some((kind, on, entity)) = sound
                && let Some(unit) = units.get(&on)
            {
                ops.push(AudioOp::Play(SoundCue {
                    key: format!("{zone}/{tick}/{index}"),
                    kind,
                    entity,
                    position: centre(unit),
                }));
            }
        }
        // Death, a dead projection or an overdue resolution ends a wind-up.
        self.windups.retain(|entity, windup| {
            let dead = died.contains(entity) || units.get(entity).is_some_and(|u| u.flags.dead);
            let ended = dead || tick > windup.end_tick + OVERDUE_TICKS;
            if ended {
                ops.push(AudioOp::Stop(windup_entity(*entity)));
            }
            !ended
        });
        ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{AbilityId, CreatureId, EntityFlags, EntityKind, ZoneId};

    fn unit(kind: EntityKind, id: u32) -> EntitySnapshot {
        EntitySnapshot {
            kind,
            id,
            appearance: 0,
            position: [100 * i32::try_from(id).unwrap(), 0, 0],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        }
    }

    fn snapshot(tick: u64, entities: Vec<EntitySnapshot>, events: Vec<ZoneEvent>) -> ZoneSnapshot {
        ZoneSnapshot {
            doors: Vec::new(),
            projectiles: Vec::new(),
            debris: Vec::new(),
            pieces: Vec::new(),
            raid: None,
            content_revision: 6,
            acknowledged_sequence: 1,
            viewer_id: 1,
            zone_id: ZoneId::new(1),
            tick,
            viewer: mmorpg_core::ViewerState::default(),
            cooldowns: Vec::new(),
            auras: Vec::new(),
            target_of_target: None,
            target_detail: mmorpg_core::TargetDetail::default(),
            inventory_revision: 1,
            inventory: None,
            equipment: None,
            loot: None,
            events,
            chat: Vec::new(),
            entities,
            quests: None,
        }
    }

    const BRUTE: EntityRef = EntityRef::Creature(CreatureId::new(7));

    fn centre(e: &EntitySnapshot) -> [f32; 3] {
        [e.position[0] as f32 / 100., 1., 0.]
    }

    fn both() -> Vec<EntitySnapshot> {
        vec![unit(EntityKind::Player, 1), unit(EntityKind::Creature, 7)]
    }

    fn windup() -> ZoneEvent {
        ZoneEvent::CastStarted {
            source: BRUTE,
            target: None,
            ability: AbilityId::new(25),
            ticks: 40,
        }
    }

    fn plays(ops: &[AudioOp]) -> Vec<(&'static str, u64)> {
        ops.iter()
            .filter_map(|op| match op {
                AudioOp::Play(cue) => Some((cue.kind, cue.entity)),
                AudioOp::Stop(_) => None,
            })
            .collect()
    }

    #[test]
    fn impacts_sound_on_their_target_once() {
        let mut cues = AudioCues::default();
        let hit = ZoneEvent::DamageDealt {
            source: EntityRef::Player(1),
            target: BRUTE,
            amount: 5,
            critical: false,
        };
        let first = snapshot(10, both(), vec![hit]);
        let ops = cues.push(&first, centre);
        assert_eq!(plays(&ops), [("combat.hit", audio_entity(BRUTE))]);
        let AudioOp::Play(cue) = &ops[0] else {
            unreachable!()
        };
        assert_eq!(cue.key, "1/10/0");
        assert_eq!(cue.position, [7., 1., 0.]);
        // A repeated or late projection of the same session stays silent.
        assert!(cues.push(&first, centre).is_empty());
        assert!(
            cues.push(&snapshot(9, both(), vec![hit]), centre)
                .is_empty()
        );
    }

    #[test]
    fn events_about_unprojected_units_are_silent() {
        let mut cues = AudioCues::default();
        let hidden = snapshot(
            5,
            vec![unit(EntityKind::Player, 1)],
            vec![
                windup(),
                ZoneEvent::DamageObserved {
                    source: EntityRef::Player(2),
                    target: BRUTE,
                    amount: 3,
                    critical: false,
                },
            ],
        );
        assert!(plays(&cues.push(&hidden, centre)).is_empty());
    }

    #[test]
    fn windup_fades_on_interrupt_death_and_leaving_but_not_on_resolution() {
        let interrupted = ZoneEvent::Interrupted {
            source: Some(EntityRef::Player(1)),
            target: BRUTE,
            ability: AbilityId::new(25),
        };
        let died = ZoneEvent::Died {
            entity: BRUTE,
            killer: None,
        };
        let resolved = ZoneEvent::AbilityUsed {
            source: BRUTE,
            target: None,
            ability: AbilityId::new(25),
        };
        let stop = AudioOp::Stop(windup_entity(BRUTE));
        for (end, faded) in [
            (snapshot(11, both(), vec![interrupted]), true),
            (snapshot(11, both(), vec![died]), true),
            (
                snapshot(11, vec![unit(EntityKind::Player, 1)], vec![]),
                true,
            ),
            (snapshot(11, both(), vec![resolved]), false),
            // Overdue: past the 40-tick wind-up plus a second's grace, at any tick rate.
            (snapshot(10 + 40 + OVERDUE_TICKS + 1, both(), vec![]), true),
        ] {
            let mut cues = AudioCues::default();
            let ops = cues.push(&snapshot(10, both(), vec![windup()]), centre);
            assert_eq!(plays(&ops), [(WINDUP, windup_entity(BRUTE))]);
            assert_eq!(cues.push(&end, centre).contains(&stop), faded);
        }
    }

    #[test]
    fn manifest_gains_the_windup_cue() {
        let manifest = sound_manifest(
            r#"{"clips":[{"id":"combat.counter-open.1","source":{"url":"a.wav"}}],"cues":[]}"#,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(value["cues"][0]["kind"], WINDUP);
        assert_eq!(value["attenuation"]["max_distance"], 45.0);
    }
}
