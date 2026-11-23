//! Live handoff between zone hosts on a trusted LAN (#145, ADR 0007).
//!
//! A [`HandoffHost`] drives the fenced transfer ([`crate::handoff`]) for the
//! zones one host runs, against live sessions:
//!
//! - **Source.** Every tick each zone publishes the players due to leave
//!   (seam crossings and waystone departures). The service freezes each one
//!   through a trusted host input, records the frozen transfer with the
//!   character's lease, prepares the handoff, offers the transfer to the
//!   destination zone's host over the transfer link and, once it accepted,
//!   commits and retires the source copy. The departed session then receives
//!   a redirect notice to the destination's route in place of its
//!   projection. A destination that refuses or cannot be reached leaves the
//!   source authoritative: the player is put back live and is not offered
//!   again until it leaves the crossing strip.
//! - **Destination.** An offered transfer is validated against the zone's
//!   content, accepted (the decision point) and takes over the character's
//!   lease. The character goes live when its client connects and claims it,
//!   and only once the source retired.
//!
//! Handoff records and zone-lease checks go through a [`HandoffAuthority`]
//! (the PostgreSQL directory in [`serve_handoff_character_host`]); the
//! characters through the shared character store.

use std::collections::BTreeMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use game_server::{ControlContext, LiveMatchHost, MatchId};
use mmorpg_characters::{Acquired, CharacterLease, EntryBackend};
use mmorpg_control_plane::{HandoffAuthority, ZoneLease};
use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterTransfer, ClassChoice, Departure, PlayerId,
    TransferId, ZoneId, ZoneSimulation,
};
use tokio::net::TcpListener;

use crate::departures::{arrival_input, decline_input, freeze_input, take_back_input};
use crate::dev_characters::{DevCharacterStorage, message, zone_of};
use crate::handoff::{
    HandoffError, OutboundOutcome, OutboundRequest, Resolution, TransferZone, accept_inbound,
    activate_inbound, begin_outbound, reconcile_outbound, resolve_pending_handoff, retire_outbound,
    source_retired,
};
use crate::instance::InstanceService;
use crate::transfer_link::{TransferLink, TransferLinkHandler, serve_transfer_link};
use crate::{HandoffChannel, Redirect, ZoneGameServerAdapter};

/// How often the service looks for departures.
const DEPARTURE_POLL: Duration = Duration::from_millis(50);

/// How a peer zone's host is reached.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneRoute {
    /// The host's transfer link, `host:port`.
    pub link: String,
    /// The session route clients connect to for that zone.
    pub session: String,
}

/// One zone this host runs.
pub struct LocalZone {
    pub match_id: MatchId,
    pub lease: ZoneLease,
    pub channel: HandoffChannel,
}

#[derive(Clone)]
struct Inbound {
    transfer: CharacterTransfer,
    /// The character's lease once the transfer is accepted.
    lease: Option<CharacterLease>,
}

pub struct HandoffHost<B: EntryBackend> {
    host: LiveMatchHost<ZoneGameServerAdapter>,
    runtime: tokio::runtime::Handle,
    storage: Arc<Mutex<DevCharacterStorage<B>>>,
    authority: Mutex<Box<dyn HandoffAuthority + Send>>,
    zones: BTreeMap<ZoneId, LocalZone>,
    routes: BTreeMap<ZoneId, ZoneRoute>,
    /// The connection epoch of each claimed session, for its host inputs.
    sessions: Mutex<BTreeMap<(MatchId, PlayerId), u32>>,
    inbound: Mutex<BTreeMap<TransferId, Inbound>>,
    /// Accepted departures whose retirement this host could not confirm yet
    /// (#1096): the zone and the redirect their frozen sessions still wait for.
    unretired: Mutex<BTreeMap<TransferId, (ZoneId, Redirect)>>,
    link_timeout: Duration,
    next_transfer: AtomicU64,
    /// Resolves threshold entries and binds private runs (#404).
    instances: Option<Arc<InstanceService>>,
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    mutex.lock().map_err(message)
}

