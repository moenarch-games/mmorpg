//! Host-to-host character transfer (#145 part 2): two zone hosts over the
//! trusted LAN link, the character lease moving through the store, epoch
//! fencing and failure injection at every phase. After each failure exactly
//! one side ends authoritative: the source with its player live again, or
//! the destination holding the lease and, once activated, the live player.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mmorpg_characters::{
    CharacterLease, CharacterStore, EntryBackend, EntryChange, LeaseHolder, StoreError,
};
use mmorpg_control_plane::{
    ControlPlaneError, HandoffPhase, HandoffRegistry, HostId, HostRegistry, ZoneDirectory,
    ZoneLease,
};
use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterTransfer, ClassChoice, PlayerClass, Sex, TransferId,
    ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale,
};
use mmorpg_game_server::handoff::{
    Authority, HandoffError, InboundOutcome, OutboundOutcome, OutboundRequest, Resolution,
    accept_inbound, activate_inbound, begin_outbound, handoff_id, holder, reconcile_inbound,
    reconcile_outbound, resolve_pending_handoff, retire_outbound,
};
use mmorpg_game_server::transfer_link::{
    LinkError, TransferLink, TransferLinkHandler, serve_transfer_link,
};
use tokio::net::TcpListener;

const WEST: ZoneId = ZoneId::new(1);
const EAST: ZoneId = ZoneId::new(2);
/// The source's player and the player ID the destination gives the arrival.
const WALKER: u32 = 1;
const ARRIVAL: u32 = 7;

#[derive(Default)]
struct MemoryEntries(BTreeMap<String, Vec<u8>>);

impl EntryBackend for MemoryEntries {
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.get(id.as_str()).cloned())
    }

    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        let next = change(self.0.get(id.as_str()).map(Vec::as_slice))?;
        self.0.insert(id.as_str().to_owned(), next);
        Ok(())
    }
}

type Store = CharacterStore<MemoryEntries>;

struct ControlPlane {
    hosts: HostRegistry,
    directory: ZoneDirectory,
    handoffs: HandoffRegistry,
    now: u64,
}

impl ControlPlane {
    fn authority(&mut self) -> Authority<'_> {
        Authority {
            directory: &self.directory,
            handoffs: &mut self.handoffs,
            now_tick: self.now,
        }
    }

    fn phase(&self, transfer: TransferId) -> Option<HandoffPhase> {
        self.handoffs
            .record(handoff_id(transfer))
            .map(|record| record.phase)
    }

    /// The zone's host failed: a new host takes over at a newer epoch.
    fn replace(&mut self, lease: &ZoneLease, host: &str) -> ZoneLease {
        self.directory
            .reassign(
                lease.zone_id,
                lease.epoch,
                HostId::new(host).unwrap(),
                &self.hosts,
                self.now,
            )
            .unwrap()
    }
}

struct World {
    control: ControlPlane,
    store: Store,
    west: ZoneSimulation,
    east: ZoneSimulation,
    west_lease: ZoneLease,
    east_lease: ZoneLease,
    character: CharacterLease,
}

fn character() -> CharacterId {
    CharacterId::new("lan-walker").unwrap()
}

fn transfer_id(value: u64) -> TransferId {
    TransferId::new(value).unwrap()
}

fn zone(id: ZoneId) -> ZoneSimulation {
    ZoneSimulation::with_content(id, greyhaven_vale::content()).unwrap()
}

/// West holds the walker live under its committed lease; East is empty.
fn world() -> World {
    let mut hosts = HostRegistry::new(1_000).unwrap();
    for name in ["host-a", "host-b", "host-c"] {
        hosts.register(HostId::new(name).unwrap(), 0).unwrap();
    }
    let mut directory = ZoneDirectory::new(1_000).unwrap();
    let west_lease = directory
        .assign(WEST, HostId::new("host-a").unwrap(), &hosts, 0)
        .unwrap();
    let east_lease = directory
        .assign(EAST, HostId::new("host-b").unwrap(), &hosts, 0)
        .unwrap();
    let mut west = zone(WEST);
    west.create_character(
        WALKER,
        ClassChoice {
            class: PlayerClass::Warden,
            sex: Sex::Female,
        },
    )
    .unwrap();
    west.apply_command(
        WALKER,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 0,
        },
    )
    .unwrap();
    for _ in 0..5 {
        west.advance_tick().unwrap();
    }
    let mut store = CharacterStore::new(MemoryEntries::default());
    let character = store
        .acquire(&self::character(), holder(&west_lease))
        .unwrap()
        .lease;
    store
        .commit(
            &character,
            &west.capture_character(WALKER, self::character()).unwrap(),
        )
        .unwrap();
    World {
        control: ControlPlane {
            hosts,
            directory,
            handoffs: HandoffRegistry::default(),
            now: 1,
        },
        store,
        west,
        east: zone(EAST),
        west_lease,
        east_lease,
        character,
    }
}

