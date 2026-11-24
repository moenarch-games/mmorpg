//! A live [`HandoffHost`] driven in process (#1152): zones as match
//! runtimes, the in-memory reference authority and character store shared by
//! every host, and an in-process transfer link that can rendezvous offers or
//! leave a destination unreachable. No PostgreSQL, no sockets.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use game_server::{ControlContext, MatchId, MatchRuntime, ReconnectToken};
use mmorpg_characters::{CharacterStore, EntryBackend, EntryChange, LeaseHolder, StoreError};
use mmorpg_control_plane::{
    ControlPlaneError, HandoffAuthority, HandoffPhase, HandoffRecord, HandoffRegistry,
    HandoffTicket, HostId, HostRegistry, ReferenceAuthority, ZoneDirectory, ZoneLease,
};
use mmorpg_core::{
    CharacterId, CharacterTransfer, ClassChoice, GREYHAVEN_OUTPOST, PlayerId, SEAM_TEST_EAST,
    SEAM_TEST_WEST, TransferId, WaystoneId, WaystoneSet, ZoneCommand, ZoneId, ZoneSimulation,
    content_for_zone, seam_test,
};
use tokio::sync::{Barrier, Semaphore};

use super::{
    ARRIVAL_CLAIM_TIMEOUT, ARRIVAL_RECHECK, HandoffHost, LocalZone, ZoneInputs, ZoneRoute,
};
use crate::dev_characters::DevCharacterStorage;
use crate::handoff::{handoff_id, holder};
use crate::transfer_link::{LinkError, LinkFailure, TransferLinkHandler, TransferLinks};
use crate::{HandoffChannel, ZoneGameServerAdapter, zone_match_id};

const WEST: ZoneId = seam_test::WEST_ZONE;
const EAST: ZoneId = seam_test::EAST_ZONE;
/// Greyhaven's host never answers in these tests.
const GREYHAVEN: ZoneId = ZoneId::new(1);

/// Character entries every host shares, as they would share the database.
#[derive(Clone, Default)]
struct Entries(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);

impl EntryBackend for Entries {
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.lock().unwrap().get(id.as_str()).cloned())
    }

    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        let mut entries = self.0.lock().unwrap();
        let next = change(entries.get(id.as_str()).map(Vec::as_slice))?;
        entries.insert(id.as_str().to_owned(), next);
        Ok(())
    }
}

struct ControlPlane {
    hosts: HostRegistry,
    directory: ZoneDirectory,
    handoffs: HandoffRegistry,
}

/// The reference authority every host shares; commits can be made to fail.
#[derive(Clone)]
struct Authority {
    control: Arc<Mutex<ControlPlane>>,
    failing_commits: Arc<AtomicU32>,
}

impl Authority {
    fn new() -> Self {
        let mut hosts = HostRegistry::new(1_000).unwrap();
        for name in ["host-a", "host-b", "host-c"] {
            hosts.register(HostId::new(name).unwrap(), 0).unwrap();
        }
        Self {
            control: Arc::new(Mutex::new(ControlPlane {
                hosts,
                directory: ZoneDirectory::new(1_000).unwrap(),
                handoffs: HandoffRegistry::default(),
            })),
            failing_commits: Arc::default(),
        }
    }

    fn assign(&self, zone: ZoneId, host: &str) -> ZoneLease {
        let mut control = self.control.lock().unwrap();
        let ControlPlane {
            hosts, directory, ..
        } = &mut *control;
        directory
            .assign(zone, HostId::new(host).unwrap(), hosts, 0)
            .unwrap()
    }

    fn phase(&self, transfer_id: TransferId) -> Option<HandoffPhase> {
        let control = self.control.lock().unwrap();
        control
            .handoffs
            .record(handoff_id(transfer_id))
            .map(|record| record.phase)
    }

    fn with<R>(&self, body: impl FnOnce(&mut ReferenceAuthority<'_>) -> R) -> R {
        let mut control = self.control.lock().unwrap();
        let ControlPlane {
            directory,
            handoffs,
            ..
        } = &mut *control;
        body(&mut ReferenceAuthority {
            directory,
            handoffs,
            now_tick: 1,
        })
    }
}

impl HandoffAuthority for Authority {
    fn ensure_current(&mut self, lease: &ZoneLease) -> Result<(), ControlPlaneError> {
        self.with(|authority| authority.ensure_current(lease))
    }

    fn current_lease(&mut self, zone_id: ZoneId) -> Result<Option<ZoneLease>, ControlPlaneError> {
        self.with(|authority| authority.current_lease(zone_id))
    }