impl<B: EntryBackend> HandoffHost<B> {
    pub fn new(
        host: LiveMatchHost<ZoneGameServerAdapter>,
        storage: Arc<Mutex<DevCharacterStorage<B>>>,
        authority: Box<dyn HandoffAuthority + Send>,
        zones: Vec<LocalZone>,
        routes: BTreeMap<ZoneId, ZoneRoute>,
        link_timeout: Duration,
        instances: Option<Arc<InstanceService>>,
    ) -> Self {
        Self {
            host,
            runtime: tokio::runtime::Handle::current(),
            storage,
            authority: Mutex::new(authority),
            zones: zones
                .into_iter()
                .map(|zone| (zone.lease.zone_id, zone))
                .collect(),
            routes,
            sessions: Mutex::default(),
            inbound: Mutex::default(),
            unretired: Mutex::default(),
            link_timeout,
            // Transfer IDs only need to be unique; start at a random point.
            next_transfer: AtomicU64::new(RandomState::new().hash_one(std::process::id())),
            instances,
        }
    }

    fn fresh_transfer_id(&self) -> TransferId {
        loop {
            let value = self.next_transfer.fetch_add(1, Ordering::Relaxed);
            if let Ok(id) = TransferId::new(value) {
                return id;
            }
        }
    }

