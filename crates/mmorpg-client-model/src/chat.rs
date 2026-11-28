//! Chat input parsing and the received chat and combat logs. The zone decides
//! who hears a line; the logs only read what it delivered.
use mmorpg_core::{
    ChatChannel, ChatMessage, ChatText, Emote, EntityKind, ErrorCode, PostureView, ZoneCommand,
    ZoneContent, ZoneEvent, ZoneId, ZoneSnapshot,
};
use std::collections::{BTreeMap, VecDeque};
use strum::VariantArray;

/// Each log keeps the newest lines only.
pub const MAX_LOG_LINES: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineKind {
    Say,
    Yell,
    Emote,
    System,
    Combat,
    Quest,
    Refusal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogLine {
    pub kind: LineKind,
    pub text: String,
}

/// The slash command and the verb of an emote: `wave`, `bow`, …
#[must_use]
pub const fn emote_name(emote: Emote) -> &'static str {
    match emote {
        Emote::Wave => "wave",
        Emote::Bow => "bow",
        Emote::Cheer => "cheer",
        Emote::Laugh => "laugh",
        Emote::Point => "point",
    }
}

/// Reads a typed line: `/y` or `/yell` yells, `/s`, `/say` or no prefix says,
/// and `/wave`, `/bow`, `/cheer`, `/laugh` or `/point` emotes. Returns the
/// command or why the line cannot be sent.
pub fn parse_chat_input(raw: &str) -> Result<ZoneCommand, String> {
    let trimmed = raw.trim();
    if let Some(word) = trimmed.strip_prefix('/') {
        let (verb, rest) = word
            .split_once(char::is_whitespace)
            .map_or((word, ""), |(verb, rest)| (verb, rest.trim_start()));
        let verb = verb.to_ascii_lowercase();
        if rest.is_empty()
            && let Some(&emote) = Emote::VARIANTS.iter().find(|e| emote_name(**e) == verb)
        {
            return Ok(ZoneCommand::Emote(emote));
        }
        // Sits on the ground where the player stands (#619).
        if rest.is_empty() && verb == "sit" {
            return Ok(ZoneCommand::Sit { seat: None });
        }
        let channel = match verb.as_str() {
            "s" | "say" => ChatChannel::Say,
            "y" | "yell" => ChatChannel::Yell,
            _ => {
                let emotes: Vec<_> = Emote::VARIANTS.iter().map(|e| emote_name(*e)).collect();
                return Err(format!(
                    "Use /s to say, /y to yell, /sit to sit, or /{}.",
                    emotes.join(", /")
                ));
            }
        };
        return text(rest).map(|text| ZoneCommand::Chat { channel, text });
    }
    text(raw).map(|text| ZoneCommand::Chat {
        channel: ChatChannel::Say,
        text,
    })
}

/// The core chat text rule as a sentence: "Chat text must not be blank."
fn text(raw: &str) -> Result<ChatText, String> {
    ChatText::new(raw).map_err(|error| {
        let message = error.message();
        let mut chars = message.chars();
        chars.next().map_or_else(String::new, |first| {
            format!("{}{}.", first.to_uppercase(), chars.as_str())
        })
    })
}

/// One log: the newest [`MAX_LOG_LINES`] lines and how many were ever added,
/// so a renderer can tell whether anything changed.
#[derive(Clone, Debug, Default)]
pub struct Log {
    lines: VecDeque<LogLine>,
    added: u64,
}

impl Log {
    pub fn lines(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }

    /// Lines added since the log began, including those already dropped.
    #[must_use]
    pub const fn added(&self) -> u64 {
        self.added
    }

    fn push(&mut self, kind: LineKind, text: String) {
        self.lines.push_back(LogLine { kind, text });
        self.added += 1;
        while self.lines.len() > MAX_LOG_LINES {
            self.lines.pop_front();
        }
    }
}

