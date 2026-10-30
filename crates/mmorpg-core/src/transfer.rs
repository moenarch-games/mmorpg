//! Zone-local phases of a fenced character transfer (#145).
//!
//! Walking through a seam corridor and teleporting share one contract. The
//! zones only ever hold the character in exactly one live place:
//!
//! 1. **Freeze** (source, at the cutover tick): the source takes the player
//!    out of its live simulation and exports a [`CharacterTransfer`]: the
//!    durable [`crate::CharacterRecord`] placed in the destination's
//!    coordinates, the held movement and velocity that keep motion
//!    continuous, and the last command sequence the source applied. The
//!    source keeps everything it needs to put the player back unchanged.
//! 2. **Stage** (destination): the destination validates the transfer against
//!    its own content and holds it, not yet live. Staging is idempotent per
//!    [`TransferId`]; a different transfer under the same ID is refused.
//! 3. **Activate** (destination, after the control plane recorded the
//!    destination's acceptance): the staged character becomes a live player.
//!    Commands with a sequence at or below the cutover sequence fail closed,
//!    so nothing the source already applied is applied twice. Activating
//!    again under the same player is a no-op.
//! 4. **Retire** (source) drops the frozen player for good once the
//!    destination accepted, or **abort** (source) restores it, unchanged and
//!    live, when the destination refused or never staged it.
//!
//! The zone never decides on its own which side wins: the control plane's
//! fenced handoff record does, and a host only activates or retires after it.
//! Frozen and staged transfers are host-phase state, not canonical: recovery
//! reconciles them from the handoff record and the character store. Session
//! and tick-local state (target, queued intents, casts, auras, guard, combo,
//! swing and roll timers) ends at the cutover, as at logout.

use std::collections::{BTreeMap, VecDeque};

use crate::entity::body_id;
use crate::physics::{RigidBody, Vec3i};
use crate::rewards::RewardKey;
use crate::zone::{Axis, PlayerState};
use crate::{
    CharacterId, CharacterLocation, CharacterRecord, EntityRef, PlayerId, ZoneError, ZoneId,
    ZoneSimulation,
};

/// Stable identity of one transfer, independent of connections, sessions and
/// player IDs on either side.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransferId(u64);