    /// Remembers the session's connection epoch for its host inputs.
    pub(crate) fn track_session(&self, id: &MatchId, context: ControlContext) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert((id.clone(), context.player_id), context.connection_epoch);
        }
        if let Some(instances) = &self.instances {
            instances.track(id, context);
        }
    }

    fn live<'a>(
        &'a self,
        zone: &'a LocalZone,
        session: Option<(PlayerId, u32)>,
    ) -> LiveZone<'a, B> {
        LiveZone {
            handoffs: self,
            zone,
            session,
            redirect: None,
        }
    }

    /// Destination: a claim of a character whose accepted transfer this host
    /// holds makes it live for the claiming session, once the source
    /// retired. `Ok(false)` when no such transfer is held.
    pub(crate) fn claim_arrival(
        &self,
        storage: &mut DevCharacterStorage<B>,
        id: &MatchId,
        context: ControlContext,
        character: &CharacterId,
        class: ClassChoice,
    ) -> Result<bool, String> {
        let Some(zone) = self.zones.get(&zone_of(id)?) else {
            return Ok(false);
        };
        let found = lock(&self.inbound)?
            .iter()
            .find(|(_, inbound)| {
                inbound.transfer.record.character_id == *character && inbound.lease.is_some()
            })
            .map(|(&transfer_id, inbound)| (transfer_id, inbound.clone()));
        let Some((transfer_id, inbound)) = found else {
            return Ok(false);
        };
        if inbound.transfer.record.class != class {
            return Err("stored character does not match the selected class/sex".into());
        }
        if let Some(instances) = &self.instances {
            instances.bind_for_arrival(id, context, character)?;
        }
        let mut live = self.live(zone, Some((context.player_id, context.connection_epoch)));
        let mut authority = lock(&self.authority)?;
        activate_inbound(&mut live, &mut **authority, transfer_id, context.player_id)
            .map_err(message)?;
        lock(&self.inbound)?.remove(&transfer_id);
        let lease = inbound.lease.ok_or("the arrival has no lease")?;
        storage
            .leases
            .insert((id.clone(), context.player_id), lease);
        Ok(true)
    }

    /// Settles a character whose handoff is pending in the store before this
    /// host acquires it ([`resolve_pending_handoff`]).
    pub(crate) fn resolve_pending(
        &self,
        storage: &mut DevCharacterStorage<B>,
        zone_id: ZoneId,
        character: &CharacterId,
    ) -> Result<Acquired, String> {
        let zone = self
            .zones
            .get(&zone_id)
            .ok_or("this host does not run the zone")?;
        let holder = storage.holder(zone_id);
        let mut authority = lock(&self.authority)?;
        match resolve_pending_handoff(&mut storage.store, &mut **authority, &zone.lease, character)
            .map_err(message)?
        {
            Resolution::Completed(acquired) if acquired.lease.holder == holder => Ok(acquired),
            Resolution::Completed(_) => Err("the character continues in another zone".into()),
            Resolution::Cancelled(_) | Resolution::Clear => {
                storage.store.acquire(character, holder).map_err(message)
            }
        }
    }

    /// Source: starts a transfer for every player due to leave. Runs off the
    /// zone tick on a blocking worker.
    pub fn process_departures(&self) {
        if !self.host.is_serving() {
            return;
        }
        self.retry_retirements();
        for zone in self.zones.values() {
            for due in zone.channel.departures() {
                if let Err(error) = self.depart(zone, due) {
                    eprintln!(
                        "zone {}: handoff of player {} failed: {error}",
                        zone.lease.zone_id.get(),
                        due.player
                    );
                }
            }
        }
    }

    /// Source: confirms the retirements that failed after their destination
    /// accepted (#1096). The destination owns those characters whatever
    /// happens here; this releases their frozen sessions to it.
    fn retry_retirements(&self) {
        let Ok(pending) = lock(&self.unretired).map(|unretired| unretired.clone()) else {
            return;
        };
        for (transfer_id, (zone_id, redirect)) in pending {
            let Some(zone) = self.zones.get(&zone_id) else {
                continue;
            };
            let mut live = self.live(zone, None);
            live.redirect = Some(redirect);
            let retired = lock(&self.authority).and_then(|mut authority| {
                retire_outbound(&mut live, &mut **authority, &zone.lease, transfer_id)
                    .map_err(message)
            });
            if retired.is_ok()
                && let Ok(mut unretired) = lock(&self.unretired)
            {
                unretired.remove(&transfer_id);
            }
        }
    }

    fn depart(&self, zone: &LocalZone, due: Departure) -> Result<(), String> {
        let key = (zone.match_id.clone(), due.player);
        let Some(epoch) = lock(&self.sessions)?.get(&key).copied() else {
            return Ok(());
        };
        let mut storage = lock(&self.storage)?;
        let Some(lease) = storage.leases.get(&key).cloned() else {
            return Ok(());
        };
        let mut authority = lock(&self.authority)?;
        let mut live = self.live(zone, Some((due.player, epoch)));
        let destination_zone = due.destination.zone_id;
        let destination = authority.current_lease(destination_zone).map_err(message)?;
        let (Some(destination), Some(route)) = (destination, self.routes.get(&destination_zone))
        else {
            live.submit(&decline_input()).map_err(message)?;
            return Err(format!(
                "zone {} has no host to go to",
                destination_zone.get()
            ));
        };
        live.redirect = Some(Redirect {
            destination: destination_zone,
            route: route.session.clone(),
        });
        let transfer_id = self.fresh_transfer_id();
        let request = OutboundRequest {
            player: due.player,
            character: &lease,
            source: &zone.lease,
            destination: &destination,
            transfer_id,
            arrival: due.destination,
        };
        let transfer =
            match begin_outbound(&mut live, &mut storage.store, &mut **authority, &request) {
                Ok(transfer) => transfer,
                // The zone refused the freeze (climbing, rolling, uncommitted
                // rewards): the player stays live and departs on a later poll.
                Err(HandoffError::Host(_)) if live.zone.channel.frozen(transfer_id).is_none() => {
                    return Ok(());
                }
                Err(error) => return Err(error.to_string()),
            };
        // A destination on this host is accepted in place, under the guards held here.
        let remote = !self.zones.contains_key(&destination_zone);
        let offered = if remote {
            // No host-wide guard is held across the network offer (#1098): the peer may be
            // offering to this host at the same moment, and its link handler here needs both
            // guards, as do this host's claims and saves. The frozen character is neither
            // live nor writable meanwhile (its handoff is pending in the store), so its lease
            // leaves the storage's live set until the offer settles: a save pass in between
            // must not take it for a lost session and release it.
            storage.leases.remove(&key);
            drop(authority);
            drop(storage);
            let offered = self
                .runtime
                .block_on(async {
                    TransferLink::connect(route.link.as_str(), self.link_timeout)
                        .await?
                        .offer(&transfer)
                        .await
                })
                .map_err(message);
            storage = lock(&self.storage)?;
            authority = lock(&self.authority)?;
            offered
        } else {
            self.accept_local(&mut storage, &mut **authority, &transfer)
        };
        let outcome = match offered {
            Ok(()) => retire_outbound(&mut live, &mut **authority, &zone.lease, transfer_id)
                .map(|()| OutboundOutcome::Retired),
            Err(_) => reconcile_outbound(
                &mut live,
                &mut storage.store,
                &mut **authority,
                &zone.lease,
                &lease.character_id,
                transfer_id,
            ),
        };
        if offered.is_ok() || outcome == Ok(OutboundOutcome::Retired) {
            // The destination holds the character's lease now.
            storage.leases.remove(&key);
            lock(&self.sessions)?.remove(&key);
        } else if remote {
            // Still this host's character: its lease is live here again.
            storage.leases.insert(key.clone(), lease.clone());
        }
        if offered.is_ok()
            && outcome.is_err()
            && let Some(redirect) = live.redirect.clone()
        {
            // Accepted, but the retirement is unconfirmed: confirm it on a later poll.
            lock(&self.unretired)?.insert(transfer_id, (zone.lease.zone_id, redirect));
        }
        drop(authority);
        drop(storage);
        match (outcome, offered) {
            (Ok(OutboundOutcome::Retired), _) => {}
            (Ok(OutboundOutcome::Restored(_)), Err(error)) => {
                return Err(format!("the destination did not accept: {error}"));
            }
            (Ok(OutboundOutcome::Restored(_)), Ok(())) => return Ok(()),
            (Err(error), _) => return Err(error.to_string()),
        }
        // Tell the destination the cutover is authorized; it checks the
        // record itself when the client claims, so a lost message is harmless.
        if self.zones.contains_key(&destination_zone) {
            return Ok(());
        }
        let _ = self.runtime.block_on(async {
            TransferLink::connect(route.link.as_str(), self.link_timeout)
                .await?
                .activate(transfer_id)
                .await
        });
        Ok(())
    }

    /// Accepts an offered transfer into one of this host's zones, with the character's lease
    /// and the handoff record under the caller's guards.
    fn accept_local(
        &self,
        storage: &mut DevCharacterStorage<B>,
        authority: &mut (dyn HandoffAuthority + Send),
        transfer: &CharacterTransfer,
    ) -> Result<(), String> {
        let zone = self
            .zones
            .get(&transfer.record.location.zone_id)
            .ok_or("this host does not run the destination zone")?;
        let transfer_id = transfer.transfer_id;
        let mut live = self.live(zone, None);
        let acquired = accept_inbound(
            &mut live,
            &mut storage.store,
            authority,
            &zone.lease,
            transfer.clone(),
        )
        .map_err(message)?;
        if let Some(inbound) = lock(&self.inbound)?.get_mut(&transfer_id) {
            inbound.lease = Some(acquired.lease);
        }
        Ok(())
    }

    /// Serves the transfer link and starts departures until the link fails.
    pub async fn run(self: Arc<Self>, listener: TcpListener) -> Result<(), String>
    where
        B: Send + 'static,
    {
        if let Some(instances) = &self.instances {
            tokio::spawn(Arc::clone(instances).run());
        }
        let handler: Arc<dyn TransferLinkHandler> = self.clone();
        let link = serve_transfer_link(listener, handler);
        let departures = async {
            let mut interval = tokio::time::interval(DEPARTURE_POLL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let handoffs = Arc::clone(&self);
                let _ = tokio::task::spawn_blocking(move || handoffs.process_departures()).await;
            }
        };
        tokio::select! {
            result = link => result.map_err(message),
            () = departures => Ok(()),
        }
    }
}

