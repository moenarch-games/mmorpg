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
    link_timeout: Duration,
    next_transfer: AtomicU64,
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
            link_timeout,
            // Transfer IDs only need to be unique; start at a random point.
            next_transfer: AtomicU64::new(RandomState::new().hash_one(std::process::id())),
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
        let offered = self.runtime.block_on(async {
            TransferLink::connect(route.link.as_str(), self.link_timeout)
                .await?
                .offer(&transfer)
                .await
        });
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
        let _ = self.runtime.block_on(async {
            TransferLink::connect(route.link.as_str(), self.link_timeout)
                .await?
                .activate(transfer_id)
                .await
        });
        Ok(())
    }

    /// Serves the transfer link and starts departures until the link fails.
    pub async fn run(self: Arc<Self>, listener: TcpListener) -> Result<(), String>
    where
        B: Send + 'static,
    {
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
        let zone = self
            .zones
            .get(&transfer.record.location.zone_id)
            .ok_or("this host does not run the destination zone")?;
        let transfer_id = transfer.transfer_id;
        let mut storage = lock(&self.storage)?;
        let mut authority = lock(&self.authority)?;
        let mut live = self.live(zone, None);
        let acquired = accept_inbound(
            &mut live,
            &mut storage.store,
            &mut **authority,
            &zone.lease,
            transfer,
        )
        .map_err(message)?;
        if let Some(inbound) = lock(&self.inbound)?.get_mut(&transfer_id) {
            inbound.lease = Some(acquired.lease);
        }
        Ok(())
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
    use std::time::Duration;

    use mmorpg_characters::CharacterStore;
    use mmorpg_characters::postgres::PostgresEntries;
    use mmorpg_control_plane::HostId;
    use mmorpg_control_plane::postgres::{
        PostgresHandoffs, PostgresZoneDirectory, UnconfirmedZoneGrant,
    };
    use mmorpg_core::ZoneId;
    use tokio::sync::mpsc;

    use super::{HandoffHost, LocalZone, ZoneRoute};
    use crate::dev_characters::{DevCharacterStorage, message};
    use crate::dev_host::{HandoffWiring, serve_characters};
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
    }

    /// Takes over `zone`'s grant: a first grant, one after a retired owner,
    /// or this host's own grant from before a restart (its previous process
    /// has stopped, so the trusted-LAN host retires it itself).
    fn acquire(
        directory: &mut PostgresZoneDirectory,
        zone: ZoneId,
        host: &HostId,
        ttl: Duration,
    ) -> Result<UnconfirmedZoneGrant, String> {
        let expected = match directory.assignment(zone).map_err(message)? {
            None => 0,
            Some(assignment) if assignment.retired => assignment.lease.epoch,
            Some(assignment) if assignment.lease.host_id == *host => {
                directory.retire(&assignment.lease).map_err(message)?;
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
        } = config;
        let zones = zone_ids.clone();
        let (directory, grants, entries, handoffs) =
            tokio::task::spawn_blocking(move || -> Result<_, String> {
                let mut client =
                    ::postgres::Client::connect(&url, ::postgres::NoTls).map_err(message)?;
                mmorpg_control_plane::postgres::migrate(&mut client).map_err(message)?;
                mmorpg_characters::postgres::migrate(&mut client).map_err(message)?;
                let mut directory =
                    PostgresZoneDirectory::connect(&url, timeout).map_err(message)?;
                let grants = zones
                    .iter()
                    .map(|&zone| acquire(&mut directory, zone, &host_id, grant_ttl))
                    .collect::<Result<Vec<_>, _>>()?;
                let entries = PostgresEntries::connect(&url, timeout).map_err(message)?;
                let handoffs = PostgresHandoffs::connect(&url, timeout).map_err(message)?;
                Ok((directory, grants, entries, handoffs))
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
        let listener = tokio::net::TcpListener::bind(link_address.as_str()).await?;
        let ids = local.iter().map(|zone| zone.match_id.clone()).collect();
        let wiring = HandoffWiring {
            channels,
            listener,
            build: Box::new(move |host, storage| {
                Arc::new(HandoffHost::new(
                    host,
                    storage,
                    Box::new(handoffs),
                    local,
                    routes,
                    timeout,
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
                let renewed = tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let mut guard = renewals.lock().map_err(message)?;
                    let (directory, grants) = &mut *guard;
                    for grant in grants.iter_mut() {
                        *grant = directory.renew(grant, grant_ttl).map_err(message)?;
                    }
                    Ok(())
                })
                .await
                .map_err(message)
                .and_then(|result| result);
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
}
