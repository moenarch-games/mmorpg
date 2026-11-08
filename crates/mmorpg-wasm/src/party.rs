//! The browser's party bridge (#404): the party frame shape, request and reply encoding through
//! `mmorpg-protocol`, and the chat command parser from `mmorpg-client-model`, so both clients
//! share them.

use mmorpg_client_model::party::{self as model, PartyChat};
use mmorpg_core as core;
use mmorpg_protocol::party::{PartyRequest, decode_party_reply};
use serde::{Deserialize, Serialize};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

/// One party frame row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct PartyRowView {
    pub name: String,
    pub index: u8,
    pub leader: bool,
    pub you: bool,
    pub online: bool,
    pub elsewhere: bool,
}

/// A pending invite.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null, large_number_types_as_bigints)]
pub struct PartyInviteView {
    #[tsify(type = "bigint")]
    pub invite: u64,
    pub from: String,
}

/// The party frame and any pending invite.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null, large_number_types_as_bigints)]
pub struct PartyView {
    pub rows: Vec<PartyRowView>,
    pub leads: bool,
    pub version: u32,
    pub invite: Option<PartyInviteView>,
}

/// `Some` once the party service has sent the viewer a standing; a roster-less, invite-less
/// view (no party) still comes through, with no rows.
#[must_use]
pub fn party_view(state: &core::PartyState, viewer: core::PlayerId) -> PartyView {
    let frame = model::party_frame(state, viewer);
    let (rows, leads, version) = frame.map_or((Vec::new(), false, 0), |frame| {
        (
            frame
                .rows
                .into_iter()
                .map(|row| PartyRowView {
                    name: row.name,
                    index: row.index,
                    leader: row.leader,
                    you: row.you,
                    online: row.online,
                    elsewhere: row.elsewhere,
                })
                .collect(),
            frame.leads,
            frame.version,
        )
    });
    PartyView {
        rows,
        leads,
        version,
        invite: state.invite.as_ref().map(|invite| PartyInviteView {
            invite: invite.invite,
            from: invite.from.clone(),
        }),
    }
}

/// A party request in the browser's shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[tsify(missing_as_null, large_number_types_as_bigints)]
pub enum PartyRequestInput {
    Hello {
        name: String,
    },
    InvitePlayer {
        player: u32,
    },
    InviteName {
        name: String,
    },
    Accept {
        #[tsify(type = "bigint")]
        invite: u64,
    },
    Decline {
        #[tsify(type = "bigint")]
        invite: u64,
    },
    Leave,
    Kick {
        member: u8,
        version: u32,
    },
}

fn request(input: PartyRequestInput) -> PartyRequest {
    match input {
        PartyRequestInput::Hello { name } => PartyRequest::Hello { name },
        PartyRequestInput::InvitePlayer { player } => PartyRequest::InvitePlayer { player },
        PartyRequestInput::InviteName { name } => PartyRequest::InviteName { name },
        PartyRequestInput::Accept { invite } => PartyRequest::Accept { invite },
        PartyRequestInput::Decline { invite } => PartyRequest::Decline { invite },
        PartyRequestInput::Leave => PartyRequest::Leave,
        PartyRequestInput::Kick { member, version } => PartyRequest::Kick { member, version },
    }
}

/// Encodes a party request for reliable control.
#[wasm_bindgen(js_name = encodePartyRequest)]
pub fn encode_party_request(input: Ts<PartyRequestInput>) -> Result<Vec<u8>, JsError> {
    let input = input
        .to_rust()
        .map_err(|error| JsError::new(&error.to_string()))?;
    request(input)
        .encode()
        .map_err(|error| JsError::new(&error.to_string()))
}

/// The message of a refused party reply, or `null` when it was accepted.
#[wasm_bindgen(js_name = partyReplyError)]
pub fn party_reply_error(payload: &[u8]) -> Result<Option<String>, JsError> {
    decode_party_reply(payload)
        .map(|reply| reply.err().map(|refusal| refusal.message().to_owned()))
        .map_err(|error| JsError::new(&error.to_string()))
}

/// A typed party command, or why it cannot be sent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[tsify(missing_as_null)]
pub enum PartyChatInput {
    Invite { name: String },
    Leave,
    Kick { name: String },
    Error { error: String },
}

/// Reads `/invite <name>`, `/leave` or `/kick <name>`; `null` for any other line.
#[wasm_bindgen(js_name = parsePartyChat, unchecked_return_type = "PartyChatInput | null")]
pub fn parse_party_chat(raw: &str) -> Result<JsValue, JsError> {
    let parsed = model::parse_party_chat(raw).map(|parsed| match parsed {
        Ok(PartyChat::Invite(name)) => PartyChatInput::Invite { name },
        Ok(PartyChat::Leave) => PartyChatInput::Leave,
        Ok(PartyChat::Kick(name)) => PartyChatInput::Kick { name },
        Err(error) => PartyChatInput::Error { error },
    });
    crate::client_model::to_js(&parsed)
}

/// The character name to announce, or `null` if it cannot be a party name.
#[wasm_bindgen(js_name = announcedPartyName)]
#[must_use]
pub fn announced_party_name(name: &str) -> Option<String> {
    model::announced_name(name)
}