fn live(zone: &ZoneSimulation) -> usize {
    zone.snapshot().unwrap().players.len()
}

impl World {
    /// The walker teleports to the same spot of East (same content, other zone).
    fn arrival(&self) -> CharacterLocation {
        let record = self.store.committed(&character()).unwrap().unwrap().record;
        CharacterLocation {
            zone_id: EAST,
            ..record.location
        }
    }

    fn begin(&mut self, transfer: u64) -> Result<CharacterTransfer, HandoffError> {
        let arrival = self.arrival();
        begin_outbound(
            &mut self.west,
            &mut self.store,
            &mut self.control.authority(),
            &OutboundRequest {
                player: WALKER,
                character: &self.character,
                source: &self.west_lease,
                destination: &self.east_lease,
                transfer_id: transfer_id(transfer),
                arrival,
            },
        )
    }

    fn accept(&mut self, transfer: CharacterTransfer) -> Result<CharacterLease, HandoffError> {
        accept_inbound(
            &mut self.east,
            &mut self.store,
            &mut self.control.authority(),
            &self.east_lease,
            transfer,
        )
        .map(|acquired| acquired.lease)
    }

    fn reconcile_west(&mut self, transfer: u64) -> Result<OutboundOutcome, HandoffError> {
        reconcile_outbound(
            &mut self.west,
            &mut self.store,
            &mut self.control.authority(),
            &self.west_lease,
            &character(),
            transfer_id(transfer),
        )
    }

    fn activate(&mut self, transfer: u64) -> Result<(), HandoffError> {
        activate_inbound(
            &mut self.east,
            &self.control.authority(),
            transfer_id(transfer),
            ARRIVAL,
        )
    }

    fn leased_by(&self) -> Option<LeaseHolder> {
        self.store
            .committed(&character())
            .unwrap()
            .unwrap()
            .leased_by
    }

    /// Never two live copies, and never a live copy without the lease.
    fn assert_single_authority(&self) {
        let (west, east) = (live(&self.west), live(&self.east));
        assert!(
            west + east <= 1,
            "two live copies: west {west}, east {east}"
        );
        let leased = self.leased_by();
        if west == 1 {
            assert_eq!(leased.map(|holder| holder.zone_id), Some(WEST));
        }
        if east == 1 {
            assert_eq!(leased.map(|holder| holder.zone_id), Some(EAST));
        }
    }

    /// The source is authoritative again and can keep committing.
    fn assert_source_restored(&mut self) {
        self.assert_single_authority();
        assert_eq!(live(&self.west), 1);
        assert_eq!(self.leased_by(), Some(holder(&self.west_lease)));
        assert_eq!(self.store.pending_handoff(&character()).unwrap(), None);
        let record = self.west.capture_character(WALKER, character()).unwrap();
        self.store.commit(&self.character, &record).unwrap();
    }
}