/// Received chat and the viewer's combat and quest feedback, each projection
/// taken once. A projection from another stream (zone, content revision or
/// viewer) starts its tick count afresh.
#[derive(Clone, Debug, Default)]
pub struct Logs {
    pub chat: Log,
    pub combat: Log,
    stream: Option<(ZoneId, u64, u32)>,
    tick: Option<u64>,
    /// The merchants on a cart journey (#992) in the last projection, by NPC ID.
    merchants: BTreeMap<u32, MerchantState>,
}

impl Logs {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// A local chat note, such as why a typed line was not sent.
    pub fn note(&mut self, kind: LineKind, text: impl Into<String>) {
        self.chat.push(kind, text.into());
    }

    /// Whether `snapshot` is newer than every projection taken so far.
    fn take(&mut self, snapshot: &ZoneSnapshot) -> bool {
        let stream = (
            snapshot.zone_id,
            snapshot.content_revision,
            snapshot.viewer_id,
        );
        if self.stream != Some(stream) {
            self.stream = Some(stream);
            self.tick = None;
        }
        if self.tick.is_some_and(|tick| snapshot.tick <= tick) {
            return false;
        }
        self.tick = Some(snapshot.tick);
        true
    }

    /// Reads a new projection's chat lines into the chat log only; returns
    /// whether lines were added.
    pub fn receive_chat(&mut self, snapshot: &ZoneSnapshot) -> bool {
        let before = self.chat.added;
        if self.take(snapshot) {
            self.chat_lines(snapshot);
        }
        self.chat.added != before
    }

    /// Reads a new projection's chat lines and its events' lines
    /// ([`crate::texts::event_line`]). Returns the refusal messages it found.
    pub fn receive(&mut self, snapshot: &ZoneSnapshot, content: &ZoneContent) -> Vec<String> {
        if !self.take(snapshot) {
            return Vec::new();
        }
        self.chat_lines(snapshot);
        let mut refusals = Vec::new();
        for event in &snapshot.events {
            let Some((kind, text)) = crate::texts::event_line(event, snapshot, content) else {
                continue;
            };
            if kind == LineKind::Refusal {
                refusals.push(text.clone());
            }
            self.combat.push(kind, text);
        }
        refusals
    }

    /// What the zone delivered as readable lines, and the throttle notice.
    fn chat_lines(&mut self, snapshot: &ZoneSnapshot) {
        let viewer = snapshot.viewer_id;
        for line in &snapshot.chat {
            let you = line.speaker == viewer;
            let speaker = if you {
                "You".to_owned()
            } else {
                format!("Player {}", line.speaker)
            };
            let verb = |own: &str, other: &str| if you { own } else { other }.to_owned();
            let (kind, text) = match line.message {
                ChatMessage::Emote(emote) => (
                    LineKind::Emote,
                    format!("{speaker} {}{}.", emote_name(emote), verb("", "s")),
                ),
                ChatMessage::Say(text) => (
                    LineKind::Say,
                    format!("{speaker} {}: {}", verb("say", "says"), text.as_str()),
                ),
                ChatMessage::Yell(text) => (
                    LineKind::Yell,
                    format!("{speaker} {}: {}", verb("yell", "yells"), text.as_str()),
                ),
            };
            self.chat.push(kind, text);
        }
        for event in &snapshot.events {
            if let ZoneEvent::Error {
                code: ErrorCode::ChatThrottled,
                ..
            } = event
            {
                self.chat.push(
                    LineKind::System,
                    "You can speak or emote once per second.".into(),
                );
            }
        }
        self.merchant_cries(snapshot);
    }

