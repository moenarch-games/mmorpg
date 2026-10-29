//! A live zone's side of cross-zone transfers (#145): the trusted host
//! inputs that freeze, take back and activate characters, and the exchange
//! with the host's handoff service ([`crate::handoff_host`]).
//!
//! Everything that changes canonical zone state is a host input, so replay
//! and recovery see it in order with commands:
//!
//! - `[1, 5] · encoded transfer`: the claiming session becomes the arriving
//!   character (stage and activate).
//! - `[1, 6] · transfer ID u64`: freeze the session's player toward its
//!   current departure ([`mmorpg_core::ZoneSimulation::departures`]).
//! - `[1, 7] · transfer ID u64`: put a frozen player back live.
//! - `[1, 8]`: decline the session's departure (no destination to go to).
//!
//! A taken-back or declined player is not offered again until it leaves the
//! crossing strip; a declined or taken-back waystone departure is dropped.
//!
//! Retiring a frozen copy and the redirect notice are host-phase state, not
//! canonical: the service hands them over through [`HandoffChannel`], and
//! the zone applies them at its next tick. After the cutover the departed
//! session's projection is the redirect notice; before it, the projection
//! captured at the freeze.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use game_server::{SimulationError, SimulationSnapshot};
use mmorpg_core::{CharacterId, CharacterTransfer, Departure, PlayerId, TransferId, ZoneId};
use mmorpg_protocol::{
    RedirectNotice, decode_character_transfer, encode_character_transfer, encode_redirect_notice,
};

use crate::{ZoneGameServerAdapter, map_protocol_error, map_zone_error};

const ARRIVE: u8 = 5;
const FREEZE: u8 = 6;
const TAKE_BACK: u8 = 7;
const DECLINE: u8 = 8;

/// Where a departed player continues: the destination zone and the session
/// route its host accepts the player on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Redirect {
    pub destination: ZoneId,
    pub route: String,
}

pub(crate) struct Departing {
    pub character: CharacterId,
    pub transfer_id: TransferId,
    pub cutover_sequence: u32,
    /// The player's last projection before the freeze.
    pub projection: SimulationSnapshot,
    pub redirect: Option<Redirect>,
}

#[derive(Default)]
struct Exchange {
    /// Zone → service, each tick: players due to leave and not yet frozen.
    departures: Vec<Departure>,
    /// Zone → service: transfers frozen and not yet settled.
    frozen: BTreeMap<TransferId, CharacterTransfer>,
    /// Service → zone: transfers the destination accepted; drop the frozen
    /// copy and redirect its session.
    retired: BTreeMap<TransferId, Redirect>,
}

/// The exchange between one live zone and its host's handoff service.
#[derive(Clone, Default)]
pub struct HandoffChannel(Arc<Mutex<Exchange>>);

