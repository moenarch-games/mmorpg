//! Floating combat text from a projection's events (#1234), shared by both clients: which
//! events float which text over which unit, the periodic-damage reading, and the cleave, pierce
//! and mastery texts a stream of projections adds. Purely cosmetic: events are lossy, and
//! everything shown already appears in the frames and logs. The native client draws the texts
//! over their units; the browser calls them through `mmorpg-wasm` and lists them in its HUD.
use mmorpg_core::{AuraKind, EntityRef, StrikeHeight, ZoneEvent, ZoneSnapshot};

/// How long a floating text lives, in milliseconds.
pub const COMBAT_TEXT_MS: u64 = 1_400;

/// What a floating text says, which picks its look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatTextKind {
    Damage,
    Critical,
    Periodic,
    Taken,
    Heal,
    Absorb,
    Interrupt,
    Miss,
    /// The viewer's guard turned a blow (#156).
    Block,
    /// A blow overpowered the viewer's guard (#156).
    GuardBreak,
    /// One swing struck several units (#909), or a Piercing Shot several (#932).
    Cleave,
    /// A Sinister Strike from the victim's flank (#949).
    Flank,
    /// A sword mastery level gained in play (#915).
    Mastery,
}

/// One floating text and the unit it rises from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombatText {
    pub kind: CombatTextKind,
    pub text: String,
    pub anchor: EntityRef,
}

const fn lane_name(lane: StrikeHeight) -> &'static str {
    match lane {
        StrikeHeight::Low => "low",
        StrikeHeight::Middle => "middle",
        StrikeHeight::High => "high",
    }
}

/// Whether the viewer's damage in this projection can only have come from a damage-over-time
/// tick or a channel pulse. Damage events carry no provenance, so this is claimed only when no
/// other source of the viewer's damage can be active this tick: no ability resolved, no swing
/// (#517) landed, and a channel is running or the target carries a damage-over-time aura.
/// Anything ambiguous shows as ordinary damage.
#[must_use]
pub fn damage_is_periodic(snapshot: &ZoneSnapshot) -> bool {
    let viewer = EntityRef::Player(snapshot.viewer_id);
    let resolved = snapshot.events.iter().any(|event| {
        matches!(event,
            ZoneEvent::AbilityUsed { source, .. }
                | ZoneEvent::StrikeSwept { source, .. }
                | ZoneEvent::AttackPerformed { source, .. }
                if *source == viewer)
    });
    if resolved {
        return false;
    }
    snapshot.viewer.cast.is_some_and(|cast| cast.channel)
        || snapshot
            .target_detail
            .auras
            .iter()
            .any(|aura| aura.kind == AuraKind::DamageOverTime)
}

/// Whether this projection interrupted the viewer's own cast.
#[must_use]
pub fn viewer_interrupted(snapshot: &ZoneSnapshot) -> bool {
    let viewer = EntityRef::Player(snapshot.viewer_id);
    snapshot
        .events
        .iter()
        .any(|event| matches!(*event, ZoneEvent::Interrupted { target, .. } if target == viewer))
}

/// How many units the viewer's current swing has struck after this projection, from `count`
/// before it (#909): the viewer's wind-up starts a new swing, and each of its sweeps that struck
/// a unit counts one. A cleaving timed swing strikes over several ticks, so the count spans
/// projections.
#[must_use]
pub fn cleave_count(snapshot: &ZoneSnapshot, count: usize) -> usize {
    let viewer = EntityRef::Player(snapshot.viewer_id);
    snapshot
        .events
        .iter()
        .fold(count, |count, event| match *event {
            ZoneEvent::SwingBegan { source, .. } if source == viewer => 0,
            ZoneEvent::StrikeSwept {
                source,
                target: Some(_),
                ..
            } if source == viewer => count + 1,
            _ => count,
        })
}

/// The viewer's latest Piercing Shot (#932): the tick it was loosed and how many units it struck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PierceShot {
    pub loosed: u64,
    pub struck: usize,
}

