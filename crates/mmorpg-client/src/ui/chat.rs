//! Chat input parsing and the received chat/combat logs, mirroring the
//! browser's `chat-frame.ts`. The zone decides who hears a line.
use mmorpg_core::{
    ChatChannel, ChatMessage, ChatText, Emote, EntityRef, ZoneCommand, ZoneEvent, ZoneSnapshot,
    ability_by_id,
};
use std::collections::VecDeque;

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

fn emote_name(emote: Emote) -> &'static str {
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
            && let Some(emote) = Emote::ALL.into_iter().find(|e| emote_name(*e) == verb)
        {
            return Ok(ZoneCommand::Emote(emote));
        }
        let channel = match verb.as_str() {
            "s" | "say" => ChatChannel::Say,
            "y" | "yell" => ChatChannel::Yell,
            _ => {
                return Err(
                    "Use /s to say, /y to yell, or /wave, /bow, /cheer, /laugh, /point.".into(),
                );
            }
        };
        return text(rest).map(|text| ZoneCommand::Chat { channel, text });
    }
    text(raw).map(|text| ZoneCommand::Chat {
        channel: ChatChannel::Say,
        text,
    })
}

fn text(raw: &str) -> Result<ChatText, String> {
    ChatText::new(raw).map_err(|error| error.message().to_owned())
}

