//! Party wire layouts (#404, docs/PROTOCOL.md): a player's party view (the projection's party
//! sheet, its canonical copy and the host input that sets it) and the reliable-control requests
//! the party service answers.

use mmorpg_core::{
    MAX_PARTY_MEMBERS, MAX_PARTY_NAME_BYTES, PartyInvite, PartyMember, PartyRefusal, PartyRoster,
    PartyState, PlayerId, valid_party_name,
};

use crate::ProtocolError;
use crate::wire::{decode_u8_count, encode_u8_count, read_u8, take};

const ROSTER_FLAG: u8 = 1;
const INVITE_FLAG: u8 = 2;
const ONLINE_FLAG: u8 = 1;
const IN_ZONE_FLAG: u8 = 2;

/// Revision and flags, a full roster, then an invite.
pub(crate) const MAX_PARTY_SHEET_BYTES: usize = 8
    + 1
    + (8 + 4 + 1 + MAX_PARTY_MEMBERS * (1 + MAX_PARTY_NAME_BYTES + 4 + 1))
    + (8 + 1 + MAX_PARTY_NAME_BYTES);

/// First byte of a party control request; character control uses `1`.
pub const PARTY_CONTROL: u8 = 2;

fn encode_name(payload: &mut Vec<u8>, name: &str) -> Result<(), ProtocolError> {
    if !valid_party_name(name) {
        return Err(ProtocolError::new("invalid party name"));
    }
    payload.push(encode_u8_count(
        name.len(),
        MAX_PARTY_NAME_BYTES,
        "party name is too long",
    )?);
    payload.extend_from_slice(name.as_bytes());
    Ok(())
}

fn decode_name(payload: &[u8], offset: &mut usize) -> Result<String, ProtocolError> {
    let length = decode_u8_count(
        payload,
        offset,
        MAX_PARTY_NAME_BYTES,
        "party name is too long",
    )?;
    let end = offset
        .checked_add(length)
        .filter(|end| *end <= payload.len())
        .ok_or_else(|| ProtocolError::new("truncated party name"))?;
    let name = std::str::from_utf8(&payload[*offset..end])
        .map_err(|_| ProtocolError::new("party name is not UTF-8"))?;
    if !valid_party_name(name) {
        return Err(ProtocolError::new("invalid party name"));
    }
    *offset = end;
    Ok(name.to_owned())
}

pub(crate) fn encode_state(payload: &mut Vec<u8>, state: &PartyState) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&state.revision.to_be_bytes());
    let flags = (u8::from(state.roster.is_some()) * ROSTER_FLAG)
        | (u8::from(state.invite.is_some()) * INVITE_FLAG);
    payload.push(flags);
    if let Some(roster) = &state.roster {
        payload.extend_from_slice(&roster.party.to_be_bytes());
        payload.extend_from_slice(&roster.version.to_be_bytes());
        payload.push(encode_u8_count(
            roster.members.len(),
            MAX_PARTY_MEMBERS,
            "party has too many members",
        )?);
        for member in &roster.members {
            encode_name(payload, &member.name)?;
            payload.extend_from_slice(&member.player.unwrap_or(0).to_be_bytes());
            payload.push(
                (u8::from(member.online) * ONLINE_FLAG)
                    | (u8::from(member.player.is_some()) * IN_ZONE_FLAG),
            );
        }
    }
    if let Some(invite) = &state.invite {
        payload.extend_from_slice(&invite.invite.to_be_bytes());
        encode_name(payload, &invite.from)?;
    }
    Ok(())
}