    fn record(
        &mut self,
        transfer_id: mmorpg_control_plane::TransferId,
    ) -> Result<Option<HandoffRecord>, ControlPlaneError> {
        self.with(|authority| authority.record(transfer_id))
    }

    fn prepare(&mut self, ticket: HandoffTicket) -> Result<HandoffPhase, ControlPlaneError> {
        self.with(|authority| authority.prepare(ticket))
    }

    fn accept(
        &mut self,
        transfer_id: mmorpg_control_plane::TransferId,
        destination: &ZoneLease,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        self.with(|authority| authority.accept(transfer_id, destination))
    }

    fn commit(
        &mut self,
        transfer_id: mmorpg_control_plane::TransferId,
        source: &ZoneLease,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        let fail = self
            .failing_commits
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        if fail {
            return Err(ControlPlaneError::Backend("the commit timed out".into()));
        }
        self.with(|authority| authority.commit(transfer_id, source))
    }

    fn abort(
        &mut self,
        transfer_id: mmorpg_control_plane::TransferId,
        source: &ZoneLease,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        self.with(|authority| authority.abort(transfer_id, source))
    }
}

/// One host's zones as match runtimes, served as soon as they exist.
#[derive(Clone, Default)]
struct Zones(Arc<Mutex<BTreeMap<MatchId, MatchRuntime<ZoneGameServerAdapter>>>>);

impl ZoneInputs for Zones {
    fn serving(&self) -> bool {
        true
    }

    fn submit(
        &self,
        id: &MatchId,
        player: PlayerId,
        _connection_epoch: u32,
        payload: &[u8],
    ) -> impl Future<Output = Result<(), String>> + Send {
        let mut zones = self.0.lock().unwrap();
        let result = zones.get_mut(id).map_or_else(
            || Err("no such zone".to_owned()),
            |runtime| {
                runtime
                    .submit_host_input(player, payload)
                    .map_err(|error| error.to_string())
            },
        );
        std::future::ready(result)
    }
}

enum Peer {
    Up(Weak<dyn TransferLinkHandler>),
    /// Connects hang until the gate closes, then fail.
    Down(Arc<Semaphore>),
}

#[derive(Default)]
struct LinkState {
    peers: Mutex<BTreeMap<String, Peer>>,
    /// Connects attempted per link address.
    connects: Mutex<BTreeMap<String, u32>>,
    /// Every offer that reached a destination: (address, transfer).
    offers: Mutex<Vec<(String, TransferId)>>,
    /// Offers wait here for each other before they reach their destination.
    rendezvous: Mutex<Option<Arc<Barrier>>>,
}

/// The transfer link, in process.
#[derive(Clone, Default)]
struct Links(Arc<LinkState>);

impl Links {
    fn connects(&self, address: &str) -> u32 {
        self.0
            .connects
            .lock()
            .unwrap()
            .get(address)
            .copied()
            .unwrap_or(0)
    }

    fn offered_to(&self, address: &str) -> Vec<TransferId> {
        self.0
            .offers
            .lock()
            .unwrap()
            .iter()
            .filter(|(to, _)| to == address)
            .map(|&(_, transfer_id)| transfer_id)
            .collect()
    }

    /// Connects the peer at `address`: its handler, or a gate it hangs on.
    async fn connect(
        &self,
        address: &str,
        timeout: Duration,
    ) -> Result<Arc<dyn TransferLinkHandler>, LinkFailure> {
        *self
            .0
            .connects
            .lock()
            .unwrap()
            .entry(address.to_owned())
            .or_default() += 1;
        let peer = match self.0.peers.lock().unwrap().get(address) {
            Some(Peer::Up(handler)) => Ok(handler.upgrade()),
            Some(Peer::Down(gate)) => Err(Arc::clone(gate)),
            None => Ok(None),
        };
        match peer {
            Ok(Some(handler)) => Ok(handler),
            Ok(None) => Err(LinkFailure::Unreachable(LinkError::Io(
                "connection refused".into(),
            ))),
            Err(gate) => {
                let _ = tokio::time::timeout(timeout, gate.acquire()).await;
                Err(LinkFailure::Unreachable(LinkError::TimedOut))
            }
        }
    }