    /// A merchant on its cart journey (#992) cries out as the viewer sees it held up, robbed or
    /// saved: each line follows the projected posture and health alone, once per change.
    fn merchant_cries(&mut self, snapshot: &ZoneSnapshot) {
        let mut seen = BTreeMap::new();
        for entity in &snapshot.entities {
            if entity.kind != EntityKind::Npc {
                continue;
            }
            let state = match entity.flags.posture {
                PostureView::Hauling if entity.health_percent < 100 => MerchantState::Held,
                PostureView::Hauling => MerchantState::Travelling,
                PostureView::Hurt => MerchantState::Hurt,
                _ => continue,
            };
            let before = self.merchants.get(&entity.id).copied();
            let line = match (before, state) {
                (Some(MerchantState::Held), MerchantState::Held) => None,
                (_, MerchantState::Held) => Some((
                    LineKind::Yell,
                    "A merchant yells: Redbrands! Help, someone, they're after my cart!",
                )),
                (Some(MerchantState::Held), MerchantState::Hurt) => Some((
                    LineKind::Yell,
                    "A merchant cries: They've taken the cart, everything!",
                )),
                (Some(MerchantState::Held), MerchantState::Travelling) => Some((
                    LineKind::Say,
                    "A merchant says: Bless you, friend! On to Millbrook, then.",
                )),
                _ => None,
            };
            if let Some((kind, text)) = line {
                self.chat.push(kind, text.to_owned());
            }
            seen.insert(entity.id, state);
        }
        self.merchants = seen;
    }
}

/// What a viewer last saw of a merchant on its cart journey (#992).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MerchantState {
    Travelling,
    Held,
    Hurt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::ChatLine;

    fn say(text: &str) -> ZoneCommand {
        ZoneCommand::Chat {
            channel: ChatChannel::Say,
            text: ChatText::new(text).unwrap(),
        }
    }

    #[test]
    fn typed_lines_become_the_shared_chat_commands() {
        assert_eq!(parse_chat_input("hello"), Ok(say("hello")));
        assert_eq!(parse_chat_input("/s hello"), Ok(say("hello")));
        assert_eq!(parse_chat_input("/say  hello"), Ok(say("hello")));
        assert_eq!(
            parse_chat_input("/YELL over here"),
            Ok(ZoneCommand::Chat {
                channel: ChatChannel::Yell,
                text: ChatText::new("over here").unwrap()
            })
        );
        assert_eq!(
            parse_chat_input("/wave"),
            Ok(ZoneCommand::Emote(Emote::Wave))
        );
        assert_eq!(
            parse_chat_input("/Bow "),
            Ok(ZoneCommand::Emote(Emote::Bow))
        );
        assert_eq!(
            parse_chat_input("/dance"),
            Err(
                "Use /s to say, /y to yell, /sit to sit, or /wave, /bow, /cheer, /laugh, /point."
                    .into()
            )
        );
        assert_eq!(
            parse_chat_input("/s"),
            Err("Chat text must be 1 to 80 bytes.".into())
        );
        assert_eq!(
            parse_chat_input(&"a".repeat(81)),
            Err("Chat text must be 1 to 80 bytes.".into())
        );
        assert_eq!(
            parse_chat_input("tab\there"),
            Err("Chat text must not contain control characters.".into())
        );
        // U+FEFF is not whitespace, so the line is valid.
        assert_eq!(parse_chat_input("\u{feff}"), Ok(say("\u{feff}")));
    }

    #[test]
    fn received_lines_read_with_speaker_names_and_each_projection_counts_once() {
        let line = |speaker, message| ChatLine { speaker, message };
        let text = |text| ChatText::new(text).unwrap();
        let mut snapshot = crate::test_support::snapshot(7, 5);
        snapshot.chat = vec![
            line(7, ChatMessage::Say(text("hello"))),
            line(9, ChatMessage::Yell(text("over here"))),
            line(9, ChatMessage::Emote(Emote::Wave)),
            line(7, ChatMessage::Emote(Emote::Bow)),
        ];
        snapshot.events = vec![ZoneEvent::Error {
            code: ErrorCode::ChatThrottled,
            target: None,
        }];
        let mut logs = Logs::default();
        assert!(logs.receive_chat(&snapshot));
        let lines: Vec<_> = logs.chat.lines().map(|line| line.text.as_str()).collect();
        assert_eq!(
            lines,
            [
                "You say: hello",
                "Player 9 yells: over here",
                "Player 9 waves.",
                "You bow.",
                "You can speak or emote once per second.",
            ]
        );
        assert!(!logs.receive_chat(&snapshot), "a projection counts once");
        // Another zone counts its own ticks.
        snapshot.zone_id = ZoneId::new(2);
        snapshot.tick = 1;
        snapshot.events.clear();
        assert!(logs.receive_chat(&snapshot));
        assert_eq!(logs.chat.added(), 9);
    }

    #[test]
    fn events_fill_the_combat_log_and_refusals_come_back() {
        let content = mmorpg_core::greyhaven_vale::content();
        let mut snapshot = crate::test_support::snapshot(7, 5);
        snapshot.events = vec![
            ZoneEvent::Died {
                entity: mmorpg_core::EntityRef::Player(7),
                killer: None,
            },
            ZoneEvent::Error {
                code: ErrorCode::NotReady,
                target: None,
            },
        ];
        let mut logs = Logs::default();
        assert_eq!(
            logs.receive(&snapshot, &content),
            ["That ability is not ready yet."]
        );
        let lines: Vec<_> = logs.combat.lines().map(|line| line.text.as_str()).collect();
        assert_eq!(lines, ["You died.", "That ability is not ready yet."]);
        assert!(logs.receive(&snapshot, &content).is_empty(), "once");
    }

    #[test]
    fn a_log_keeps_the_newest_lines() {
        let mut logs = Logs::default();
        for index in 0..MAX_LOG_LINES + 5 {
            logs.note(LineKind::System, format!("{index}"));
        }
        assert_eq!(logs.chat.lines().count(), MAX_LOG_LINES);
        assert_eq!(logs.chat.lines().next().unwrap().text, "5");
        assert_eq!(logs.chat.added(), (MAX_LOG_LINES + 5) as u64);
    }
}