/// The viewer's latest Piercing Shot after this projection, from `shot` before it (#932):
/// loosing one starts a count, the viewer's next ability or attack ends it, and each of the
/// viewer's hits within a second of the release counts one. Damage events carry no provenance,
/// so like [`damage_is_periodic`] this reads the viewer's own events around the hits.
#[must_use]
pub fn pierce_shot(snapshot: &ZoneSnapshot, shot: Option<PierceShot>) -> Option<PierceShot> {
    let viewer = EntityRef::Player(snapshot.viewer_id);
    let window = u64::from(mmorpg_core::ticks_from_ms(1_000));
    snapshot
        .events
        .iter()
        .fold(shot, |shot, event| match *event {
            ZoneEvent::AbilityUsed {
                source, ability, ..
            } if source == viewer => (ability == mmorpg_core::ability::ids::PIERCING_SHOT)
                .then_some(PierceShot {
                    loosed: snapshot.tick,
                    struck: 0,
                }),
            ZoneEvent::AttackPerformed { source, .. } if source == viewer => None,
            ZoneEvent::DamageDealt { source, .. } if source == viewer => shot.map(|shot| {
                if snapshot.tick.saturating_sub(shot.loosed) <= window {
                    PierceShot {
                        struck: shot.struck + 1,
                        ..shot
                    }
                } else {
                    shot
                }
            }),
            _ => shot,
        })
}

/// What a sword mastery level gained in play says (#915).
#[must_use]
pub fn mastery_unlock_text(level: u8) -> String {
    if level == mmorpg_core::LEAPING_SLASH_LEVEL {
        format!("Sword mastery {level}: Leaping Slash unlocked")
    } else {
        format!("Sword mastery {level}")
    }
}

/// The floating texts one projection's events produce, in event order.
#[must_use]
pub fn combat_texts(snapshot: &ZoneSnapshot) -> Vec<CombatText> {
    let viewer = EntityRef::Player(snapshot.viewer_id);
    let periodic = damage_is_periodic(snapshot);
    let text = |kind, text: String, anchor| Some(CombatText { kind, text, anchor });
    snapshot
        .events
        .iter()
        .filter_map(|event| match *event {
            ZoneEvent::DamageDealt {
                target,
                amount,
                critical,
                ..
            } => {
                let (kind, shown) = if critical {
                    (CombatTextKind::Critical, format!("{amount}!"))
                } else if periodic {
                    (CombatTextKind::Periodic, amount.to_string())
                } else {
                    (CombatTextKind::Damage, amount.to_string())
                };
                text(kind, shown, target)
            }
            ZoneEvent::DamageTaken {
                target,
                amount,
                critical,
                ..
            } => text(
                CombatTextKind::Taken,
                format!("-{amount}{}", if critical { "!" } else { "" }),
                target,
            ),
            ZoneEvent::Miss { source, target } if source == viewer => {
                text(CombatTextKind::Miss, "Miss".into(), target)
            }
            ZoneEvent::TrapHit { target, amount, .. } => {
                text(CombatTextKind::Taken, format!("-{amount}"), target)
            }
            ZoneEvent::Evade { target, .. } => text(CombatTextKind::Miss, "Evade".into(), target),
            // The viewer's own heals, heals on it, and heals on its target (a creature
            // bandaging itself).
            ZoneEvent::Healed {
                source,
                target,
                amount,
                ..
            } if target == viewer || source == viewer || snapshot.viewer.target == Some(target) => {
                text(CombatTextKind::Heal, format!("+{amount}"), target)
            }
            ZoneEvent::Absorbed { target, .. } => {
                text(CombatTextKind::Absorb, "Absorb".into(), target)
            }
            // The viewer's own shield (#156); others' blocks read in the log.
            ZoneEvent::Blocked { target, lane, .. } if target == viewer => text(
                CombatTextKind::Block,
                format!("Block {}", lane_name(lane)),
                target,
            ),
            ZoneEvent::GuardBroken { target, .. } if target == viewer => {
                text(CombatTextKind::GuardBreak, "Guard broken!".into(), target)
            }
            // A hit shows its damage; a whiff of the viewer's own sweep says so.
            ZoneEvent::StrikeSwept {
                source,
                target: None,
                ..
            } if source == viewer => text(CombatTextKind::Miss, "Whiff".into(), source),
            // A signaler raising its horn (#952): interrupt it or kill it before help comes.
            ZoneEvent::CastStarted {
                source,
                ability: mmorpg_core::ability::ids::SIGNAL_HORN,
                ..
            } => text(
                CombatTextKind::Interrupt,
                "Calling for help!".into(),
                source,
            ),
            // The viewer's Sinister Strike from its victim's flank (#949).
            ZoneEvent::Flanked { target, .. } => {
                text(CombatTextKind::Flank, "Flank!".into(), target)
            }
            ZoneEvent::Interrupted { target, .. } => text(
                CombatTextKind::Interrupt,
                if target == viewer {
                    "Interrupted"
                } else {
                    "Interrupt"
                }
                .into(),
                target,
            ),
            _ => None,
        })
        .collect()
}