pub(crate) fn decode_state(
    payload: &[u8],
    offset: &mut usize,
) -> Result<PartyState, ProtocolError> {
    let revision = u64::from_be_bytes(take(payload, offset)?);
    let flags = read_u8(payload, offset)?;
    if flags & !(ROSTER_FLAG | INVITE_FLAG) != 0 {
        return Err(ProtocolError::new("unknown party flags"));
    }
    let roster = if flags & ROSTER_FLAG != 0 {
        let party = u64::from_be_bytes(take(payload, offset)?);
        let version = u32::from_be_bytes(take(payload, offset)?);
        let count = decode_u8_count(
            payload,
            offset,
            MAX_PARTY_MEMBERS,
            "party has too many members",
        )?;
        let mut members = Vec::with_capacity(count);
        for _ in 0..count {
            let name = decode_name(payload, offset)?;
            let player: PlayerId = u32::from_be_bytes(take(payload, offset)?);
            let member_flags = read_u8(payload, offset)?;
            if member_flags & !(ONLINE_FLAG | IN_ZONE_FLAG) != 0 {
                return Err(ProtocolError::new("unknown party member flags"));
            }
            members.push(PartyMember {
                name,
                player: (member_flags & IN_ZONE_FLAG != 0).then_some(player),
                online: member_flags & ONLINE_FLAG != 0,
            });
        }
        Some(PartyRoster {
            party,
            version,
            members,
        })
    } else {
        None
    };
    let invite = if flags & INVITE_FLAG != 0 {
        let invite = u64::from_be_bytes(take(payload, offset)?);
        let from = decode_name(payload, offset)?;
        Some(PartyInvite { invite, from })
    } else {
        None
    };
    let state = PartyState {
        revision,
        roster,
        invite,
    };
    state
        .validate()
        .map_err(|_| ProtocolError::new("invalid party view"))?;
    Ok(state)
}

/// The trusted host input that sets one player's party view.
pub fn encode_party_state(state: &PartyState) -> Result<Vec<u8>, ProtocolError> {
    let mut payload = Vec::with_capacity(MAX_PARTY_SHEET_BYTES);
    encode_state(&mut payload, state)?;
    Ok(payload)
}

pub fn decode_party_state(payload: &[u8]) -> Result<PartyState, ProtocolError> {
    let mut offset = 0;
    let state = decode_state(payload, &mut offset)?;
    crate::wire::ensure_fully_consumed(payload, offset)?;
    Ok(state)
}

/// A client's party request over reliable control.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PartyRequest {
    /// Announces the character's display name; sent once per connection.
    Hello {
        name: String,
    },
    InvitePlayer {
        player: PlayerId,
    },
    InviteName {
        name: String,
    },
    Accept {
        invite: u64,
    },
    Decline {
        invite: u64,
    },
    Leave,
    /// Removes the member at roster index `member`, decided on roster `version`.
    Kick {
        member: u8,
        version: u32,
    },
}

impl PartyRequest {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut payload = vec![PARTY_CONTROL];
        match self {
            Self::Hello { name } => {
                payload.push(1);
                encode_name(&mut payload, name)?;
            }
            Self::InvitePlayer { player } => {
                payload.push(2);
                payload.extend_from_slice(&player.to_be_bytes());
            }
            Self::InviteName { name } => {
                payload.push(3);
                encode_name(&mut payload, name)?;
            }
            Self::Accept { invite } => {
                payload.push(4);
                payload.extend_from_slice(&invite.to_be_bytes());
            }
            Self::Decline { invite } => {
                payload.push(5);
                payload.extend_from_slice(&invite.to_be_bytes());
            }
            Self::Leave => payload.push(6),
            Self::Kick { member, version } => {
                payload.push(7);
                payload.push(*member);
                payload.extend_from_slice(&version.to_be_bytes());
            }
        }
        Ok(payload)
    }

    pub fn decode(payload: &[u8]) -> Result<Self, ProtocolError> {
        let mut offset = 0;
        if read_u8(payload, &mut offset)? != PARTY_CONTROL {
            return Err(ProtocolError::new("not a party request"));
        }
        let invite = |offset: &mut usize| -> Result<u64, ProtocolError> {
            Ok(u64::from_be_bytes(take(payload, offset)?))
        };
        let request = match read_u8(payload, &mut offset)? {
            1 => Self::Hello {
                name: decode_name(payload, &mut offset)?,
            },
            2 => Self::InvitePlayer {
                player: u32::from_be_bytes(take(payload, &mut offset)?),
            },
            3 => Self::InviteName {
                name: decode_name(payload, &mut offset)?,
            },
            4 => Self::Accept {
                invite: invite(&mut offset)?,
            },
            5 => Self::Decline {
                invite: invite(&mut offset)?,
            },
            6 => Self::Leave,
            7 => Self::Kick {
                member: read_u8(payload, &mut offset)?,
                version: u32::from_be_bytes(take(payload, &mut offset)?),
            },
            _ => return Err(ProtocolError::new("unknown party request")),
        };
        crate::wire::ensure_fully_consumed(payload, offset)?;
        Ok(request)
    }
}