#[test]
fn a_transfer_moves_the_live_character_and_its_lease_once() {
    let mut world = world();
    let before = world.store.committed(&character()).unwrap().unwrap();
    let transfer = world.begin(1).unwrap();
    // Frozen: no side is live, and the source can no longer write.
    assert_eq!((live(&world.west), live(&world.east)), (0, 0));
    assert_eq!(
        world.store.commit(&world.character, &before.record),
        Err(StoreError::HandoffPending(transfer_id(1)))
    );
    assert_eq!(
        world.store.acquire(&character(), holder(&world.east_lease)),
        Err(StoreError::HandoffPending(transfer_id(1)))
    );

    let lease = world.accept(transfer.clone()).unwrap();
    assert_eq!(
        world.control.phase(transfer_id(1)),
        Some(HandoffPhase::Accepted)
    );
    assert_eq!(lease.holder, holder(&world.east_lease));
    let committed = world.store.committed(&character()).unwrap().unwrap();
    assert_eq!(committed.revision, before.revision + 1);
    assert_eq!(committed.record, transfer.record);
    // The destination waits for the source's retirement before going live.
    assert_eq!(
        world.activate(1),
        Err(HandoffError::NotCommitted(transfer_id(1)))
    );
    // The old lease is fenced at the store.
    assert_eq!(
        world.store.commit(&world.character, &before.record),
        Err(StoreError::StaleLease)
    );

    retire_outbound(
        &mut world.west,
        &mut world.control.authority(),
        &world.west_lease,
        transfer_id(1),
    )
    .unwrap();
    world.activate(1).unwrap();
    assert_eq!((live(&world.west), live(&world.east)), (0, 1));
    world.assert_single_authority();

    // Every step repeats harmlessly: a resent offer returns the same lease.
    assert_eq!(world.accept(transfer).unwrap(), lease);
    world.activate(1).unwrap();
    assert_eq!(world.reconcile_west(1), Ok(OutboundOutcome::Retired));
    assert_eq!((live(&world.west), live(&world.east)), (0, 1));
    // The destination is the only writer now.
    let record = world.east.capture_character(ARRIVAL, character()).unwrap();
    world.store.commit(&lease, &record).unwrap();
}

#[test]
fn an_unreachable_destination_leaves_the_source_authoritative() {
    let mut world = world();
    let transfer = world.begin(2).unwrap();
    assert_eq!(
        world.reconcile_west(2),
        Ok(OutboundOutcome::Restored(Some(WALKER)))
    );
    assert_eq!(
        world.control.phase(transfer_id(2)),
        Some(HandoffPhase::Aborted)
    );
    world.assert_source_restored();
    // A late delivery of the offer is refused and leaves nothing staged.
    assert_eq!(
        world.accept(transfer),
        Err(HandoffError::ControlPlane(
            ControlPlaneError::TransferAborted(handoff_id(transfer_id(2)))
        ))
    );
    assert_eq!(world.east.staged_transfer(transfer_id(2)), None);
    world.assert_source_restored();
}

#[test]
fn a_destination_that_stops_before_accepting_changes_nothing() {
    let mut world = world();
    let transfer = world.begin(3).unwrap();
    world.east.stage_transfer(transfer).unwrap();
    // The destination host stops: its staged copy is gone with it.
    world.east_lease = world.control.replace(&world.east_lease, "host-c");
    world.east = zone(EAST);
    assert_eq!(
        world.reconcile_west(3),
        Ok(OutboundOutcome::Restored(Some(WALKER)))
    );
    world.assert_source_restored();
}

#[test]
fn the_accepting_side_wins_a_race_with_the_sources_abort() {
    let mut world = world();
    let transfer = world.begin(4).unwrap();
    // The destination accepts, but its reply never reaches the source.
    world.accept(transfer).unwrap();
    assert_eq!(world.reconcile_west(4), Ok(OutboundOutcome::Retired));
    assert_eq!(
        world.control.phase(transfer_id(4)),
        Some(HandoffPhase::Committed)
    );
    world.activate(4).unwrap();
    assert_eq!((live(&world.west), live(&world.east)), (0, 1));
    world.assert_single_authority();
}

#[test]
fn a_destination_that_stops_after_accepting_still_ends_up_owning_the_character() {
    let mut world = world();
    let transfer = world.begin(5).unwrap();
    world.east.stage_transfer(transfer.clone()).unwrap();
    world
        .control
        .handoffs
        .accept(
            handoff_id(transfer_id(5)),
            &world.east_lease,
            &world.control.directory,
            world.control.now,
        )
        .unwrap();
    // The destination stops before taking over the lease.
    let old_east = world.east_lease.clone();
    world.east_lease = world.control.replace(&old_east, "host-c");
    world.east = zone(EAST);
    // The source retires regardless; the transfer is decided.
    assert_eq!(world.reconcile_west(5), Ok(OutboundOutcome::Retired));
    assert_eq!(live(&world.west), 0);
    // The new destination host meets the pending handoff when the character
    // returns, completes it from the store's copy and supersedes the lease.
    assert_eq!(
        world.store.acquire(&character(), holder(&world.east_lease)),
        Err(StoreError::HandoffPending(transfer_id(5)))
    );
    let resolved = resolve_pending_handoff(
        &mut world.store,
        &mut world.control.authority(),
        &world.east_lease,
        &character(),
    )
    .unwrap();
    let Resolution::Completed(acquired) = resolved else {
        panic!("expected completion, got {resolved:?}");
    };
    assert_eq!(acquired.lease.holder, holder(&old_east));
    let acquired = world
        .store
        .acquire(&character(), holder(&world.east_lease))
        .unwrap();
    assert_eq!(acquired.record, Some(transfer.record.clone()));
    world
        .east
        .admit_character(ARRIVAL, acquired.record.as_ref().unwrap())
        .unwrap();
    world.assert_single_authority();
}

