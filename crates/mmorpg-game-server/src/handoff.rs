//! Host-side steps of a fenced cross-zone character transfer (#145, ADR 0007).
//!
//! Three authorities take part and none of them decides alone:
//!
//! - each zone's `ZoneSimulation` holds the character live in at most one
//!   place (freeze, stage, activate, retire, abort);
//! - the character store (`mmorpg-characters`) holds the character's lease
//!   and, while a handoff is pending, the frozen transfer itself, so it
//!   survives either host stopping;
//! - the control plane's fenced handoff record decides the outcome:
//!   `Accepted` is the decision point. Before it the source may abort and
//!   stays authoritative; from it on the transfer completes to the
//!   destination.
//!
//! The happy path is [`begin_outbound`] (source) → [`accept_inbound`]
//! (destination) → [`retire_outbound`] (source) → [`activate_inbound`]
//! (destination). Every step is idempotent per transfer ID. After any
//! failure, [`reconcile_outbound`] and [`reconcile_inbound`] settle a host's
//! live frozen or staged copy from the handoff record, and
//! [`resolve_pending_handoff`] settles a character whose hosts both lost
//! their host-phase state: whoever next needs the character resolves it.
//!
//! These are synchronous steps with storage I/O: hosts run them off the zone
//! tick, between ticks. [`crate::transfer_link`] carries the transfer between
//! hosts over a trusted LAN.

use std::fmt;

use mmorpg_characters::{
    Acquired, CharacterLease, CharacterStore, EntryBackend, LeaseHolder, PendingHandoff, StoreError,
};
use mmorpg_control_plane::{
    ControlPlaneError, EntityId, HandoffPhase, HandoffRegistry, HandoffTicket, ZoneDirectory,
    ZoneLease,
};
use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterTransfer, PlayerId, TransferId, ZoneError,
    ZoneSimulation,
};

/// The control plane as one host operation sees it, at one control-plane tick.
pub struct Authority<'a> {
    pub directory: &'a ZoneDirectory,
    pub handoffs: &'a mut HandoffRegistry,
    pub now_tick: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub enum HandoffError {
    Zone(ZoneError),
    Store(StoreError),
    ControlPlane(ControlPlaneError),
    /// The request does not describe a transfer between these leases.
    Invalid(&'static str),
    /// The destination may not go live before the source retired.
    NotCommitted(TransferId),
}

impl fmt::Display for HandoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zone(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
            Self::ControlPlane(error) => error.fmt(formatter),
            Self::Invalid(reason) => formatter.write_str(reason),
            Self::NotCommitted(transfer) => write!(
                formatter,
                "transfer {} has not retired its source yet",
                transfer.get()
            ),
        }
    }
}

impl std::error::Error for HandoffError {}

impl From<ZoneError> for HandoffError {
    fn from(error: ZoneError) -> Self {
        Self::Zone(error)
    }
}

impl From<StoreError> for HandoffError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<ControlPlaneError> for HandoffError {
    fn from(error: ControlPlaneError) -> Self {
        Self::ControlPlane(error)
    }
}

/// The control plane's handoff identity of a transfer.
#[must_use]
pub fn handoff_id(transfer_id: TransferId) -> mmorpg_control_plane::TransferId {
    mmorpg_control_plane::TransferId::new(transfer_id.get())
}

/// The control plane's entity identity of a character: a stable FNV-1a hash
/// of its ID. Two characters sharing a hash cannot transfer at once (the
/// control plane refuses the second), which fails closed.
#[must_use]
pub fn character_entity(character: &CharacterId) -> EntityId {
    let hash = character
        .as_str()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    EntityId::new(hash)
}

/// The character-lease holder a zone lease grants.
#[must_use]
pub const fn holder(lease: &ZoneLease) -> LeaseHolder {
    LeaseHolder {
        zone_id: lease.zone_id,
        zone_epoch: lease.epoch,
    }
}

