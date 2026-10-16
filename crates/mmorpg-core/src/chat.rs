//! Zone chat (#68) and emotes (#69): bounded `/say` and `/yell` lines and a
//! small typed emote vocabulary, as zone-local events on one path.
//!
//! An emote is delivered like a `/say` line: same range, the speaker's shared
//! rate limit, the same ordering and per-tick cap. Core only names the emote;
//! clients choose how to present it and gain no animation authority.
//!
//! A line is validated text of 1–[`MAX_CHAT_BYTES`] UTF-8 bytes without
//! control characters. It is a sequenced intent resolved in tick step 1 in
//! `(player id, sequence)` order: a speaker may speak once every
//! [`CHAT_INTERVAL_TICKS`], and every player whose position is within the
//! channel's horizontal range of the speaker, the speaker included, hears it
//! in ascending player ID order. Each player hears at most
//! [`MAX_CHAT_PER_TICK`] lines per tick; later lines of that tick are not
//! delivered to that player. Heard lines are this tick's projection data,
//! like events: cosmetic, lossy with their datagram and never stored.

use std::fmt;

use crate::{ErrorCode, PlayerId, ZoneError, ZoneEvent, ZoneSimulation};

/// The longest chat line in UTF-8 bytes.
pub const MAX_CHAT_BYTES: usize = 80;
/// A speaker may speak once per second.
pub const CHAT_INTERVAL_TICKS: u64 = 30;
/// Lines one player hears in one tick.
pub const MAX_CHAT_PER_TICK: usize = 4;
/// `/say` reaches 20 m (inclusive, horizontal).
pub const SAY_RANGE_UNITS: i32 = 2_000;
/// `/yell` reaches 60 m (inclusive, horizontal).
pub const YELL_RANGE_UNITS: i32 = 6_000;

/// The emote vocabulary. Wire IDs are stable; unknown IDs are malformed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Emote {
    Wave,
    Bow,
    Cheer,
    Laugh,
    Point,
}

impl Emote {
    pub const ALL: [Self; 5] = [Self::Wave, Self::Bow, Self::Cheer, Self::Laugh, Self::Point];

    /// Wire value: 1 wave, 2 bow, 3 cheer, 4 laugh, 5 point.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Wave => 1,
            Self::Bow => 2,
            Self::Cheer => 3,
            Self::Laugh => 4,
            Self::Point => 5,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Wave),
            2 => Some(Self::Bow),
            3 => Some(Self::Cheer),
            4 => Some(Self::Laugh),
            5 => Some(Self::Point),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ChatChannel {
    Say,
    Yell,
}

impl ChatChannel {
    #[must_use]
    pub const fn range_units(self) -> i32 {
        match self {
            Self::Say => SAY_RANGE_UNITS,
            Self::Yell => YELL_RANGE_UNITS,
        }
    }

    /// Wire value: 0 say, 1 yell.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Say => 0,
            Self::Yell => 1,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Say),
            1 => Some(Self::Yell),
            _ => None,
        }
    }

    #[must_use]
    pub const fn message(self, text: ChatText) -> ChatMessage {
        match self {
            Self::Say => ChatMessage::Say(text),
            Self::Yell => ChatMessage::Yell(text),
        }
    }
}

/// What one speaker delivers: a line on a channel, or an emote.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChatMessage {
    Say(ChatText),
    Yell(ChatText),
    Emote(Emote),
}

impl ChatMessage {
    /// Emotes reach as far as `/say`.
    #[must_use]
    pub const fn range_units(self) -> i32 {
        match self {
            Self::Say(_) | Self::Emote(_) => SAY_RANGE_UNITS,
            Self::Yell(_) => YELL_RANGE_UNITS,
        }
    }

    /// Wire value: 0 say, 1 yell, 2 emote.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Say(_) => 0,
            Self::Yell(_) => 1,
            Self::Emote(_) => 2,
        }
    }
}

/// A validated chat line, stored inline so commands stay `Copy`.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ChatText {
    bytes: [u8; MAX_CHAT_BYTES],
    len: u8,
}

