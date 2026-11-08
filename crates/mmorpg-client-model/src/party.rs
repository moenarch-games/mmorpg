//! Party rules both clients share (#404): the party chat commands and what the party frame and
//! invite prompt show. Nothing here decides membership; the party directory validates every
//! request.

use mmorpg_core::{MAX_PARTY_NAME_BYTES, PartyRoster, PartyState, PlayerId, valid_party_name};

/// A typed party command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PartyChat {
    Invite(String),
    Leave,
    Kick(String),
}

/// Reads `/invite <name>`, `/leave` or `/kick <name>`; `None` for any other line, which then
/// goes through [`crate::chat::parse_chat_input`].
#[must_use]
pub fn parse_party_chat(raw: &str) -> Option<Result<PartyChat, String>> {
    let trimmed = raw.trim();
    let command = trimmed
        .split_once(char::is_whitespace)
        .map_or(trimmed, |(command, _)| command);
    let lower = command.to_ascii_lowercase();
    let name = trimmed
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim());
    match lower.as_str() {
        "/invite" => Some(if name.is_empty() {
            Err("Whom do you want to invite? Use /invite <name>.".to_owned())
        } else if !valid_party_name(name) {
            Err("That is not a player's name.".to_owned())
        } else {
            Ok(PartyChat::Invite(name.to_owned()))
        }),
        "/leave" | "/leaveparty" => Some(Ok(PartyChat::Leave)),
        "/kick" => Some(if name.is_empty() {
            Err("Whom do you want to remove? Use /kick <name>.".to_owned())
        } else {
            Ok(PartyChat::Kick(name.to_owned()))
        }),
        _ => None,
    }
}

/// One line of the party frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartyRow {
    pub name: String,
    /// Roster index, for kicking.
    pub index: u8,
    pub leader: bool,
    pub you: bool,
    pub online: bool,
    /// Online in another zone.
    pub elsewhere: bool,
}

/// What the party frame shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartyFrame {
    pub rows: Vec<PartyRow>,
    /// The viewer leads and may kick.
    pub leads: bool,
    /// The roster version a kick is decided on.
    pub version: u32,
}

/// The frame for the viewer's roster, or `None` without a party.
#[must_use]
pub fn party_frame(state: &PartyState, viewer: PlayerId) -> Option<PartyFrame> {
    let roster = state.roster.as_ref()?;
    let viewer_index = roster
        .members
        .iter()
        .position(|member| member.player == Some(viewer));
    let rows = roster
        .members
        .iter()
        .enumerate()
        .map(|(index, member)| PartyRow {
            name: member.name.clone(),
            index: u8::try_from(index).unwrap_or(u8::MAX),
            leader: index == 0,
            you: Some(index) == viewer_index,
            online: member.online,
            elsewhere: member.online && member.player.is_none(),
        })
        .collect();
    Some(PartyFrame {
        rows,
        leads: viewer_index == Some(0),
        version: roster.version,
    })
}

/// The roster index and version of the member named `name`, for `/kick`.
#[must_use]
pub fn kick_target(roster: &PartyRoster, name: &str) -> Option<(u8, u32)> {
    let wanted = name.trim().to_lowercase();
    roster
        .members
        .iter()
        .position(|member| member.name.to_lowercase() == wanted)
        .and_then(|index| u8::try_from(index).ok())
        .map(|index| (index, roster.version))
}

/// The character name to announce, shortened to fit the wire limit.
#[must_use]
pub fn announced_name(name: &str) -> Option<String> {
    let mut end = MAX_PARTY_NAME_BYTES.min(name.len());
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let short = name[..end].trim();
    valid_party_name(short).then(|| short.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::PartyMember;

    fn state() -> PartyState {
        PartyState {
            revision: 1,
            roster: Some(PartyRoster {
                party: 1,
                version: 7,
                members: vec![
                    PartyMember {
                        name: "Aria".into(),
                        player: Some(4),
                        online: true,
                    },
                    PartyMember {
                        name: "Bren".into(),
                        player: None,
                        online: true,
                    },
                ],
            }),
            invite: None,
        }
    }

    #[test]
    fn party_chat_parses_the_three_commands_and_leaves_the_rest_alone() {
        assert_eq!(
            parse_party_chat("/invite Bren Stone"),
            Some(Ok(PartyChat::Invite("Bren Stone".into())))
        );
        assert!(matches!(parse_party_chat("/invite"), Some(Err(_))));
        assert_eq!(parse_party_chat("/LEAVE"), Some(Ok(PartyChat::Leave)));
        assert_eq!(parse_party_chat("/s hello"), None);
        assert_eq!(parse_party_chat("hello"), None);
    }

    #[test]
    fn the_frame_marks_the_leader_and_you_and_those_elsewhere() {
        let frame = party_frame(&state(), 4).unwrap();
        assert!(frame.leads);
        assert!(frame.rows[0].you && frame.rows[0].leader);
        assert!(frame.rows[1].elsewhere);
        assert!(!party_frame(&state(), 9).unwrap().leads);
    }

    #[test]
    fn kick_targets_resolve_by_name_and_names_shorten_on_char_boundaries() {
        let roster = state().roster.unwrap();
        assert_eq!(kick_target(&roster, "bren"), Some((1, 7)));
        assert_eq!(kick_target(&roster, "nobody"), None);
        assert_eq!(announced_name("Aria"), Some("Aria".to_owned()));
        let long = "é".repeat(MAX_PARTY_NAME_BYTES);
        assert!(announced_name(&long).unwrap().len() <= MAX_PARTY_NAME_BYTES);
    }
}