/// What the source asks for when one of its players crosses a seam or
/// teleports.
pub struct OutboundRequest<'a> {
    pub player: PlayerId,
    pub character: &'a CharacterLease,
    pub source: &'a ZoneLease,
    pub destination: &'a ZoneLease,
    pub transfer_id: TransferId,
    /// Where the character continues, in the destination's coordinates.
    pub arrival: CharacterLocation,
}

/// Source, at the cutover: freezes the player, records the frozen transfer
/// with the character's lease and prepares the handoff. Returns the transfer
/// to send to the destination. A refused step undoes the earlier ones, so
/// the player is live at the source again.
pub fn begin_outbound<B: EntryBackend>(
    zone: &mut ZoneSimulation,
    store: &mut CharacterStore<B>,
    authority: &mut Authority<'_>,
    request: &OutboundRequest<'_>,
) -> Result<CharacterTransfer, HandoffError> {
    if request.source.zone_id != zone.zone_id()
        || request.character.holder != holder(request.source)
        || request.arrival.zone_id != request.destination.zone_id
    {
        return Err(HandoffError::Invalid(
            "the transfer does not leave this zone lease for the destination lease",
        ));
    }
    authority
        .directory
        .ensure_current(request.source, authority.now_tick)?;
    let character = &request.character.character_id;
    let transfer = zone.freeze_for_transfer(
        request.player,
        character.clone(),
        request.transfer_id,
        request.arrival,
    )?;
    let undo = |zone: &mut ZoneSimulation| zone.abort_transfer(request.transfer_id).map(|_| ());
    if let Err(error) =
        store.begin_handoff(request.character, &transfer, holder(request.destination))
    {
        undo(zone)?;
        return Err(error.into());
    }
    let ticket = HandoffTicket {
        transfer_id: handoff_id(request.transfer_id),
        entity_id: character_entity(character),
        source: request.source.clone(),
        destination: request.destination.clone(),
    };
    match authority
        .handoffs
        .prepare(ticket, authority.directory, authority.now_tick)
    {
        Ok(HandoffPhase::Aborted) => {
            store.cancel_handoff(character, request.transfer_id)?;
            undo(zone)?;
            Err(ControlPlaneError::TransferAborted(handoff_id(request.transfer_id)).into())
        }
        Ok(_) => Ok(transfer),
        Err(error) => {
            store.cancel_handoff(character, request.transfer_id)?;
            undo(zone)?;
            Err(error.into())
        }
    }
}

/// Destination, on receiving a transfer: stages it, records the acceptance
/// (the decision point) and takes over the character's lease with the
/// transfer's record committed. The character is not live until
/// [`activate_inbound`]. A refused acceptance discards the staged copy; a
/// failure after acceptance keeps it for [`reconcile_inbound`].
pub fn accept_inbound<B: EntryBackend>(
    zone: &mut ZoneSimulation,
    store: &mut CharacterStore<B>,
    authority: &mut Authority<'_>,
    destination: &ZoneLease,
    transfer: CharacterTransfer,
) -> Result<Acquired, HandoffError> {
    if destination.zone_id != zone.zone_id() {
        return Err(HandoffError::Invalid("the lease is not this zone's"));
    }
    let transfer_id = transfer.transfer_id;
    let character = transfer.record.character_id.clone();
    zone.stage_transfer(transfer)?;
    if let Err(error) = authority.handoffs.accept(
        handoff_id(transfer_id),
        destination,
        authority.directory,
        authority.now_tick,
    ) {
        zone.discard_staged_transfer(transfer_id);
        return Err(error.into());
    }
    Ok(store.complete_handoff(&character, transfer_id)?)
}

/// Source, once the destination accepted: confirms the retirement and drops
/// the frozen copy for good.
pub fn retire_outbound(
    zone: &mut ZoneSimulation,
    authority: &mut Authority<'_>,
    source: &ZoneLease,
    transfer_id: TransferId,
) -> Result<(), HandoffError> {
    authority.handoffs.commit(
        handoff_id(transfer_id),
        source,
        authority.directory,
        authority.now_tick,
    )?;
    zone.retire_transfer(transfer_id);
    Ok(())
}