impl HandoffChannel {
    fn with<R>(&self, body: impl FnOnce(&mut Exchange) -> R) -> R {
        body(
            &mut self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Players due to leave as of the zone's last tick.
    #[must_use]
    pub fn departures(&self) -> Vec<Departure> {
        self.with(|exchange| exchange.departures.clone())
    }

    /// The transfer the zone froze under `transfer_id`, until it settles.
    #[must_use]
    pub fn frozen(&self, transfer_id: TransferId) -> Option<CharacterTransfer> {
        self.with(|exchange| exchange.frozen.get(&transfer_id).cloned())
    }

    /// The destination accepted: the zone drops the frozen copy at its next
    /// tick and redirects the departed session.
    pub fn retire(&self, transfer_id: TransferId, redirect: Redirect) {
        self.with(|exchange| {
            exchange.frozen.remove(&transfer_id);
            exchange.retired.insert(transfer_id, redirect);
        });
    }
}

/// The trusted host input making the session `[1, 5]` the arriving character.
pub fn arrival_input(transfer: &CharacterTransfer) -> Result<Vec<u8>, SimulationError> {
    let mut payload = vec![1, ARRIVE];
    payload.extend_from_slice(&encode_character_transfer(transfer).map_err(map_protocol_error)?);
    if payload.len() > game_server::MAX_COMMAND_PAYLOAD_BYTES {
        return Err(SimulationError::new("arrival exceeds host input limit"));
    }
    Ok(payload)
}

/// The trusted host input freezing the session's player under `transfer_id`.
#[must_use]
pub fn freeze_input(transfer_id: TransferId) -> Vec<u8> {
    let mut payload = vec![1, FREEZE];
    payload.extend_from_slice(&transfer_id.get().to_be_bytes());
    payload
}

/// The trusted host input putting the frozen player of `transfer_id` back.
#[must_use]
pub fn take_back_input(transfer_id: TransferId) -> Vec<u8> {
    let mut payload = vec![1, TAKE_BACK];
    payload.extend_from_slice(&transfer_id.get().to_be_bytes());
    payload
}

/// The trusted host input declining the session's departure.
#[must_use]
pub fn decline_input() -> Vec<u8> {
    vec![1, DECLINE]
}

fn transfer_id(body: &[u8]) -> Result<TransferId, SimulationError> {
    let bytes = <[u8; 8]>::try_from(body)
        .map_err(|_| SimulationError::new("transfer input needs a transfer ID"))?;
    TransferId::new(u64::from_be_bytes(bytes)).map_err(map_zone_error)
}

impl ZoneGameServerAdapter {
    /// Attaches the host's handoff service; the zone publishes its
    /// departures to it every tick.
    pub fn attach_handoffs(&mut self, channel: HandoffChannel) {
        self.handoffs = Some(channel);
    }

    /// Applies a transfer host input; `false` when `payload` is none.
    pub(crate) fn apply_transfer_input(
        &mut self,
        player: PlayerId,
        payload: &[u8],
    ) -> Result<bool, SimulationError> {
        match payload {
            [1, ARRIVE, body @ ..] => self.arrive(player, body)?,
            [1, FREEZE, body @ ..] => self.freeze(player, transfer_id(body)?)?,
            [1, TAKE_BACK, body @ ..] => self.take_back(transfer_id(body)?)?,
            [1, DECLINE] => {
                self.zone.cancel_departure(player);
                self.held_back.insert(player);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn arrive(&mut self, player: PlayerId, body: &[u8]) -> Result<(), SimulationError> {
        let transfer = decode_character_transfer(body).map_err(map_protocol_error)?;
        let character = transfer.record.character_id.clone();
        if self.characters.contains_key(&player)
            || self.departing.contains_key(&player)
            || self.characters.iter().any(|(session, id)| {
                *id == character && !self.parked_characters.contains_key(session)
            })
        {
            return Err(SimulationError::new(
                "character or session is already bound",
            ));
        }
        let transfer_id = transfer.transfer_id;
        self.zone.stage_transfer(transfer).map_err(map_zone_error)?;
        // The session joined as a placeholder player; the arrival replaces it.
        let placeholder = self.zone.remove_player(player);
        if let Err(error) = self.zone.activate_transfer(transfer_id, player) {
            self.zone.discard_staged_transfer(transfer_id);
            if placeholder {
                self.zone.add_player(player).map_err(map_zone_error)?;
            }
            return Err(map_zone_error(error));
        }
        self.parked_characters.remove(&player);
        self.characters.insert(player, character);
        Ok(())
    }

    fn freeze(&mut self, player: PlayerId, transfer_id: TransferId) -> Result<(), SimulationError> {
        let character = self
            .characters
            .get(&player)
            .cloned()
            .ok_or_else(|| SimulationError::new("session has no character"))?;
        let departure = self
            .zone
            .departure(player)
            .ok_or_else(|| SimulationError::new("the player is not departing"))?;
        let projection = game_server::GameSimulation::snapshot_for(self, player)?;
        let transfer = self
            .zone
            .freeze_for_transfer(
                player,
                character.clone(),
                transfer_id,
                departure.destination,
            )
            .map_err(map_zone_error)?;
        self.characters.remove(&player);
        self.departing.insert(
            player,
            Departing {
                character,
                transfer_id,
                cutover_sequence: transfer.cutover_sequence,
                projection,
                redirect: None,
            },
        );
        if let Some(channel) = &self.handoffs {
            channel.with(|exchange| {
                exchange.departures.retain(|due| due.player != player);
                exchange.frozen.insert(transfer_id, transfer);
            });
        }
        Ok(())
    }

    fn take_back(&mut self, transfer_id: TransferId) -> Result<(), SimulationError> {
        let player = self
            .zone
            .abort_transfer(transfer_id)
            .map_err(map_zone_error)?;
        self.zone.cancel_departure(player);
        self.held_back.insert(player);
        if let Some(departing) = self.departing.remove(&player) {
            self.characters.insert(player, departing.character);
        }
        if let Some(channel) = &self.handoffs {
            channel.with(|exchange| exchange.frozen.remove(&transfer_id));
        }
        Ok(())
    }

    /// Tick step: settles retired transfers from the service and publishes
    /// this tick's departures.
    pub(crate) fn exchange_departures(&mut self) {
        let Some(channel) = self.handoffs.clone() else {
            return;
        };
        let departures = self.zone.departures();
        self.held_back
            .retain(|player| departures.iter().any(|due| due.player == *player));
        let retired = channel.with(|exchange| {
            exchange.departures = departures
                .iter()
                .filter(|due| !self.held_back.contains(&due.player))
                .copied()
                .collect();
            std::mem::take(&mut exchange.retired)
        });
        for (transfer_id, redirect) in retired {
            self.zone.retire_transfer(transfer_id);
            if let Some(departing) = self
                .departing
                .values_mut()
                .find(|departing| departing.transfer_id == transfer_id)
            {
                departing.redirect = Some(redirect);
            }
        }
    }

    pub(crate) fn departing_projection(
        &self,
        departing: &Departing,
    ) -> Result<SimulationSnapshot, SimulationError> {
        let Some(redirect) = &departing.redirect else {
            return Ok(departing.projection.clone());
        };
        let tick = self.zone.current_tick();
        let notice = RedirectNotice {
            zone_id: self.zone.zone_id(),
            tick,
            destination: redirect.destination,
            transfer_id: departing.transfer_id,
            cutover_sequence: departing.cutover_sequence,
            route: redirect.route.clone(),
        };
        let payload = encode_redirect_notice(&notice).map_err(map_protocol_error)?;
        Ok(SimulationSnapshot::new(tick, payload))
    }
}