impl TransferId {
    /// A nonzero ID.
    pub fn new(value: u64) -> Result<Self, ZoneError> {
        if value == 0 {
            return Err(ZoneError::new("transfer ID must be nonzero"));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Held movement carried across the cutover so motion stays continuous.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TransferMotion {
    pub forward: i8,
    pub strafe: i8,
    pub rise: i8,
    /// Body velocity in subunits per tick.
    pub velocity: [i32; 3],
}

/// Everything a destination needs to continue a character the source froze.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterTransfer {
    pub transfer_id: TransferId,
    pub source_zone: ZoneId,
    /// The source tick at which the source stopped simulating the character.
    pub cutover_tick: u64,
    /// The last command sequence the source applied; the destination accepts
    /// only later sequences.
    pub cutover_sequence: u32,
    /// The durable character, located in the destination zone's coordinates.
    pub record: CharacterRecord,
    pub motion: TransferMotion,
}

/// A player the source froze, kept until it retires or aborts.
#[derive(Clone, Debug)]
struct Frozen {
    player_id: PlayerId,
    state: PlayerState,
    body: RigidBody,
    receipts: Option<std::collections::BTreeSet<RewardKey>>,
    transfer: CharacterTransfer,
}

/// How many activated transfers a zone remembers, so a late duplicate of one
/// is recognised; the oldest is forgotten first.
pub const MAX_REMEMBERED_TRANSFERS: usize = 4_096;

/// The transfers a zone takes part in.
#[derive(Clone, Debug, Default)]
pub(crate) struct Transfers {
    outbound: BTreeMap<TransferId, Frozen>,
    staged: BTreeMap<TransferId, CharacterTransfer>,
    /// Activated transfers by the player they became.
    activated: BTreeMap<TransferId, PlayerId>,
    /// `activated` in activation order.
    activation_order: VecDeque<TransferId>,
    /// Teleports to a stone of another zone, waiting for the host to freeze
    /// the traveller ([`ZoneSimulation::departures`]).
    pub(crate) waystone_departures: BTreeMap<PlayerId, CharacterLocation>,
    /// Whether the host moves travellers to other zones
    /// ([`ZoneSimulation::enable_waystone_departures`]).
    pub(crate) waystone_departures_enabled: bool,
}

impl Transfers {
    fn remember(&mut self, transfer_id: TransferId, player_id: PlayerId) {
        if self.activation_order.len() >= MAX_REMEMBERED_TRANSFERS
            && let Some(oldest) = self.activation_order.pop_front()
        {
            self.activated.remove(&oldest);
        }
        self.activated.insert(transfer_id, player_id);
        self.activation_order.push_back(transfer_id);
    }
}

impl ZoneSimulation {
    /// Freezes `player_id` at the cutover and exports its transfer to
    /// `destination` (a seam crossing's or a teleport's arrival, in the
    /// destination's coordinates). The player leaves this zone's live
    /// simulation at once; [`Self::retire_transfer`] or
    /// [`Self::abort_transfer`] settles it. A refused freeze changes nothing.
    ///
    /// Refused while the player is climbing or mid-roll (the host retries a
    /// tick later), when this tick granted it rewards that the host has not
    /// yet committed with the tick, or for a destination in this zone.
    pub fn freeze_for_transfer(
        &mut self,
        player_id: PlayerId,
        character_id: CharacterId,
        transfer_id: TransferId,
        destination: CharacterLocation,
    ) -> Result<CharacterTransfer, ZoneError> {
        if self.transfers.outbound.contains_key(&transfer_id) {
            return Err(ZoneError::new("transfer is already in progress"));
        }
        if destination.zone_id == self.zone_id {
            return Err(ZoneError::new("a transfer must leave its source zone"));
        }
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        if player.traversal != crate::Traversal::Free || player.roll.rolling().is_some() {
            return Err(ZoneError::new(
                "a climbing or rolling player cannot transfer yet",
            ));
        }
        if self
            .rewards
            .as_ref()
            .is_some_and(|history| history.grants.iter().any(|grant| grant.player == player_id))
        {
            return Err(ZoneError::new(
                "the player's reward grants of this tick are not committed yet",
            ));
        }
        let mut record = self.capture_character(player_id, character_id)?;
        record.location = destination;
        let body = self
            .world
            .body(body_id(EntityRef::Player(player_id)))
            .cloned()
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        let velocity = body.velocity();
        let transfer = CharacterTransfer {
            transfer_id,
            source_zone: self.zone_id,
            cutover_tick: self.tick,
            cutover_sequence: player.last_sequence,
            record,
            motion: TransferMotion {
                forward: player.forward.get(),
                strafe: player.strafe.get(),
                rise: player.rise.get(),
                velocity: [velocity.x, velocity.y, velocity.z],
            },
        };
        let state = player.clone();
        let receipts = self
            .rewards
            .as_ref()
            .and_then(|history| history.players.get(&player_id).cloned());
        self.remove_player(player_id);
        self.transfers.outbound.insert(
            transfer_id,
            Frozen {
                player_id,
                state,
                body,
                receipts,
                transfer: transfer.clone(),
            },
        );
        Ok(transfer)
    }

    /// The exported transfer of a frozen player, for resending it.
    #[must_use]
    pub fn outbound_transfer(&self, transfer_id: TransferId) -> Option<&CharacterTransfer> {
        self.transfers
            .outbound
            .get(&transfer_id)
            .map(|frozen| &frozen.transfer)
    }

    /// Drops a frozen player for good once its destination accepted. Returns
    /// whether it was still frozen, so a repeated retire is harmless.
    pub fn retire_transfer(&mut self, transfer_id: TransferId) -> bool {
        self.transfers.outbound.remove(&transfer_id).is_some()
    }

    /// Puts a frozen player back, live and unchanged, under its old player ID:
    /// the destination refused or never staged the transfer. Its last command
    /// sequence is the cutover sequence, so only later commands apply.
    pub fn abort_transfer(&mut self, transfer_id: TransferId) -> Result<PlayerId, ZoneError> {
        let frozen = self
            .transfers
            .outbound
            .get(&transfer_id)
            .ok_or_else(|| ZoneError::new("unknown transfer"))?;
        let player_id = frozen.player_id;
        if self.players.contains_key(&player_id) {
            return Err(ZoneError::new("player already exists in zone"));
        }
        if self.players.len() >= crate::MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("zone player capacity reached"));
        }
        let frozen = self
            .transfers
            .outbound
            .remove(&transfer_id)
            .ok_or_else(|| ZoneError::new("unknown transfer"))?;
        let mut state = frozen.state;
        // Its slot may have gone to a newcomer meanwhile; slots only place spawns.
        if self
            .players
            .values()
            .any(|other| other.spawn_slot == state.spawn_slot)
        {
            state.spawn_slot = self
                .available_spawn_slot()
                .ok_or_else(|| ZoneError::new("zone spawn capacity reached"))?;
        }
        let position = frozen.body.position();
        self.world
            .add_body(frozen.body)
            .map_err(crate::zone::physics_error)?;
        let entity = EntityRef::Player(player_id);
        self.interest.insert(entity, position.x, position.z);
        self.interest_work.bucket_inserts += 1;
        self.players.insert(player_id, state);
        if let (Some(history), Some(receipts)) = (&mut self.rewards, frozen.receipts) {
            history.players.insert(player_id, receipts);
        }
        Ok(player_id)
    }