impl<B: EntryBackend + Send + 'static> TransferLinkHandler for HandoffHost<B> {
    fn offer(&self, transfer: CharacterTransfer) -> Result<(), String> {
        let mut storage = lock(&self.storage)?;
        let mut authority = lock(&self.authority)?;
        self.accept_local(&mut storage, &mut **authority, &transfer)
    }

    fn activate(&self, transfer_id: TransferId) -> Result<(), String> {
        let mut authority = lock(&self.authority)?;
        if source_retired(&mut **authority, transfer_id).map_err(message)? {
            Ok(())
        } else {
            Err("the source has not retired the transfer".into())
        }
    }
}

/// A live zone's side of one transfer step, through host inputs and the
/// zone's [`HandoffChannel`].
struct LiveZone<'a, B: EntryBackend> {
    handoffs: &'a HandoffHost<B>,
    zone: &'a LocalZone,
    /// The session the host inputs apply to.
    session: Option<(PlayerId, u32)>,
    /// Where a retired player is redirected.
    redirect: Option<Redirect>,
}

impl<B: EntryBackend> LiveZone<'_, B> {
    fn submit(&self, payload: &[u8]) -> Result<(), HandoffError> {
        let (player, epoch) = self
            .session
            .ok_or_else(|| HandoffError::Host("no session for the host input".into()))?;
        self.handoffs
            .runtime
            .block_on(self.handoffs.host.submit_host_input(
                &self.zone.match_id,
                player,
                epoch,
                payload,
            ))
            .map_err(|error| HandoffError::Host(error.to_string()))
    }

    fn inbound(&self) -> Result<MutexGuard<'_, BTreeMap<TransferId, Inbound>>, HandoffError> {
        self.handoffs
            .inbound
            .lock()
            .map_err(|error| HandoffError::Host(error.to_string()))
    }
}