#[test]
fn a_destination_whose_lease_move_failed_retries_from_the_handoff_record() {
    let mut world = world();
    let transfer = world.begin(6).unwrap();
    world.east.stage_transfer(transfer).unwrap();
    world
        .control
        .handoffs
        .accept(
            handoff_id(transfer_id(6)),
            &world.east_lease,
            &world.control.directory,
            world.control.now,
        )
        .unwrap();
    // The store write after acceptance failed; the staged copy is kept.
    let outcome = reconcile_inbound(
        &mut world.east,
        &mut world.store,
        &mut world.control.authority(),
        &world.east_lease,
        &character(),
        transfer_id(6),
    )
    .unwrap();
    assert!(matches!(outcome, InboundOutcome::Accepted(_)));
    assert_eq!(world.leased_by(), Some(holder(&world.east_lease)));
    retire_outbound(
        &mut world.west,
        &mut world.control.authority(),
        &world.west_lease,
        transfer_id(6),
    )
    .unwrap();
    world.activate(6).unwrap();
    world.assert_single_authority();
    assert_eq!(live(&world.east), 1);
}

#[test]
fn a_source_that_stops_after_acceptance_lets_the_destination_go_live() {
    let mut world = world();
    let transfer = world.begin(7).unwrap();
    world.accept(transfer).unwrap();
    // The source host stops before confirming retirement.
    assert_eq!(
        world.activate(7),
        Err(HandoffError::NotCommitted(transfer_id(7)))
    );
    world.west_lease = world.control.replace(&world.west_lease, "host-c");
    world.west = zone(WEST);
    // Its fenced epoch can neither restore nor write the character; the
    // destination goes live, and the source's successor confirms retirement.
    world.activate(7).unwrap();
    assert_eq!(world.reconcile_west(7), Ok(OutboundOutcome::Retired));
    assert_eq!(
        world.control.phase(transfer_id(7)),
        Some(HandoffPhase::Committed)
    );
    world.assert_single_authority();
    assert_eq!(live(&world.east), 1);
}

#[test]
fn a_source_that_stops_before_acceptance_gets_its_character_back() {
    for prepared in [false, true] {
        let mut world = world();
        let before = world.store.committed(&character()).unwrap().unwrap();
        let transfer = if prepared {
            world.begin(8).unwrap()
        } else {
            // The source stops between recording the frozen transfer and
            // preparing the handoff.
            let transfer = world
                .west
                .freeze_for_transfer(WALKER, character(), transfer_id(8), world.arrival())
                .unwrap();
            world
                .store
                .begin_handoff(&world.character, &transfer, holder(&world.east_lease))
                .unwrap();
            transfer
        };
        // The live source epoch cannot take an undecided handoff back by
        // resolving; only its own reconcile (or a fenced successor) may.
        assert_eq!(
            resolve_pending_handoff(
                &mut world.store,
                &mut world.control.authority(),
                &world.west_lease,
                &character(),
            ),
            Err(HandoffError::Store(StoreError::HandoffPending(
                transfer_id(8)
            )))
        );
        world.west_lease = world.control.replace(&world.west_lease, "host-c");
        world.west = zone(WEST);
        let resolved = resolve_pending_handoff(
            &mut world.store,
            &mut world.control.authority(),
            &world.west_lease,
            &character(),
        )
        .unwrap();
        assert!(matches!(resolved, Resolution::Cancelled(_)), "{resolved:?}");
        if prepared {
            assert_eq!(
                world.control.phase(transfer_id(8)),
                Some(HandoffPhase::Aborted)
            );
        }
        // A late offer can no longer complete it.
        assert!(world.accept(transfer).is_err());
        assert_eq!(live(&world.east), 0);
        // The successor re-admits the last committed record.
        let acquired = world
            .store
            .acquire(&character(), holder(&world.west_lease))
            .unwrap();
        assert_eq!(acquired.record, Some(before.record));
        world
            .west
            .admit_character(WALKER, acquired.record.as_ref().unwrap())
            .unwrap();
        world.assert_single_authority();
    }
}