    /// Validates `transfer` against this zone and holds it until
    /// [`Self::activate_transfer`]; the character is not live yet. Staging the
    /// same transfer again, even after activation, is a no-op; a different
    /// transfer under the same ID, or another transfer of a character already
    /// staged, is refused.
    pub fn stage_transfer(&mut self, transfer: CharacterTransfer) -> Result<(), ZoneError> {
        if let Some(staged) = self.transfers.staged.get(&transfer.transfer_id) {
            return if *staged == transfer {
                Ok(())
            } else {
                Err(ZoneError::new("a different transfer has this ID"))
            };
        }
        if self.transfers.activated.contains_key(&transfer.transfer_id) {
            return Ok(());
        }
        if transfer.record.location.zone_id != self.zone_id || transfer.source_zone == self.zone_id
        {
            return Err(ZoneError::new("the transfer is not addressed to this zone"));
        }
        if self
            .transfers
            .staged
            .values()
            .any(|staged| staged.record.character_id == transfer.record.character_id)
        {
            return Err(ZoneError::new("the character is already staged"));
        }
        Axis::new(transfer.motion.forward)?;
        Axis::new(transfer.motion.strafe)?;
        Axis::new(transfer.motion.rise)?;
        self.validate_transfer_record(&transfer.record)?;
        self.transfers.staged.insert(transfer.transfer_id, transfer);
        Ok(())
    }

    /// Drops a staged transfer the control plane aborted. Returns whether it
    /// was staged.
    pub fn discard_staged_transfer(&mut self, transfer_id: TransferId) -> bool {
        self.transfers.staged.remove(&transfer_id).is_some()
    }

    /// The staged transfer, if any.
    #[must_use]
    pub fn staged_transfer(&self, transfer_id: TransferId) -> Option<&CharacterTransfer> {
        self.transfers.staged.get(&transfer_id)
    }

    /// Declares that this zone's host carries travellers to stones of other
    /// zones through the fenced transfer. Without it such travel is refused
    /// with [`crate::ErrorCode::InvalidWaystone`], as on a single-zone host.
    /// Host-phase configuration, not canonical: the host sets it on every
    /// zone it builds or restores.
    pub fn enable_waystone_departures(&mut self) {
        self.transfers.waystone_departures_enabled = true;
    }

    /// Takes over `previous`'s host-phase transfer state (frozen, staged and
    /// activated transfers and pending waystone departures) when a host
    /// replaces its zone with one restored from `previous`'s canonical
    /// snapshot. Departures of players this zone no longer holds are dropped.
    pub fn adopt_transfers(&mut self, previous: &mut ZoneSimulation) {
        self.transfers = std::mem::take(&mut previous.transfers);
        let players = &self.players;
        self.transfers
            .waystone_departures
            .retain(|player, _| players.contains_key(player));
    }

    /// Makes a staged character live as `player_id`, continuing its held
    /// movement and velocity; only commands after its cutover sequence are
    /// accepted. Activating the same transfer as the same player again is a
    /// no-op. A refused activation keeps the transfer staged.
    pub fn activate_transfer(
        &mut self,
        transfer_id: TransferId,
        player_id: PlayerId,
    ) -> Result<(), ZoneError> {
        if let Some(&activated) = self.transfers.activated.get(&transfer_id) {
            return if activated == player_id {
                Ok(())
            } else {
                Err(ZoneError::new("the transfer is active as another player"))
            };
        }
        let transfer = self
            .transfers
            .staged
            .get(&transfer_id)
            .ok_or_else(|| ZoneError::new("the transfer is not staged"))?
            .clone();
        self.admit_character(player_id, &transfer.record)?;
        let motion = transfer.motion;
        let alive = transfer.record.health > 0;
        if let Some(player) = self.players.get_mut(&player_id) {
            player.last_sequence = transfer.cutover_sequence;
            if alive {
                player.forward = Axis::new(motion.forward)?;
                player.strafe = Axis::new(motion.strafe)?;
                player.rise = Axis::new(motion.rise)?;
            }
        }
        if alive {
            let [x, y, z] = motion.velocity;
            self.world
                .set_velocity(body_id(EntityRef::Player(player_id)), Vec3i::new(x, y, z))
                .map_err(crate::zone::physics_error)?;
        }
        self.transfers.staged.remove(&transfer_id);
        self.transfers.remember(transfer_id, player_id);
        Ok(())
    }
}