impl<B: EntryBackend> TransferZone for LiveZone<'_, B> {
    fn zone_id(&self) -> ZoneId {
        self.zone.lease.zone_id
    }

    fn freeze(
        &mut self,
        player: PlayerId,
        _character: CharacterId,
        transfer_id: TransferId,
        _arrival: CharacterLocation,
    ) -> Result<CharacterTransfer, HandoffError> {
        if self.session.map(|(session, _)| session) != Some(player) {
            return Err(HandoffError::Invalid("the freeze names another session"));
        }
        // The zone freezes toward the player's departure as of this input.
        self.submit(&freeze_input(transfer_id))?;
        self.zone
            .channel
            .frozen(transfer_id)
            .ok_or_else(|| HandoffError::Host("the zone froze no transfer".into()))
    }

    fn has_frozen(&self, transfer_id: TransferId) -> bool {
        self.zone.channel.frozen(transfer_id).is_some()
    }

    fn abort(&mut self, transfer_id: TransferId) -> Result<PlayerId, HandoffError> {
        let (player, _) = self
            .session
            .ok_or_else(|| HandoffError::Host("no session to restore".into()))?;
        self.submit(&take_back_input(transfer_id))?;
        Ok(player)
    }

    fn retire(&mut self, transfer_id: TransferId) {
        if let Some(redirect) = self.redirect.clone() {
            self.zone.channel.retire(transfer_id, redirect);
        }
    }

    fn stage(&mut self, transfer: CharacterTransfer) -> Result<(), HandoffError> {
        let transfer_id = transfer.transfer_id;
        if let Some(staged) = self.inbound()?.get(&transfer_id) {
            return if staged.transfer == transfer {
                Ok(())
            } else {
                Err(HandoffError::Invalid("a different transfer has this ID"))
            };
        }
        // Validate against this zone's content without touching the live zone.
        let zone_id = self.zone_id();
        let mut scratch =
            ZoneSimulation::with_content(zone_id, mmorpg_core::content_for_zone(zone_id))?;
        scratch.stage_transfer(transfer.clone())?;
        self.inbound()?.insert(
            transfer_id,
            Inbound {
                transfer,
                lease: None,
            },
        );
        Ok(())
    }

    fn has_staged(&self, transfer_id: TransferId) -> bool {
        self.inbound()
            .is_ok_and(|inbound| inbound.contains_key(&transfer_id))
    }

    fn discard(&mut self, transfer_id: TransferId) {
        if let Ok(mut inbound) = self.inbound() {
            inbound.remove(&transfer_id);
        }
    }

    fn activate(&mut self, transfer_id: TransferId, player: PlayerId) -> Result<(), HandoffError> {
        if self.session.map(|(session, _)| session) != Some(player) {
            return Err(HandoffError::Invalid("the arrival names another session"));
        }
        let transfer = self
            .inbound()?
            .get(&transfer_id)
            .map(|inbound| inbound.transfer.clone())
            .ok_or(HandoffError::Invalid("the transfer is not staged"))?;
        let input =
            arrival_input(&transfer).map_err(|error| HandoffError::Host(error.to_string()))?;
        self.submit(&input)
    }
}

#[cfg(feature = "postgres")]
pub use postgres_host::{HandoffHostConfig, serve_handoff_character_host};

#[cfg(feature = "postgres")]
mod postgres_host {
    use std::collections::BTreeMap;
    use std::error::Error;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use mmorpg_characters::CharacterStore;
    use mmorpg_characters::postgres::PostgresEntries;
    use mmorpg_control_plane::postgres::{
        PostgresHandoffs, PostgresInstances, PostgresParties, PostgresZoneDirectory,
        UnconfirmedZoneGrant,
    };
    use mmorpg_control_plane::{HostId, ZoneLease};
    use mmorpg_core::ZoneId;
    use tokio::sync::mpsc;

    use super::{HandoffHost, LocalZone, ZoneRoute};
    use crate::dev_characters::{DevCharacterStorage, message};
    use crate::dev_host::{HandoffWiring, serve_characters};
    use crate::instance::{InstanceChannel, InstanceService, InstanceZone};
    use crate::{HandoffChannel, zone_match_id};