/// The service's reply: `[2, 0]` accepted, `[2, refusal code]` refused.
#[must_use]
pub fn encode_party_reply(result: Result<(), PartyRefusal>) -> Vec<u8> {
    vec![PARTY_CONTROL, result.err().map_or(0, PartyRefusal::code)]
}

/// `Ok(Ok(()))` when accepted, `Ok(Err(refusal))` when refused.
pub fn decode_party_reply(payload: &[u8]) -> Result<Result<(), PartyRefusal>, ProtocolError> {
    match *payload {
        [PARTY_CONTROL, 0] => Ok(Ok(())),
        [PARTY_CONTROL, code] => PartyRefusal::from_code(code)
            .map(Err)
            .ok_or_else(|| ProtocolError::new("unknown party refusal")),
        _ => Err(ProtocolError::new("not a party reply")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PartyState {
        PartyState {
            revision: 9,
            roster: Some(PartyRoster {
                party: 4,
                version: 2,
                members: vec![
                    PartyMember {
                        name: "Aria".into(),
                        player: Some(3),
                        online: true,
                    },
                    PartyMember {
                        name: "Bren".into(),
                        player: None,
                        online: false,
                    },
                ],
            }),
            invite: None,
        }
    }

    #[test]
    fn the_party_sheet_travels_in_a_projection_and_is_kept_until_it_changes() {
        let mut snapshot: mmorpg_core::ZoneSnapshot = crate::projection::tests::fixture_snapshot();
        snapshot.party = Some(state());
        let mut decoder = crate::SnapshotDecoder::new();
        let packed = crate::pack_snapshot(&snapshot).unwrap();
        let decoded = decoder.decode(&packed.payload).unwrap();
        assert_eq!(decoded.party, Some(state()));
        assert_ne!(decoder.revisions().party, 0);
        // Acknowledged, it stays home yet the decoder keeps presenting it.
        let quiet = crate::pack_snapshot_acknowledged(&snapshot, decoder.revisions()).unwrap();
        assert!(quiet.payload.len() < packed.payload.len());
        assert_eq!(decoder.decode(&quiet.payload).unwrap().party, Some(state()));
    }

    #[test]
    fn party_view_round_trips_and_truncation_fails_closed() {
        let bytes = encode_party_state(&state()).unwrap();
        assert_eq!(decode_party_state(&bytes).unwrap(), state());
        assert!(decode_party_state(&bytes[..bytes.len() - 1]).is_err());
        let mut invite_view = state();
        invite_view.roster = None;
        invite_view.invite = Some(PartyInvite {
            invite: 11,
            from: "Aria".into(),
        });
        let bytes = encode_party_state(&invite_view).unwrap();
        assert_eq!(decode_party_state(&bytes).unwrap(), invite_view);
    }

    #[test]
    fn requests_round_trip_and_unknown_codes_fail_closed() {
        for request in [
            PartyRequest::Hello {
                name: "Aria".into(),
            },
            PartyRequest::InvitePlayer { player: 7 },
            PartyRequest::InviteName {
                name: "Bren Stone".into(),
            },
            PartyRequest::Accept { invite: 5 },
            PartyRequest::Decline { invite: 5 },
            PartyRequest::Leave,
            PartyRequest::Kick {
                member: 2,
                version: 3,
            },
        ] {
            let bytes = request.encode().unwrap();
            assert_eq!(PartyRequest::decode(&bytes).unwrap(), request);
        }
        assert!(PartyRequest::decode(&[PARTY_CONTROL, 99]).is_err());
        assert!(PartyRequest::decode(&[1, 0]).is_err());
        assert_eq!(
            decode_party_reply(&encode_party_reply(Ok(()))).unwrap(),
            Ok(())
        );
        let refused = encode_party_reply(Err(PartyRefusal::StaleParty));
        assert_eq!(
            decode_party_reply(&refused).unwrap(),
            Err(PartyRefusal::StaleParty)
        );
    }
}