#[test]
fn a_stale_source_epoch_cannot_start_a_transfer() {
    let mut world = world();
    let stale = world.west_lease.clone();
    world.west_lease = world.control.replace(&stale, "host-c");
    world.west_lease = stale;
    assert!(matches!(
        world.begin(9),
        Err(HandoffError::ControlPlane(
            ControlPlaneError::StaleLease { .. }
        ))
    ));
    assert_eq!(live(&world.west), 1);
    assert_eq!(world.store.pending_handoff(&character()).unwrap(), None);
}

/// A destination host serving the LAN link.
struct EastHost {
    control: Arc<Mutex<ControlPlane>>,
    store: Arc<Mutex<Store>>,
    zone: Mutex<ZoneSimulation>,
    lease: ZoneLease,
}

impl TransferLinkHandler for EastHost {
    fn offer(&self, transfer: CharacterTransfer) -> Result<(), String> {
        let mut control = self.control.lock().unwrap();
        accept_inbound(
            &mut self.zone.lock().unwrap(),
            &mut self.store.lock().unwrap(),
            &mut control.authority(),
            &self.lease,
            transfer,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    fn activate(&self, transfer_id: TransferId) -> Result<(), String> {
        let mut control = self.control.lock().unwrap();
        activate_inbound(
            &mut self.zone.lock().unwrap(),
            &control.authority(),
            transfer_id,
            ARRIVAL,
        )
        .map_err(|error| error.to_string())
    }
}

#[tokio::test]
async fn two_hosts_hand_a_character_over_the_lan_link() {
    let World {
        control,
        store,
        mut west,
        east,
        west_lease,
        east_lease,
        character: lease,
    } = world();
    let control = Arc::new(Mutex::new(control));
    let store = Arc::new(Mutex::new(store));
    let east = Arc::new(EastHost {
        control: Arc::clone(&control),
        store: Arc::clone(&store),
        zone: Mutex::new(east),
        lease: east_lease.clone(),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_transfer_link(listener, east.clone()));
    let mut link = TransferLink::connect(address, Duration::from_secs(5))
        .await
        .unwrap();

    let arrival = {
        let committed = store.lock().unwrap().committed(&character()).unwrap();
        CharacterLocation {
            zone_id: EAST,
            ..committed.unwrap().record.location
        }
    };
    let transfer = begin_outbound(
        &mut west,
        &mut store.lock().unwrap(),
        &mut control.lock().unwrap().authority(),
        &OutboundRequest {
            player: WALKER,
            character: &lease,
            source: &west_lease,
            destination: &east_lease,
            transfer_id: transfer_id(10),
            arrival,
        },
    )
    .unwrap();
    // Going live before the source retired is refused over the link too.
    assert!(matches!(
        link.activate(transfer_id(10)).await,
        Err(LinkError::Refused(_))
    ));
    link.offer(&transfer).await.unwrap();
    link.offer(&transfer).await.unwrap();
    retire_outbound(
        &mut west,
        &mut control.lock().unwrap().authority(),
        &west_lease,
        transfer_id(10),
    )
    .unwrap();
    link.activate(transfer_id(10)).await.unwrap();
    link.activate(transfer_id(10)).await.unwrap();

    assert_eq!(live(&west), 0);
    assert_eq!(live(&east.zone.lock().unwrap()), 1);
    let committed = store
        .lock()
        .unwrap()
        .committed(&character())
        .unwrap()
        .unwrap();
    assert_eq!(committed.leased_by, Some(holder(&east_lease)));
    assert_eq!(committed.record.location.zone_id, EAST);
    // Commands the source already applied stay applied once.
    let mut zone = east.zone.lock().unwrap();
    let stale = zone.apply_command(
        ARRIVAL,
        transfer.cutover_sequence,
        ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing: 0,
        },
    );
    assert!(stale.is_err());
    drop(zone);
    server.abort();
}