    type HostResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

    /// A trusted-LAN zone host whose characters, zone grants and handoff
    /// records live in one PostgreSQL database shared with its peer hosts.
    pub struct HandoffHostConfig {
        pub database_url: String,
        pub host_id: HostId,
        pub zone_ids: Vec<ZoneId>,
        /// Where this host's transfer link listens, `host:port`.
        pub link_address: String,
        /// How to reach the hosts of other zones.
        pub routes: BTreeMap<ZoneId, ZoneRoute>,
        pub reconnect_grace_ticks: u64,
        /// Zone grant lifetime; grants renew every third of it.
        pub grant_ttl: Duration,
        /// Bounds every database statement and link request.
        pub timeout: Duration,
        /// How long an emptied Redbrand Depths run stays resumable (#404).
        pub depths_grace: Duration,
        /// The operator vouches that this host ID's previous process is stopped, so its
        /// unexpired grants are taken back at once (`MMORPG_CONFIRM_SOURCE_STOPPED=1`).
        pub confirmed_stopped: bool,
    }

    /// How often a starting host looks whether its previous grant has expired.
    const OWN_GRANT_POLL: Duration = Duration::from_millis(250);

    /// Retires the unretired grant `previous` that this host ID's earlier
    /// process left behind (#1097). The matching ID alone proves nothing: that
    /// process may be hung, not stopped, and would keep simulating the zone.
    /// So unless the operator vouches that it stopped, the grant must first
    /// expire unrenewed. A running host stops serving before its grant's
    /// deadline by its own clock (the renewal watchdog below), so an expired
    /// grant has no writer left; a grant that is renewed meanwhile has one.
    fn retire_own(
        directory: &mut PostgresZoneDirectory,
        previous: &ZoneLease,
        ttl: Duration,
        confirmed_stopped: bool,
    ) -> Result<(), String> {
        if confirmed_stopped {
            return directory.retire(previous).map_err(message);
        }
        let zone = previous.zone_id;
        let started = Instant::now();
        let mut announced = false;
        loop {
            if directory.retire_expired(previous).map_err(message)? {
                return Ok(());
            }
            match directory.assignment(zone).map_err(message)? {
                // Its previous process retired it on its way out.
                Some(current) if current.retired && current.lease.epoch == previous.epoch => {
                    return Ok(());
                }
                Some(current) if !current.retired && current.lease == *previous => {}
                _ => {
                    return Err(format!(
                        "zone {} is being served by another process with host ID {}; stop it first",
                        zone.get(),
                        previous.host_id.as_str()
                    ));
                }
            }
            if started.elapsed() > ttl + Duration::from_secs(1) {
                return Err(format!(
                    "zone {}: the previous grant of host ID {} did not expire",
                    zone.get(),
                    previous.host_id.as_str()
                ));
            }
            if !announced {
                announced = true;
                eprintln!(
                    "zone {}: waiting for this host ID's previous grant to expire (at most {} s); \
                     set MMORPG_CONFIRM_SOURCE_STOPPED=1 if its process is known to be stopped",
                    zone.get(),
                    ttl.as_secs()
                );
            }
            std::thread::sleep(OWN_GRANT_POLL);
        }
    }

    /// Takes over `zone`'s grant: a first grant, one after a retired owner,
    /// or this host's own grant from before a restart ([`retire_own`]).
    fn acquire(
        directory: &mut PostgresZoneDirectory,
        zone: ZoneId,
        host: &HostId,
        ttl: Duration,
        confirmed_stopped: bool,
    ) -> Result<UnconfirmedZoneGrant, String> {
        let expected = match directory.assignment(zone).map_err(message)? {
            None => 0,
            Some(assignment) if assignment.retired => assignment.lease.epoch,
            Some(assignment) if assignment.lease.host_id == *host => {
                retire_own(directory, &assignment.lease, ttl, confirmed_stopped)?;
                assignment.lease.epoch
            }
            Some(assignment) => {
                return Err(format!(
                    "zone {} is held by host {}; retire it first",
                    zone.get(),
                    assignment.lease.host_id.as_str()
                ));
            }
        };
        directory
            .acquire(zone, host.clone(), expected, ttl)
            .map_err(message)
    }