/// The floating texts a stream of projections produces, each projection taken once: its
/// events' texts, then a cleave or pierce through several units and a mastery level gained.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CombatTextFeed {
    tick: Option<u64>,
    /// Units the viewer's running swing has struck so far (#909).
    cleaved: usize,
    /// The viewer's latest Piercing Shot and the units it struck (#932).
    pierce: Option<PierceShot>,
    /// The sword mastery level last seen (#915).
    mastery: Option<u8>,
}

impl CombatTextFeed {
    /// Forgets the stream, as after a zone or viewer change.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The texts a newer projection adds; none for one already taken.
    pub fn observe(&mut self, snapshot: &ZoneSnapshot) -> Vec<CombatText> {
        if self.tick.is_some_and(|tick| snapshot.tick <= tick) {
            return Vec::new();
        }
        self.tick = Some(snapshot.tick);
        let viewer = EntityRef::Player(snapshot.viewer_id);
        let mut texts = combat_texts(snapshot);
        let cleaved = cleave_count(snapshot, self.cleaved);
        if cleaved > self.cleaved && cleaved > 1 {
            texts.push(CombatText {
                kind: CombatTextKind::Cleave,
                text: format!("Cleave \u{d7}{cleaved}"),
                anchor: viewer,
            });
        }
        self.cleaved = cleaved;
        // A Piercing Shot through several units floats the cleave's "×N" (#932).
        let pierce = pierce_shot(snapshot, self.pierce);
        let before = self
            .pierce
            .filter(|before| pierce.is_some_and(|shot| shot.loosed == before.loosed))
            .map_or(0, |before| before.struck);
        if let Some(shot) = pierce.filter(|shot| shot.struck > 1 && shot.struck > before) {
            texts.push(CombatText {
                kind: CombatTextKind::Cleave,
                text: format!("Pierce \u{d7}{}", shot.struck),
                anchor: viewer,
            });
        }
        self.pierce = pierce;
        // A mastery level gained in play floats its unlock (#915).
        let level = snapshot.viewer.mastery.level;
        if self.mastery.is_some_and(|before| level > before) {
            texts.push(CombatText {
                kind: CombatTextKind::Mastery,
                text: mastery_unlock_text(level),
                anchor: viewer,
            });
        }
        self.mastery = Some(level);
        texts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureId, ZoneId, ZoneSimulation, greyhaven_vale};

    fn snapshot() -> ZoneSnapshot {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
        zone.add_player(1).unwrap();
        zone.advance_tick().unwrap();
        zone.snapshot_for_player(1).unwrap()
    }

    const YOU: EntityRef = EntityRef::Player(1);
    const WOLF: EntityRef = EntityRef::Creature(CreatureId::new(1));

    fn shown(texts: &[CombatText]) -> Vec<(CombatTextKind, &str, EntityRef)> {
        texts
            .iter()
            .map(|text| (text.kind, text.text.as_str(), text.anchor))
            .collect()
    }