/// Destination, once the source retired: makes the staged character live as
/// `player`. Only commands after the cutover sequence apply.
///
/// The source has retired when it committed the handoff, or when the
/// transfer is accepted and the source's zone lease is no longer current: a
/// fenced host has lost its frozen copy or can no longer act on it, and an
/// accepted transfer never aborts back to it.
pub fn activate_inbound(
    zone: &mut ZoneSimulation,
    authority: &Authority<'_>,
    transfer_id: TransferId,
    player: PlayerId,
) -> Result<(), HandoffError> {
    let retired = authority
        .handoffs
        .record(handoff_id(transfer_id))
        .is_some_and(|record| match record.phase {
            HandoffPhase::Committed => true,
            HandoffPhase::Accepted => authority
                .directory
                .ensure_current(&record.ticket.source, authority.now_tick)
                .is_err(),
            HandoffPhase::Prepared | HandoffPhase::Aborted => false,
        });
    if !retired {
        return Err(HandoffError::NotCommitted(transfer_id));
    }
    Ok(zone.activate_transfer(transfer_id, player)?)
}

/// How a source's transfer ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundOutcome {
    /// Nobody accepted: the source is authoritative and its player is live
    /// again under this ID (`None` when this host held no frozen copy).
    Restored(Option<PlayerId>),
    /// The destination accepted: the source retired its copy.
    Retired,
}

/// Source, after a failed or timed-out step: settles its transfer from the
/// handoff record. Aborts it while nobody accepted it (the abort races the
/// destination's acceptance in the control plane, and exactly one wins),
/// otherwise retires.
pub fn reconcile_outbound<B: EntryBackend>(
    zone: &mut ZoneSimulation,
    store: &mut CharacterStore<B>,
    authority: &mut Authority<'_>,
    source: &ZoneLease,
    character: &CharacterId,
    transfer_id: TransferId,
) -> Result<OutboundOutcome, HandoffError> {
    let id = handoff_id(transfer_id);
    let phase = authority.handoffs.record(id).map(|record| record.phase);
    let phase = match phase {
        Some(HandoffPhase::Prepared) => {
            match authority
                .handoffs
                .abort(id, source, authority.directory, authority.now_tick)
            {
                Ok(phase) => Some(phase),
                Err(ControlPlaneError::TransferAlreadyAccepted(_)) => {
                    authority.handoffs.record(id).map(|record| record.phase)
                }
                Err(error) => return Err(error.into()),
            }
        }
        phase => phase,
    };
    match phase {
        None | Some(HandoffPhase::Aborted) => {
            authority
                .directory
                .ensure_current(source, authority.now_tick)?;
            store.cancel_handoff(character, transfer_id)?;
            let restored = match zone.outbound_transfer(transfer_id) {
                Some(_) => Some(zone.abort_transfer(transfer_id)?),
                None => None,
            };
            Ok(OutboundOutcome::Restored(restored))
        }
        Some(HandoffPhase::Accepted | HandoffPhase::Committed) => {
            retire_outbound(zone, authority, source, transfer_id)?;
            Ok(OutboundOutcome::Retired)
        }
        Some(HandoffPhase::Prepared) => unreachable!("a prepared transfer was aborted above"),
    }
}

/// How a destination's transfer stands.
#[derive(Debug, Eq, PartialEq)]
pub enum InboundOutcome {
    /// Aborted or never prepared: the staged copy, if any, is gone.
    Discarded,
    /// Prepared but this host holds no staged copy to accept; the source decides.
    Waiting,
    /// Accepted: the destination holds the lease. Activate once committed.
    Accepted(Box<Acquired>),
}