    pub async fn serve_handoff_character_host(
        config: HandoffHostConfig,
        transport: game_server::MatchHostWebTransportConfig,
        status: game_server::MatchHostStatusConfig,
        mut shutdown: mpsc::Receiver<()>,
    ) -> HostResult<()> {
        let HandoffHostConfig {
            database_url: url,
            host_id,
            zone_ids,
            link_address,
            routes,
            reconnect_grace_ticks,
            grant_ttl,
            timeout,
            depths_grace,
            confirmed_stopped,
        } = config;
        let zones = zone_ids.clone();
        let (directory, grants, entries, handoffs, parties, instances) =
            tokio::task::spawn_blocking(move || -> Result<_, String> {
                let mut client =
                    ::postgres::Client::connect(&url, ::postgres::NoTls).map_err(message)?;
                mmorpg_control_plane::postgres::migrate(&mut client).map_err(message)?;
                mmorpg_characters::postgres::migrate(&mut client).map_err(message)?;
                let mut directory =
                    PostgresZoneDirectory::connect(&url, timeout).map_err(message)?;
                let grants = zones
                    .iter()
                    .map(|&zone| {
                        acquire(&mut directory, zone, &host_id, grant_ttl, confirmed_stopped)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let entries = PostgresEntries::connect(&url, timeout).map_err(message)?;
                let handoffs = PostgresHandoffs::connect(&url, timeout).map_err(message)?;
                let parties = PostgresParties::connect(&url, timeout).map_err(message)?;
                let instances = PostgresInstances::connect(&url, timeout).map_err(message)?;
                Ok((directory, grants, entries, handoffs, parties, instances))
            })
            .await??;
        let leases: Vec<_> = grants.iter().map(|grant| grant.lease().clone()).collect();
        let storage = DevCharacterStorage::shared(
            CharacterStore::new(entries),
            leases
                .iter()
                .map(|lease| (lease.zone_id, lease.epoch))
                .collect(),
        );
        let mut local = Vec::new();
        for lease in &leases {
            local.push(LocalZone {
                match_id: zone_match_id(lease.zone_id)?,
                lease: lease.clone(),
                channel: HandoffChannel::default(),
            });
        }
        let channels = local
            .iter()
            .map(|zone| (zone.match_id.clone(), zone.channel.clone()))
            .collect();
        // Every zone reports to the instance service: public thresholds and depths slots.
        let instance_zones: BTreeMap<_, _> = local
            .iter()
            .map(|zone| {
                (
                    zone.match_id.clone(),
                    InstanceZone {
                        zone: zone.lease.zone_id,
                        lease: Some(zone.lease.clone()),
                        channel: InstanceChannel::default(),
                    },
                )
            })
            .collect();
        let instance_channels = instance_zones
            .iter()
            .map(|(id, zone)| (id.clone(), zone.channel.clone()))
            .collect();
        let listener = tokio::net::TcpListener::bind(link_address.as_str()).await?;
        let ids = local.iter().map(|zone| zone.match_id.clone()).collect();
        let party_leases = local
            .iter()
            .map(|zone| (zone.match_id.clone(), zone.lease.clone()))
            .collect();
        let wiring = HandoffWiring {
            channels,
            instance_channels,
            parties: Box::new(parties),
            leases: party_leases,
            listener,
            build: Box::new(move |host, storage| {
                let instances = InstanceService::new(
                    host.clone(),
                    Box::new(instances),
                    instance_zones,
                    depths_grace,
                );
                Arc::new(HandoffHost::new(
                    host,
                    storage,
                    Box::new(handoffs),
                    local,
                    routes,
                    timeout,
                    Some(instances),
                ))
            }),
        };

        // Renew the zone grants; a lost grant stops the host.
        let directory = Arc::new(Mutex::new((directory, grants)));
        let (stop, stopped) = mpsc::channel(4);
        let renewals = Arc::clone(&directory);
        let renew_stop = stop.clone();
        let renewal = tokio::spawn(async move {
            let mut interval = tokio::time::interval(grant_ttl / 3);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                let renewals = Arc::clone(&renewals);
                let renewal = tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let mut guard = renewals.lock().map_err(message)?;
                    let (directory, grants) = &mut *guard;
                    for grant in grants.iter_mut() {
                        *grant = directory.renew(grant, grant_ttl).map_err(message)?;
                    }
                    Ok(())
                });
                // The host fences itself by its own clock (#1097): a renewal that began a
                // third of the lifetime after the last one and has not answered within
                // another third stops the host, a third before the grant can expire. A
                // successor may take an expired grant over without asking this process.
                let renewed = match tokio::time::timeout(grant_ttl / 3, renewal).await {
                    Ok(joined) => joined.map_err(message).and_then(|result| result),
                    Err(_) => Err("the directory did not answer in time".into()),
                };
                if let Err(error) = renewed {
                    eprintln!("zone grant renewal failed, stopping: {error}");
                    let _ = renew_stop.send(()).await;
                    break;
                }
            }
        });
        let forward = tokio::spawn(async move {
            while shutdown.recv().await.is_some() {
                if stop.send(()).await.is_err() {
                    break;
                }
            }
        });