impl ChatText {
    /// Accepts 1–80 bytes of UTF-8 with at least one non-whitespace
    /// character and no control characters.
    pub fn new(text: &str) -> Result<Self, ZoneError> {
        if text.is_empty() || text.len() > MAX_CHAT_BYTES {
            return Err(ZoneError::new("chat text must be 1 to 80 bytes"));
        }
        if text.chars().any(char::is_control) {
            return Err(ZoneError::new(
                "chat text must not contain control characters",
            ));
        }
        if text.trim().is_empty() {
            return Err(ZoneError::new("chat text must not be blank"));
        }
        let mut bytes = [0; MAX_CHAT_BYTES];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        Ok(Self {
            bytes,
            len: u8::try_from(text.len()).map_err(|_| ZoneError::new("chat text is too long"))?,
        })
    }

    /// Validates raw wire bytes as a chat line.
    pub fn from_utf8(bytes: &[u8]) -> Result<Self, ZoneError> {
        Self::new(
            std::str::from_utf8(bytes).map_err(|_| ZoneError::new("chat text must be UTF-8"))?,
        )
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        // Construction only accepts valid UTF-8.
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or_default()
    }
}

impl fmt::Debug for ChatText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), formatter)
    }
}

/// One line or emote a player heard this tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChatLine {
    pub speaker: PlayerId,
    pub message: ChatMessage,
}

impl ZoneSimulation {
    /// Tick step 1 of `Chat` and `Emote`: refuses a speaker that spoke or
    /// emoted within the last [`CHAT_INTERVAL_TICKS`], otherwise delivers the
    /// message to every player in range, in ascending player ID order.
    pub(crate) fn speak(
        &mut self,
        speaker: PlayerId,
        message: ChatMessage,
        now: u64,
    ) -> Result<(), ZoneError> {
        let Some(state) = self.players.get(&speaker) else {
            return Ok(());
        };
        if now < state.chat_ready_at {
            self.notify(
                speaker,
                ZoneEvent::Error {
                    code: ErrorCode::ChatThrottled,
                    target: None,
                },
            );
            return Ok(());
        }
        let from = self.player_position(speaker)?;
        let range = i128::from(message.range_units());
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        let mut hearers = Vec::new();
        for id in ids {
            let at = self.player_position(id)?;
            let [dx, dz] = [
                i128::from(at.x) - i128::from(from.x),
                i128::from(at.z) - i128::from(from.z),
            ];
            if dx * dx + dz * dz <= range * range {
                hearers.push(id);
            }
        }
        let line = ChatLine { speaker, message };
        for id in hearers {
            if let Some(hearer) = self.players.get_mut(&id)
                && hearer.chat.len() < MAX_CHAT_PER_TICK
            {
                hearer.chat.push(line);
            }
        }
        if let Some(state) = self.players.get_mut(&speaker) {
            state.chat_ready_at = now + CHAT_INTERVAL_TICKS;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_text_is_bounded_printable_utf8() {
        assert_eq!(
            ChatText::new("Hail, traveller!").unwrap().as_str(),
            "Hail, traveller!"
        );
        assert_eq!(ChatText::new("Grüße").unwrap().as_str(), "Grüße");
        assert!(ChatText::new(&"a".repeat(80)).is_ok());
        for invalid in [
            "",
            "   ",
            "line\nbreak",
            "tab\t",
            &"a".repeat(81),
            "bell\u{7}",
        ] {
            assert!(ChatText::new(invalid).is_err(), "{invalid:?}");
        }
        assert!(ChatText::from_utf8(&[0xff, 0xfe]).is_err());
        // Twenty-seven three-byte characters exceed the byte bound.
        assert!(ChatText::new(&"€".repeat(27)).is_err());
        assert!(ChatText::new(&"€".repeat(26)).is_ok());
    }

    #[test]
    fn emote_codes_round_trip_and_reject_unknown_ids() {
        for emote in Emote::ALL {
            assert_eq!(Emote::from_code(emote.code()), Some(emote));
        }
        for unknown in [0, 6, 255] {
            assert_eq!(Emote::from_code(unknown), None);
        }
    }

    #[test]
    fn channel_codes_round_trip() {
        for channel in [ChatChannel::Say, ChatChannel::Yell] {
            assert_eq!(ChatChannel::from_code(channel.code()), Some(channel));
        }
        assert_eq!(ChatChannel::from_code(2), None);
    }
}