    async fn request(
        handler: Arc<dyn TransferLinkHandler>,
        timeout: Duration,
        request: impl FnOnce(&dyn TransferLinkHandler) -> Result<(), String> + Send + 'static,
    ) -> Result<(), LinkFailure> {
        let reply = tokio::task::spawn_blocking(move || request(handler.as_ref()));
        match tokio::time::timeout(timeout, reply).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(reason))) => Err(LinkFailure::Request(LinkError::Refused(reason))),
            Ok(Err(error)) => Err(LinkFailure::Request(LinkError::Io(error.to_string()))),
            Err(_) => Err(LinkFailure::Request(LinkError::TimedOut)),
        }
    }
}

impl TransferLinks for Links {
    async fn offer(
        &self,
        address: &str,
        transfer: &CharacterTransfer,
        timeout: Duration,
    ) -> Result<(), LinkFailure> {
        let handler = self.connect(address, timeout).await?;
        let rendezvous = self.0.rendezvous.lock().unwrap().clone();
        if let Some(rendezvous) = rendezvous {
            rendezvous.wait().await;
        }
        self.0
            .offers
            .lock()
            .unwrap()
            .push((address.to_owned(), transfer.transfer_id));
        let transfer = transfer.clone();
        Self::request(handler, timeout, move |handler| handler.offer(transfer)).await
    }

    async fn activate(
        &self,
        address: &str,
        transfer_id: TransferId,
        timeout: Duration,
    ) -> Result<(), LinkFailure> {
        let handler = self.connect(address, timeout).await?;
        Self::request(handler, timeout, move |handler| {
            handler.activate(transfer_id)
        })
        .await
    }
}

type Host = HandoffHost<Entries, Zones, Links>;

/// Hosts that share one authority, one store and one link.
struct World {
    runtime: Option<tokio::runtime::Runtime>,
    authority: Authority,
    entries: Entries,
    links: Links,
    routes: BTreeMap<ZoneId, ZoneRoute>,
    leases: BTreeMap<ZoneId, ZoneLease>,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// A claimed character on its way to another zone.
struct Traveller {
    character: CharacterId,
    player: PlayerId,
}

/// One host and the zones it runs.
struct Hosted {
    host: Arc<Host>,
    zones: Zones,
    next_token: u64,
}

/// The hosts' transfer links. Greyhaven's sorts first, so departures that ran
/// one after another in any stable order would wait on it before East's.
const WEST_LINK: &str = "seam-west:link";
const EAST_LINK: &str = "seam-east:link";
const GREYHAVEN_LINK: &str = "greyhaven:link";

impl World {
    /// West on host A, East on host B, Greyhaven on host C (never reachable).
    fn new(link_timeout: Duration) -> (Self, Hosted, Hosted) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let authority = Authority::new();
        let mut leases = BTreeMap::new();
        let mut routes = BTreeMap::new();
        for (zone, host, link) in [
            (WEST, "host-a", WEST_LINK),
            (EAST, "host-b", EAST_LINK),
            (GREYHAVEN, "host-c", GREYHAVEN_LINK),
        ] {
            leases.insert(zone, authority.assign(zone, host));
            routes.insert(
                zone,
                ZoneRoute {
                    link: link.to_owned(),
                    session: format!("{host}:session"),
                },
            );
        }
        let mut world = Self {
            runtime: Some(runtime),
            authority,
            entries: Entries::default(),
            links: Links::default(),
            routes,
            leases,
        };
        let a = world.host(WEST, link_timeout);
        let b = world.host(EAST, link_timeout);
        (world, a, b)
    }

    fn host(&mut self, zone: ZoneId, link_timeout: Duration) -> Hosted {
        let lease = self.leases[&zone].clone();
        let match_id = zone_match_id(zone).unwrap();
        let channel = HandoffChannel::default();
        let mut adapter =
            ZoneGameServerAdapter::with_content(zone, content_for_zone(zone)).unwrap();
        adapter.require_durable_character_claims();
        adapter.attach_handoffs(channel.clone());
        let zones = Zones::default();
        zones
            .0
            .lock()
            .unwrap()
            .insert(match_id.clone(), MatchRuntime::new(adapter, 30));
        let storage = Arc::new(Mutex::new(DevCharacterStorage::shared(
            CharacterStore::new(self.entries.clone()),
            BTreeMap::from([(zone, lease.epoch)]),
        )));
        let routes = self
            .routes
            .iter()
            .filter(|&(&other, _)| other != zone)
            .map(|(&other, route)| (other, route.clone()))
            .collect();
        let _entered = self.runtime.as_ref().unwrap().enter();
        let host = Arc::new(HandoffHost::new(
            zones.clone(),
            self.links.clone(),
            storage,
            Box::new(self.authority.clone()),
            vec![LocalZone {
                match_id,
                lease,
                channel,
            }],
            routes,
            link_timeout,
            None,
        ));
        let handler: Arc<dyn TransferLinkHandler> = host.clone();
        self.links.0.peers.lock().unwrap().insert(
            self.routes[&zone].link.clone(),
            Peer::Up(Arc::downgrade(&handler)),
        );
        Hosted {
            host,
            zones,
            next_token: 0,
        }
    }