/// Destination, after a failed or timed-out step: settles its staged
/// transfer from the handoff record.
pub fn reconcile_inbound<B: EntryBackend>(
    zone: &mut ZoneSimulation,
    store: &mut CharacterStore<B>,
    authority: &mut Authority<'_>,
    destination: &ZoneLease,
    character: &CharacterId,
    transfer_id: TransferId,
) -> Result<InboundOutcome, HandoffError> {
    let id = handoff_id(transfer_id);
    let mut phase = authority.handoffs.record(id).map(|record| record.phase);
    if phase == Some(HandoffPhase::Prepared) {
        if zone.staged_transfer(transfer_id).is_none() {
            return Ok(InboundOutcome::Waiting);
        }
        phase = match authority.handoffs.accept(
            id,
            destination,
            authority.directory,
            authority.now_tick,
        ) {
            Ok(phase) => Some(phase),
            Err(ControlPlaneError::TransferAborted(_)) => Some(HandoffPhase::Aborted),
            Err(error) => return Err(error.into()),
        };
    }
    match phase {
        None | Some(HandoffPhase::Aborted) => {
            zone.discard_staged_transfer(transfer_id);
            Ok(InboundOutcome::Discarded)
        }
        Some(HandoffPhase::Accepted | HandoffPhase::Committed) => Ok(InboundOutcome::Accepted(
            Box::new(store.complete_handoff(character, transfer_id)?),
        )),
        Some(HandoffPhase::Prepared) => unreachable!("a prepared transfer was accepted above"),
    }
}

/// How [`resolve_pending_handoff`] settled a character.
#[derive(Debug, Eq, PartialEq)]
pub enum Resolution {
    /// No handoff was pending.
    Clear,
    /// The handoff was taken back: the source zone keeps the character.
    Cancelled(PendingHandoff),
    /// The handoff completed: the destination holds the character's lease.
    /// Only a host of the destination zone may use it.
    Completed(Acquired),
}

/// Settles a character whose handoff is still pending in the store, for a
/// host about to acquire it (`resolver` is that host's zone lease). An
/// accepted handoff completes to its destination whoever resolves it. An
/// unaccepted one is taken back only by a newer epoch of the source zone,
/// which proves the source host that began it is fenced and can no longer
/// prepare or accept it.
pub fn resolve_pending_handoff<B: EntryBackend>(
    store: &mut CharacterStore<B>,
    authority: &mut Authority<'_>,
    resolver: &ZoneLease,
    character: &CharacterId,
) -> Result<Resolution, HandoffError> {
    authority
        .directory
        .ensure_current(resolver, authority.now_tick)?;
    let Some(pending) = store.pending_handoff(character)? else {
        return Ok(Resolution::Clear);
    };
    let transfer_id = pending.transfer.transfer_id;
    let id = handoff_id(transfer_id);
    let mut phase = authority.handoffs.record(id).map(|record| record.phase);
    if matches!(phase, None | Some(HandoffPhase::Prepared)) {
        let fenced_source =
            resolver.zone_id == pending.from.zone_id && resolver.epoch > pending.from.zone_epoch;
        if !fenced_source {
            return Err(StoreError::HandoffPending(transfer_id).into());
        }
        if phase.is_some() {
            phase = match authority.handoffs.abort(
                id,
                resolver,
                authority.directory,
                authority.now_tick,
            ) {
                Ok(phase) => Some(phase),
                Err(ControlPlaneError::TransferAlreadyAccepted(_)) => {
                    authority.handoffs.record(id).map(|record| record.phase)
                }
                Err(error) => return Err(error.into()),
            };
        }
    }
    match phase {
        None | Some(HandoffPhase::Aborted) => {
            store.cancel_handoff(character, transfer_id)?;
            Ok(Resolution::Cancelled(pending))
        }
        Some(HandoffPhase::Accepted | HandoffPhase::Committed) => {
            let acquired = store.complete_handoff(character, transfer_id)?;
            if phase == Some(HandoffPhase::Accepted)
                && resolver.zone_id == pending.transfer.source_zone
            {
                // The resolver speaks for the source, whose copy is gone.
                authority
                    .handoffs
                    .commit(id, resolver, authority.directory, authority.now_tick)?;
            }
            Ok(Resolution::Completed(acquired))
        }
        Some(HandoffPhase::Prepared) => unreachable!("a prepared transfer was aborted above"),
    }
}