        // Zone state starts fresh each run; characters come from the store.
        let recovery = std::env::temp_dir().join(format!(
            "mmorpg-handoff-host-{}-{}",
            std::process::id(),
            leases.first().map_or(0, |lease| lease.epoch)
        ));
        let _ = std::fs::remove_dir_all(&recovery);
        let served = serve_characters(
            storage,
            ids,
            recovery.clone(),
            reconnect_grace_ticks,
            Some(wiring),
            transport,
            status,
            stopped,
        )
        .await;
        renewal.abort();
        forward.abort();
        let _ = std::fs::remove_dir_all(&recovery);
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut guard = directory.lock().map_err(message)?;
            let (directory, grants) = &mut *guard;
            for grant in grants.iter() {
                directory.retire(grant.lease()).map_err(message)?;
            }
            Ok(())
        })
        .await??;
        served
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// #1097: a restarted host takes its own grant back only when the
        /// previous process is provably gone. Needs PostgreSQL
        /// (`MMORPG_CHARACTERS_POSTGRES_URL`); skipped without it.
        #[test]
        fn a_restart_takes_its_own_grant_back_only_once_it_expired_unrenewed() {
            let Ok(url) = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL") else {
                return;
            };
            let mut client = ::postgres::Client::connect(&url, ::postgres::NoTls).unwrap();
            mmorpg_control_plane::postgres::migrate(&mut client).unwrap();
            client
                .execute("SELECT pg_advisory_lock($1)", &[&0x4d4d_5a44_5445_5354_i64])
                .unwrap();
            let first: i64 = client
                .query_one(
                    "SELECT coalesce(max(zone_id),1000)+1 FROM mmorpg_coordination.zones",
                    &[],
                )
                .unwrap()
                .get(0);
            let zone = |offset: i64| ZoneId::new(u32::try_from(first + offset).unwrap());
            let timeout = Duration::from_secs(5);
            let host = HostId::new("restarting-host").unwrap();
            let short = Duration::from_millis(600);
            let mut directory = PostgresZoneDirectory::connect(&url, timeout).unwrap();

            // Crashed: the grant expires unrenewed, and the restart waits for that.
            let crashed = acquire(&mut directory, zone(0), &host, short, false).unwrap();
            let started = Instant::now();
            let next = acquire(&mut directory, zone(0), &host, short, false).unwrap();
            assert_eq!(next.lease().epoch, crashed.lease().epoch + 1);
            assert!(started.elapsed() >= Duration::from_millis(300));

            // Hung, not stopped: the grant is renewed meanwhile, so the restart refuses.
            let live = acquire(&mut directory, zone(1), &host, short, false).unwrap();
            let renewing = std::thread::spawn({
                let url = url.clone();
                move || {
                    let mut directory = PostgresZoneDirectory::connect(&url, timeout).unwrap();
                    std::thread::sleep(Duration::from_millis(200));
                    directory.renew(&live, Duration::from_secs(30)).unwrap()
                }
            });
            let refused = acquire(&mut directory, zone(1), &host, short, false)
                .err()
                .expect("a renewed grant has a live holder");
            assert!(refused.contains("another process"), "{refused}");
            let live = renewing.join().unwrap();

            // The operator vouches that it is stopped: taken back at once.
            let started = Instant::now();
            let vouched = acquire(&mut directory, zone(1), &host, short, true).unwrap();
            assert_eq!(vouched.lease().epoch, live.lease().epoch + 1);
            assert!(started.elapsed() < Duration::from_millis(300));

            // Another host's grant is never taken, expired or not.
            let other = HostId::new("other-host").unwrap();
            assert!(acquire(&mut directory, zone(1), &other, short, true).is_err());
        }
    }
}