    /// Greyhaven's host hangs on connects until the returned gate is closed.
    fn greyhaven_hangs(&self) -> Arc<Semaphore> {
        let gate = Arc::new(Semaphore::new(0));
        self.links
            .0
            .peers
            .lock()
            .unwrap()
            .insert(GREYHAVEN_LINK.to_owned(), Peer::Down(Arc::clone(&gate)));
        gate
    }

    fn leased_by(&self, character: &CharacterId) -> Option<LeaseHolder> {
        CharacterStore::new(self.entries.clone())
            .committed(character)
            .unwrap()
            .unwrap()
            .leased_by
    }

    /// Waits for `done`, failing after a generous bound so a regression
    /// fails instead of hanging.
    fn wait_for(&self, what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Hosted {
    fn zone(&self) -> &LocalZone {
        self.host.zones.values().next().unwrap()
    }

    fn match_id(&self) -> MatchId {
        self.zone().match_id.clone()
    }

    /// Claims a fresh character in this host's zone, beside its waystone,
    /// and sends it to `stone`; it is due to leave after the next ticks.
    fn traveller(&mut self, world: &World, name: &str, stone: WaystoneId) -> Traveller {
        let zone_id = self.zone().lease.zone_id;
        let lease = &world.leases[&zone_id];
        let match_id = self.match_id();
        self.next_token += 1;
        let mut token = [0; 16];
        token[..8].copy_from_slice(&self.next_token.to_be_bytes());
        let mut zones = self.zones.0.lock().unwrap();
        let runtime = zones.get_mut(&match_id).unwrap();
        let session = runtime.admit(ReconnectToken(token)).unwrap();
        let player = session.player_id;

        let content = content_for_zone(zone_id);
        let mut scratch = ZoneSimulation::with_content(zone_id, Arc::clone(&content)).unwrap();
        scratch
            .create_character(player, ClassChoice::from_appearance(4).unwrap())
            .unwrap();
        let mut state = scratch.snapshot().unwrap();
        let body = state
            .players
            .iter_mut()
            .find(|body| body.player_id == player)
            .unwrap();
        body.position = [
            seam_test::STONE[0] as f32,
            90.0,
            seam_test::STONE[1] as f32 + 300.0,
        ];
        let scratch = ZoneSimulation::from_snapshot(state, content).unwrap();
        let character = CharacterId::new(name).unwrap();
        let mut record = scratch
            .capture_character(player, character.clone())
            .unwrap();
        record.waystones =
            WaystoneSet::from_ids([GREYHAVEN_OUTPOST, SEAM_TEST_WEST, SEAM_TEST_EAST]).unwrap();

        let mut store = CharacterStore::new(world.entries.clone());
        let acquired = store.acquire(&character, holder(lease)).unwrap();
        store.commit(&acquired.lease, &record).unwrap();
        runtime
            .submit_host_input(
                player,
                &ZoneGameServerAdapter::character_admission_input(&record).unwrap(),
            )
            .unwrap();
        let travel = mmorpg_protocol::encode_command(ZoneCommand::UseWaystone { waystone: stone });
        runtime
            .submit_command(player, session.connection_epoch, 1, &travel)
            .unwrap();
        drop(zones);
        self.host.track_session(
            &match_id,
            ControlContext {
                player_id: player,
                connection_epoch: session.connection_epoch,
            },
        );
        self.host
            .storage
            .lock()
            .unwrap()
            .leases
            .insert((match_id, player), acquired.lease);
        Traveller { character, player }
    }

    fn tick(&self, ticks: usize) {
        let mut zones = self.zones.0.lock().unwrap();
        for runtime in zones.values_mut() {
            for _ in 0..ticks {
                runtime.advance_tick().unwrap();
            }
        }
    }

    /// Players due to leave as the zone last published them.
    fn due(&self) -> BTreeSet<PlayerId> {
        self.zone()
            .channel
            .departures()
            .iter()
            .map(|due| due.player)
            .collect()
    }

    fn poll(&self) {
        self.host.process_departures();
    }
}

/// #1096: the destination accepted, but the source's commit failed. The
/// destination owns the character whatever happens; the frozen session waits
/// and the next poll confirms the retirement, which lets it go.
#[test]
fn a_retirement_whose_commit_failed_after_the_offer_is_retried_on_the_next_poll() {
    let (world, mut a, b) = World::new(Duration::from_secs(10));
    let walker = a.traveller(&world, "walker", SEAM_TEST_EAST);
    a.tick(3);
    assert_eq!(a.due(), BTreeSet::from([walker.player]));

    world.authority.failing_commits.store(1, Ordering::SeqCst);
    a.poll();
    let [transfer_id] = world.links.offered_to(EAST_LINK)[..] else {
        panic!("one offer to East");
    };
    assert_eq!(
        world.authority.phase(transfer_id),
        Some(HandoffPhase::Accepted)
    );
    assert_eq!(
        world.leased_by(&walker.character),
        Some(holder(&world.leases[&EAST])),
        "the destination took the character's lease at its acceptance"
    );
    assert!(a.host.unretired.lock().unwrap().contains_key(&transfer_id));
    assert!(
        a.zone().channel.frozen(transfer_id).is_some(),
        "the session stays frozen until the retirement is confirmed"
    );
    assert!(
        b.host.activate(transfer_id).is_err(),
        "the destination does not go live before the source retired"
    );

    a.poll();
    assert_eq!(
        world.authority.phase(transfer_id),
        Some(HandoffPhase::Committed)
    );
    assert!(a.host.unretired.lock().unwrap().is_empty());
    assert!(a.zone().channel.frozen(transfer_id).is_none(), "retired");
    b.host.activate(transfer_id).unwrap();
    assert_eq!(
        world.links.offered_to(EAST_LINK),
        [transfer_id],
        "the retry commits; it never offers again"
    );
}

/// #1098: two hosts offer to each other at the same moment. Neither holds a
/// host-wide guard across its offer, so each one's link handler runs while
/// its own offer waits, and both characters move exactly once.
#[test]
fn two_hosts_offering_to_each_other_at_once_both_hand_over() {
    let (world, mut a, mut b) = World::new(Duration::from_secs(10));
    let walker = a.traveller(&world, "west-walker", SEAM_TEST_EAST);
    let rider = b.traveller(&world, "east-rider", SEAM_TEST_WEST);
    a.tick(3);
    b.tick(3);
    assert_eq!(a.due().len(), 1);
    assert_eq!(b.due().len(), 1);
    *world.links.0.rendezvous.lock().unwrap() = Some(Arc::new(Barrier::new(2)));

    std::thread::scope(|scope| {
        scope.spawn(|| a.poll());
        scope.spawn(|| b.poll());
    });

    let [east_bound] = world.links.offered_to(EAST_LINK)[..] else {
        panic!("one offer to East");
    };
    let [west_bound] = world.links.offered_to(WEST_LINK)[..] else {
        panic!("one offer to West");
    };
    for transfer_id in [east_bound, west_bound] {
        assert_eq!(
            world.authority.phase(transfer_id),
            Some(HandoffPhase::Committed)
        );
    }
    assert_eq!(
        world.leased_by(&walker.character),
        Some(holder(&world.leases[&EAST]))
    );
    assert_eq!(
        world.leased_by(&rider.character),
        Some(holder(&world.leases[&WEST]))
    );
    assert!(a.zone().channel.frozen(east_bound).is_none());
    assert!(b.zone().channel.frozen(west_bound).is_none());
    let arrivals = |hosted: &Hosted| -> Vec<_> {
        hosted
            .host
            .inbound
            .lock()
            .unwrap()
            .values()
            .filter(|inbound| inbound.lease.is_some())
            .map(|inbound| inbound.transfer.record.character_id.clone())
            .collect()
    };
    assert_eq!(arrivals(&b), [walker.character]);
    assert_eq!(arrivals(&a), [rider.character]);
}

/// Departures toward East and toward an unreachable Greyhaven host run side
/// by side: East's completes while Greyhaven's connect still hangs, and
/// Greyhaven is tried once a poll however many players wait for it, then
/// backs off.
#[test]
fn an_unreachable_destination_delays_only_its_own_departures_once_a_poll() {
    let (world, mut a, _b) = World::new(Duration::from_secs(30));
    let gate = world.greyhaven_hangs();
    let stuck: Vec<_> = (0..3)
        .map(|index| a.traveller(&world, &format!("stuck-{index}"), GREYHAVEN_OUTPOST))
        .collect();
    let walker = a.traveller(&world, "walker", SEAM_TEST_EAST);
    a.tick(3);
    assert_eq!(a.due().len(), 4, "every traveller is due");
    assert!(
        stuck.iter().all(|stuck| stuck.player < walker.player),
        "in player order, Greyhaven's travellers come first"
    );

    std::thread::scope(|scope| {
        let polling = scope.spawn(|| a.poll());
        world.wait_for("the departure to East", || {
            world
                .links
                .offered_to(EAST_LINK)
                .first()
                .is_some_and(|&transfer_id| {
                    world.authority.phase(transfer_id) == Some(HandoffPhase::Committed)
                })
        });
        assert!(!polling.is_finished(), "Greyhaven's connect still hangs");
        assert_eq!(world.links.connects(GREYHAVEN_LINK), 1);
        gate.close();
    });
    assert_eq!(
        world.leased_by(&walker.character),
        Some(holder(&world.leases[&EAST]))
    );
    assert_eq!(
        world.links.connects(GREYHAVEN_LINK),
        1,
        "one failed connect ends Greyhaven's departures for the poll"
    );
    // The first was taken back and is held back; the others wait, live.
    let waiting: BTreeSet<_> = stuck[1..].iter().map(|stuck| stuck.player).collect();
    assert_eq!(a.due(), waiting);
    for stuck in &stuck {
        assert_eq!(
            world.leased_by(&stuck.character),
            Some(holder(&world.leases[&WEST])),
            "the source still owns its characters"
        );
    }

    // Once a poll, then backing off: polls 2 and 4 try, 3 waits.
    let mut connects = Vec::new();
    for _ in 0..3 {
        a.poll();
        connects.push(world.links.connects(GREYHAVEN_LINK));
    }
    assert_eq!(connects, [2, 2, 3]);
    assert!(a.due().is_empty(), "each attempt took one traveller back");
    for stuck in &stuck {
        assert_eq!(
            world.leased_by(&stuck.character),
            Some(holder(&world.leases[&WEST]))
        );
    }
}

/// #1146: an accepted arrival whose client never claims it is given up after
/// [`ARRIVAL_CLAIM_TIMEOUT`], once its source retired: the destination
/// releases the character's lease, so the character logs in afresh in the
/// destination zone, and a resent offer adds no arrival.
#[test]
fn an_unclaimed_arrival_releases_its_lease_once_its_source_retired() {
    let (world, mut a, b) = World::new(Duration::from_secs(10));
    let walker = a.traveller(&world, "walker", SEAM_TEST_EAST);
    a.tick(3);
    world.authority.failing_commits.store(1, Ordering::SeqCst);
    a.poll();
    let [transfer_id] = world.links.offered_to(EAST_LINK)[..] else {
        panic!("one offer to East");
    };
    let arrivals = || b.host.inbound.lock().unwrap().len();
    assert_eq!(arrivals(), 1);
    let offered = b.host.inbound.lock().unwrap()[&transfer_id]
        .transfer
        .clone();
    let east = holder(&world.leases[&EAST]);

    // Past its deadline, but the source has not retired: it waits.
    let late = Instant::now() + ARRIVAL_CLAIM_TIMEOUT;
    b.host.expire_arrivals(late);
    assert_eq!(arrivals(), 1, "the source may still hold the frozen copy");
    assert_eq!(world.leased_by(&walker.character), Some(east));

    a.poll();
    assert_eq!(
        world.authority.phase(transfer_id),
        Some(HandoffPhase::Committed)
    );
    b.host.expire_arrivals(late + ARRIVAL_RECHECK);
    assert_eq!(arrivals(), 0);
    assert_eq!(world.leased_by(&walker.character), None, "released");
    let record = CharacterStore::new(world.entries.clone())
        .committed(&walker.character)
        .unwrap()
        .unwrap()
        .record;
    assert_eq!(record.location.zone_id, EAST, "it stands where it arrived");

    // A resent offer of the settled transfer stages nothing again.
    assert!(b.host.offer(offered).is_err());
    assert_eq!(arrivals(), 0);
    assert_eq!(
        world.authority.phase(transfer_id),
        Some(HandoffPhase::Committed),
        "the record still answers within its retention"
    );
    CharacterStore::new(world.entries.clone())
        .acquire(&walker.character, east)
        .expect("the character logs in again at its destination");
}