#[cfg(test)]
mod merchant_tests {
    use super::*;
    use mmorpg_core::{EntityFlags, EntitySnapshot};

    fn merchant(posture: PostureView, health_percent: u8) -> EntitySnapshot {
        EntitySnapshot {
            kind: EntityKind::Npc,
            id: 17,
            appearance: 17,
            position: [0; 3],
            velocity: [0; 3],
            facing: 0,
            level: 3,
            health_percent,
            flags: EntityFlags {
                posture,
                ..EntityFlags::default()
            },
        }
    }

    #[test]
    fn a_held_up_merchant_cries_once_and_then_by_its_outcome() {
        let mut logs = Logs::default();
        let mut tick = 0;
        let mut see = |logs: &mut Logs, entities: Vec<EntitySnapshot>| {
            tick += 1;
            let mut snapshot = crate::test_support::snapshot(1, tick);
            snapshot.entities = entities;
            logs.receive_chat(&snapshot);
            logs.chat.lines().map(|line| line.kind).collect::<Vec<_>>()
        };
        assert!(see(&mut logs, vec![merchant(PostureView::Hauling, 100)]).is_empty());
        let held = vec![merchant(PostureView::Hauling, 60)];
        assert_eq!(see(&mut logs, held.clone()), [LineKind::Yell]);
        assert_eq!(see(&mut logs, held.clone()), [LineKind::Yell]);
        let saved = see(&mut logs, vec![merchant(PostureView::Hauling, 100)]);
        assert_eq!(saved, [LineKind::Yell, LineKind::Say]);
        see(&mut logs, held);
        let robbed = see(&mut logs, vec![merchant(PostureView::Hurt, 35)]);
        assert_eq!(robbed.len(), 4);
        // A merchant already hurt when it comes into view says nothing.
        let mut fresh = Logs::default();
        assert!(see(&mut fresh, vec![merchant(PostureView::Hurt, 35)]).is_empty());
    }
}