/// `NotEnoughMoney` reads "Not enough money".
#[must_use]
pub fn humanize(debug: &str) -> String {
    let mut out = String::new();
    for (index, character) in debug.chars().enumerate() {
        if character.is_uppercase() && index > 0 {
            out.push(' ');
            out.extend(character.to_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

/// Received chat and the viewer's combat/quest feedback, each projection taken once.
#[derive(Default)]
pub struct Logs {
    pub chat: VecDeque<LogLine>,
    pub combat: VecDeque<LogLine>,
    tick: Option<u64>,
}

impl Logs {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn note(&mut self, kind: LineKind, text: impl Into<String>) {
        push(&mut self.chat, kind, text.into());
    }

    /// Reads a new projection's chat lines and events; `name` names units.
    /// Returns the refusal messages it found.
    pub fn receive(
        &mut self,
        snapshot: &ZoneSnapshot,
        name: impl Fn(EntityRef) -> String,
        quest_name: impl Fn(mmorpg_core::QuestId) -> String,
    ) -> Vec<String> {
        if self.tick.is_some_and(|tick| snapshot.tick <= tick) {
            return Vec::new();
        }
        self.tick = Some(snapshot.tick);
        let viewer = snapshot.viewer_id;
        for line in &snapshot.chat {
            let you = line.speaker == viewer;
            let speaker = if you {
                "You".to_owned()
            } else {
                format!("Player {}", line.speaker)
            };
            let (kind, text) = match line.message {
                ChatMessage::Emote(emote) => {
                    let verb = emote_name(emote);
                    (
                        LineKind::Emote,
                        format!("{speaker} {verb}{}.", if you { "" } else { "s" }),
                    )
                }
                ChatMessage::Say(text) => (
                    LineKind::Say,
                    format!(
                        "{speaker} {}: {}",
                        if you { "say" } else { "says" },
                        text.as_str()
                    ),
                ),
                ChatMessage::Yell(text) => (
                    LineKind::Yell,
                    format!(
                        "{speaker} {}: {}",
                        if you { "yell" } else { "yells" },
                        text.as_str()
                    ),
                ),
            };
            push(&mut self.chat, kind, text);
        }
        let me = EntityRef::Player(viewer);
        let who = |entity: EntityRef| {
            if entity == me {
                "You".to_owned()
            } else {
                name(entity)
            }
        };
        let ability = |id| ability_by_id(id).map_or("an ability", |a| a.name);
        let mut refusals = Vec::new();
        for event in &snapshot.events {
            let (kind, text) = match *event {
                ZoneEvent::DamageDealt {
                    target,
                    amount,
                    critical,
                    ..
                } => (
                    LineKind::Combat,
                    format!(
                        "You {} {} for {amount}.",
                        if critical { "critically hit" } else { "hit" },
                        who(target)
                    ),
                ),
                ZoneEvent::DamageTaken {
                    source,
                    amount,
                    critical,
                    ..
                } => (
                    LineKind::Combat,
                    format!(
                        "{} {} you for {amount}.",
                        who(source),
                        if critical { "critically hits" } else { "hits" }
                    ),
                ),
                ZoneEvent::Miss { source, target } => (
                    LineKind::Combat,
                    format!("{} missed {}.", who(source), who(target)),
                ),
                ZoneEvent::Died { entity, .. } => (
                    LineKind::Combat,
                    if entity == me {
                        "You died.".into()
                    } else {
                        format!("{} dies.", who(entity))
                    },
                ),
                ZoneEvent::Evade { target, .. } => {
                    (LineKind::Combat, format!("{} evades.", who(target)))
                }
                // A trailing lurker gave up on the diver (#274).
                ZoneEvent::LostInterest { .. } => {
                    (LineKind::Combat, "The lurker loses interest.".into())
                }
                ZoneEvent::Healed {
                    target,
                    ability: id,
                    amount,
                    ..
                } => (
                    LineKind::Combat,
                    format!("{} heals {} for {amount}.", ability(id), who(target)),
                ),
                ZoneEvent::AuraApplied {
                    target,
                    ability: id,
                    ..
                } if target == me => (LineKind::Combat, format!("You gain {}.", ability(id))),
                ZoneEvent::Interrupted {
                    target,
                    ability: id,
                    ..
                } => (
                    LineKind::Combat,
                    format!("{}'s {} is interrupted.", who(target), ability(id)),
                ),
                ZoneEvent::Absorbed { target, amount, .. } => (
                    LineKind::Combat,
                    format!("{amount} absorbed on {}.", who(target)),
                ),
                ZoneEvent::QuestProgress {
                    quest,
                    objective,
                    count,
                } => (
                    LineKind::Quest,
                    format!(
                        "{}: objective {} now {count}.",
                        quest_name(quest),
                        objective + 1
                    ),
                ),
                ZoneEvent::QuestCompleted { quest } => {
                    (LineKind::Quest, format!("{} completed.", quest_name(quest)))
                }
                ZoneEvent::Error { code, .. } => {
                    let text = match code {
                        mmorpg_core::ErrorCode::DoorBlocked => {
                            "Something is in the way of the door.".into()
                        }
                        mmorpg_core::ErrorCode::DoorBusy => "The door is still swinging.".into(),
                        mmorpg_core::ErrorCode::NoMinions => {
                            "You have no minions to command.".into()
                        }
                        mmorpg_core::ErrorCode::VendorClosed => {
                            "The shop is closed for the night.".into()
                        }
                        _ => humanize(&format!("{code:?}")),
                    };
                    refusals.push(text.clone());
                    if code == mmorpg_core::ErrorCode::ChatThrottled {
                        push(
                            &mut self.chat,
                            LineKind::System,
                            "You can speak or emote once per second.".into(),
                        );
                    }
                    (LineKind::Refusal, text)
                }
                _ => continue,
            };
            push(&mut self.combat, kind, text);
        }
        refusals
    }
}

fn push(log: &mut VecDeque<LogLine>, kind: LineKind, text: String) {
    log.push_back(LogLine { kind, text });
    while log.len() > MAX_LOG_LINES {
        log.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lines_become_the_shared_chat_commands() {
        let say = |text: &str| ZoneCommand::Chat {
            channel: ChatChannel::Say,
            text: ChatText::new(text).unwrap(),
        };
        assert_eq!(parse_chat_input("hello"), Ok(say("hello")));
        assert_eq!(parse_chat_input("/s hello"), Ok(say("hello")));
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
        assert!(parse_chat_input("/dance").is_err());
        assert!(parse_chat_input("/s").is_err());
        assert_eq!(humanize("NotEnoughMoney"), "Not enough money");
    }
}
