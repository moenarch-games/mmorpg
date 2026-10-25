//! World-space interface: nameplates and NPC quest `!`/`?` markers above the
//! presented units, and floating combat text from the projection's events
//! (the browser's `combat-text.ts` and quest markers). Purely cosmetic: events
//! are lossy, and everything shown already appears in the frames and logs.
use egui::{Align2, Color32, FontId, Pos2};
use mmorpg_core::{
    AuraKind, EntityKind, EntityRef, QuestMarker, QuestSheet, ZoneEvent, ZoneSnapshot,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// Where a unit's labels hang, above its presented head, in world metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitLabel {
    pub entity: EntityRef,
    pub anchor: [f32; 3],
}

/// A world point's place on screen in egui points, or `None` behind the eye.
pub type Project<'a> = &'a dyn Fn([f32; 3]) -> Option<Pos2>;

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
}

/// One floating text and the unit it rises from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombatText {
    pub kind: CombatTextKind,
    pub text: String,
    pub anchor: EntityRef,
}

/// How long a floating text lives, as the browser's `COMBAT_TEXT_MS`.
pub const COMBAT_TEXT_LIFETIME: Duration = Duration::from_millis(1_400);
const MAX_FLOATING_TEXTS: usize = 24;

/// Whether the viewer's damage this tick can only be a periodic tick (the
/// browser's `damageIsPeriodic`): nothing else of the viewer's resolved.
fn damage_is_periodic(snapshot: &ZoneSnapshot) -> bool {
    let viewer = EntityRef::Player(snapshot.viewer_id);
    let resolved = snapshot.events.iter().any(|event| {
        matches!(event,
            ZoneEvent::AbilityUsed { source, .. } | ZoneEvent::StrikeSwept { source, .. }
                if *source == viewer)
    });
    if resolved || snapshot.viewer.auto_attacking {
        return false;
    }
    snapshot.viewer.cast.is_some_and(|cast| cast.channel)
        || snapshot
            .target_detail
            .auras
            .iter()
            .any(|aura| aura.kind == AuraKind::DamageOverTime)
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
            ZoneEvent::Evade { target, .. } => text(CombatTextKind::Miss, "Evade".into(), target),
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
            ZoneEvent::StrikeSwept {
                source,
                target: None,
                ..
            } if source == viewer => text(CombatTextKind::Miss, "Whiff".into(), source),
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

const fn text_style(kind: CombatTextKind) -> (Color32, f32) {
    match kind {
        CombatTextKind::Damage => (Color32::from_rgb(255, 255, 255), 20.0),
        CombatTextKind::Critical => (Color32::from_rgb(255, 214, 64), 28.0),
        CombatTextKind::Periodic => (Color32::from_rgb(255, 236, 160), 17.0),
        CombatTextKind::Taken => (Color32::from_rgb(255, 86, 72), 20.0),
        CombatTextKind::Heal => (Color32::from_rgb(110, 236, 110), 20.0),
        CombatTextKind::Absorb => (Color32::from_rgb(150, 205, 255), 17.0),
        CombatTextKind::Interrupt => (Color32::from_rgb(255, 160, 64), 19.0),
        CombatTextKind::Miss => (Color32::from_rgb(200, 200, 200), 17.0),
    }
}

/// Floating combat texts alive now.
#[derive(Default)]
pub struct FloatingTexts {
    tick: Option<u64>,
    entries: VecDeque<(CombatText, Instant, usize)>,
    lane: usize,
}

impl FloatingTexts {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Takes each newer projection's events once.
    pub fn observe(&mut self, snapshot: &ZoneSnapshot, now: Instant) {
        if self.tick.is_some_and(|tick| snapshot.tick <= tick) {
            return;
        }
        self.tick = Some(snapshot.tick);
        for text in combat_texts(snapshot) {
            self.lane = self.lane.wrapping_add(1);
            self.entries.push_back((text, now, self.lane % 5));
        }
        while self.entries.len() > MAX_FLOATING_TEXTS {
            self.entries.pop_front();
        }
    }

    /// Texts still alive at `now`.
    pub fn alive(&mut self, now: Instant) -> impl Iterator<Item = &(CombatText, Instant, usize)> {
        self.entries
            .retain(|(_, born, _)| now.saturating_duration_since(*born) < COMBAT_TEXT_LIFETIME);
        self.entries.iter()
    }
}

/// Nameplate colour by disposition, as the target frame.
fn plate_color(kind: EntityKind, hostile: bool) -> Color32 {
    if hostile {
        Color32::from_rgb(255, 110, 100)
    } else if kind == EntityKind::Npc {
        Color32::from_rgb(120, 220, 120)
    } else if kind == EntityKind::Player {
        Color32::from_rgb(140, 190, 255)
    } else {
        Color32::from_rgb(255, 230, 120)
    }
}

/// Labels this far from the eye are not drawn.
const LABEL_RANGE_METRES: f32 = 60.0;

/// Paints nameplates, quest markers and floating combat text behind every
/// interface window.
pub fn paint(
    ctx: &egui::Context,
    snapshot: &ZoneSnapshot,
    labels: &[UnitLabel],
    project: Project<'_>,
    eye: [f32; 3],
    names: &dyn Fn(EntityRef) -> String,
    quests: Option<&QuestSheet>,
    texts: &mut FloatingTexts,
    now: Instant,
) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::unique("world-labels"),
    ));
    let viewer = EntityRef::Player(snapshot.viewer_id);
    let anchor_of = |entity| labels.iter().find(|l| l.entity == entity);
    for label in labels {
        if label.entity == viewer {
            continue;
        }
        let Some(record) = snapshot
            .entities
            .iter()
            .find(|e| e.entity() == label.entity)
        else {
            continue;
        };
        if record.flags.dead {
            continue;
        }
        let distance = label
            .anchor
            .iter()
            .zip(eye)
            .map(|(a, e)| (a - e) * (a - e))
            .sum::<f32>()
            .sqrt();
        if distance > LABEL_RANGE_METRES {
            continue;
        }
        let Some(at) = project(label.anchor) else {
            continue;
        };
        let fade = (1.0 - (distance - LABEL_RANGE_METRES * 0.7) / (LABEL_RANGE_METRES * 0.3))
            .clamp(0.0, 1.0);
        let alpha = |c: Color32| c.gamma_multiply(fade);
        let targeted = snapshot.viewer.target == Some(label.entity);
        let name = names(label.entity);
        let color = plate_color(record.kind, record.flags.hostile);
        let size = if targeted { 15.0 } else { 13.0 };
        // A dark offset copy keeps the name legible over bright sky and grass.
        painter.text(
            at + egui::vec2(1.0, 1.0),
            Align2::CENTER_BOTTOM,
            &name,
            FontId::proportional(size),
            alpha(Color32::from_black_alpha(200)),
        );
        let rect = painter.text(
            at,
            Align2::CENTER_BOTTOM,
            &name,
            FontId::proportional(size),
            alpha(color),
        );
        let marker = match label.entity {
            EntityRef::Npc(npc) => quests
                .and_then(|sheet| sheet.markers.iter().find(|m| m.npc == npc))
                .map(|m| m.marker),
            _ => None,
        };
        if let Some(marker) = marker {
            let (glyph, color) = match marker {
                QuestMarker::Available => ("!", Color32::from_rgb(255, 210, 0)),
                QuestMarker::Complete => ("?", Color32::from_rgb(255, 210, 0)),
                QuestMarker::InProgress => ("?", Color32::from_gray(170)),
            };
            let top = Pos2::new(at.x, rect.top() - 2.0);
            painter.text(
                top + egui::vec2(2.0, 2.0),
                Align2::CENTER_BOTTOM,
                glyph,
                FontId::proportional(34.0),
                alpha(Color32::from_black_alpha(220)),
            );
            painter.text(
                top,
                Align2::CENTER_BOTTOM,
                glyph,
                FontId::proportional(34.0),
                alpha(color),
            );
        }
    }
    for (text, born, lane) in texts.alive(now) {
        let Some(at) = anchor_of(text.anchor).and_then(|l| project(l.anchor)) else {
            continue;
        };
        let age =
            now.saturating_duration_since(*born).as_secs_f32() / COMBAT_TEXT_LIFETIME.as_secs_f32();
        let (color, size) = text_style(text.kind);
        let drift = (*lane as f32 - 2.0) * 16.0;
        let at = at + egui::vec2(drift, -22.0 - age * 48.0);
        let fade = (1.0 - (age - 0.6).max(0.0) / 0.4).clamp(0.0, 1.0);
        painter.text(
            at + egui::vec2(1.5, 1.5),
            Align2::CENTER_BOTTOM,
            &text.text,
            FontId::proportional(size),
            Color32::from_black_alpha(220).gamma_multiply(fade),
        );
        painter.text(
            at,
            Align2::CENTER_BOTTOM,
            &text.text,
            FontId::proportional(size),
            color.gamma_multiply(fade),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureId, ZoneId, ZoneSimulation, greyhaven_vale};

    #[test]
    fn events_become_floating_text_over_the_unit_they_concern() {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
        zone.add_player(1).unwrap();
        zone.advance_tick().unwrap();
        let mut snapshot = zone.snapshot_for_player(1).unwrap();
        let wolf = EntityRef::Creature(CreatureId::new(1));
        let you = EntityRef::Player(1);
        snapshot.events = vec![
            ZoneEvent::DamageDealt {
                source: you,
                target: wolf,
                amount: 12,
                critical: true,
            },
            ZoneEvent::DamageTaken {
                source: wolf,
                target: you,
                amount: 4,
                critical: false,
            },
            ZoneEvent::Miss {
                source: you,
                target: wolf,
            },
            ZoneEvent::Miss {
                source: wolf,
                target: you,
            },
        ];
        let texts = combat_texts(&snapshot);
        let shown: Vec<_> = texts
            .iter()
            .map(|t| (t.kind, t.text.as_str(), t.anchor))
            .collect();
        assert_eq!(
            shown,
            [
                (CombatTextKind::Critical, "12!", wolf),
                (CombatTextKind::Taken, "-4", you),
                (CombatTextKind::Miss, "Miss", wolf),
            ]
        );
        let mut floating = FloatingTexts::default();
        let now = Instant::now();
        floating.observe(&snapshot, now);
        floating.observe(&snapshot, now);
        assert_eq!(floating.alive(now).count(), 3, "a tick is taken once");
        assert_eq!(floating.alive(now + COMBAT_TEXT_LIFETIME).count(), 0);
    }
}