    #[test]
    fn events_become_texts_over_the_unit_they_concern() {
        let mut snapshot = snapshot();
        snapshot.events = vec![
            ZoneEvent::DamageDealt {
                source: YOU,
                target: WOLF,
                amount: 12,
                critical: true,
            },
            ZoneEvent::DamageTaken {
                source: WOLF,
                target: YOU,
                amount: 4,
                critical: false,
            },
            ZoneEvent::Miss {
                source: YOU,
                target: WOLF,
            },
            ZoneEvent::Miss {
                source: WOLF,
                target: YOU,
            },
        ];
        assert_eq!(
            shown(&combat_texts(&snapshot)),
            [
                (CombatTextKind::Critical, "12!", WOLF),
                (CombatTextKind::Taken, "-4", YOU),
                (CombatTextKind::Miss, "Miss", WOLF),
            ]
        );
    }

    #[test]
    fn only_the_viewers_guard_floats_its_block_and_break() {
        let mut snapshot = snapshot();
        let other = EntityRef::Player(2);
        snapshot.events = vec![
            ZoneEvent::Blocked {
                source: WOLF,
                target: YOU,
                lane: StrikeHeight::Middle,
                amount: 3,
            },
            ZoneEvent::GuardBroken {
                source: WOLF,
                target: YOU,
                lane: StrikeHeight::Middle,
                ticks: 45,
            },
            ZoneEvent::Blocked {
                source: WOLF,
                target: other,
                lane: StrikeHeight::High,
                amount: 3,
            },
        ];
        assert_eq!(
            shown(&combat_texts(&snapshot)),
            [
                (CombatTextKind::Block, "Block middle", YOU),
                (CombatTextKind::GuardBreak, "Guard broken!", YOU),
            ]
        );
    }

    #[test]
    fn a_feed_takes_each_projection_once_and_floats_a_piercing_shot_through_two() {
        let mut snapshot = snapshot();
        let hit = |id| ZoneEvent::DamageDealt {
            source: YOU,
            target: EntityRef::Creature(CreatureId::new(id)),
            amount: 10,
            critical: false,
        };
        let mut feed = CombatTextFeed::default();
        let mut pierces = Vec::new();
        snapshot.events = vec![ZoneEvent::AbilityUsed {
            source: YOU,
            target: None,
            ability: mmorpg_core::ability::ids::PIERCING_SHOT,
        }];
        assert!(feed.observe(&snapshot).is_empty());
        assert!(feed.observe(&snapshot).is_empty(), "a tick is taken once");
        // The arrow passes the first wolf, then the second a few ticks later.
        for events in [vec![hit(1)], vec![], vec![hit(2)]] {
            snapshot.tick += 3;
            snapshot.events = events;
            pierces.extend(
                feed.observe(&snapshot)
                    .into_iter()
                    .filter(|text| text.text.starts_with("Pierce")),
            );
        }
        assert_eq!(
            shown(&pierces),
            [(CombatTextKind::Cleave, "Pierce \u{d7}2", YOU)]
        );
        // A later ordinary attack ends the count: its hits are not the arrow's.
        snapshot.tick += 3;
        snapshot.events = vec![
            ZoneEvent::AttackPerformed {
                source: YOU,
                target: None,
            },
            hit(3),
        ];
        assert!(
            feed.observe(&snapshot)
                .iter()
                .all(|text| !text.text.starts_with("Pierce"))
        );
    }

    #[test]
    fn a_mastery_level_gained_in_play_floats_its_unlock_once() {
        let mut snapshot = snapshot();
        let mut feed = CombatTextFeed::default();
        snapshot.viewer.mastery.level = mmorpg_core::LEAPING_SLASH_LEVEL - 1;
        assert!(
            feed.observe(&snapshot).is_empty(),
            "the first level is no gain"
        );
        snapshot.tick += 1;
        snapshot.viewer.mastery.level = mmorpg_core::LEAPING_SLASH_LEVEL;
        let texts = feed.observe(&snapshot);
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].kind, CombatTextKind::Mastery);
        assert_eq!(
            texts[0].text,
            mastery_unlock_text(mmorpg_core::LEAPING_SLASH_LEVEL)
        );
        assert!(texts[0].text.ends_with("Leaping Slash unlocked"));
    }
}
