#![cfg(feature = "postgres")]
use mmorpg_control_plane::{
    ControlPlaneError, EntityId, HandoffAuthority, HandoffPhase, HandoffTicket, HostId, TransferId,
    postgres::{
        DirectoryStoreError, PostgresHandoffs, PostgresZoneDirectory, ensure_writer_in, migrate,
    },
};
use mmorpg_core::ZoneId;
use std::{
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};

/// The durable handoff records follow the reference model's rules (#145):
/// fenced by both grants, idempotent per transfer, one unsettled transfer
/// per entity, `Accepted` as the decision point and a newer source epoch
/// speaking for a fenced one.
#[test]
fn postgres_handoffs_are_fenced_idempotent_and_decided_at_acceptance() {
    let Ok(url) = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL") else {
        return;
    };
    let mut client = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    migrate(&mut client).unwrap();
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
    let ttl = Duration::from_secs(30);
    let timeout = Duration::from_secs(5);
    let mut directory = PostgresZoneDirectory::connect(&url, timeout).unwrap();
    let west = directory
        .acquire(zone(0), HostId::new("west").unwrap(), 0, ttl)
        .unwrap();
    let east = directory
        .acquire(zone(1), HostId::new("east").unwrap(), 0, ttl)
        .unwrap();
    let (west_lease, east_lease) = (west.lease().clone(), east.lease().clone());
    let mut handoffs = PostgresHandoffs::connect(&url, timeout).unwrap();
    handoffs.ensure_current(&west_lease).unwrap();
    assert_eq!(
        handoffs.current_lease(zone(1)).unwrap(),
        Some(east_lease.clone())
    );

    let transfer = TransferId::new(u64::MAX - u64::try_from(first).unwrap());
    let entity = EntityId::new(u64::MAX - 7 - u64::try_from(first).unwrap());
    let ticket = HandoffTicket {
        transfer_id: transfer,
        entity_id: entity,
        source: west_lease.clone(),
        destination: east_lease.clone(),
    };
    assert_eq!(
        handoffs.prepare(ticket.clone()).unwrap(),
        HandoffPhase::Prepared
    );
    assert_eq!(
        handoffs.prepare(ticket.clone()).unwrap(),
        HandoffPhase::Prepared
    );
    let other = HandoffTicket {
        transfer_id: TransferId::new(transfer.get() - 1),
        ..ticket.clone()
    };
    assert_eq!(
        handoffs.prepare(other.clone()),
        Err(ControlPlaneError::EntityTransferInProgress(entity))
    );
    assert_eq!(
        handoffs.prepare(HandoffTicket {
            destination: west_lease.clone(),
            source: east_lease.clone(),
            ..ticket.clone()
        }),
        Err(ControlPlaneError::TransferIdCollision(transfer))
    );
    assert_eq!(
        handoffs.commit(transfer, &west_lease),
        Err(ControlPlaneError::TransferNotAccepted(transfer))
    );
    assert_eq!(
        handoffs.accept(transfer, &west_lease),
        Err(ControlPlaneError::TransferLeaseMismatch(transfer))
    );
    assert_eq!(
        handoffs.accept(transfer, &east_lease).unwrap(),
        HandoffPhase::Accepted
    );
    assert_eq!(
        handoffs.accept(transfer, &east_lease).unwrap(),
        HandoffPhase::Accepted
    );
    assert_eq!(
        handoffs.abort(transfer, &west_lease),
        Err(ControlPlaneError::TransferAlreadyAccepted(transfer))
    );
    let record = handoffs.record(transfer).unwrap().unwrap();
    assert_eq!(record.phase, HandoffPhase::Accepted);
    assert!(record.ticket.source.same_authority(&west_lease));

    // The source host stops; a newer epoch of its zone commits for it.
    directory.retire(&west_lease).unwrap();
    assert!(handoffs.ensure_current(&west_lease).is_err());
    let successor = directory
        .acquire(
            zone(0),
            HostId::new("west-2").unwrap(),
            west_lease.epoch,
            ttl,
        )
        .unwrap();
    assert_eq!(
        handoffs.commit(transfer, &west_lease),
        Err(ControlPlaneError::StaleLease {
            zone_id: zone(0),
            expected_epoch: west_lease.epoch,
            actual_epoch: successor.lease().epoch,
        })
    );
    assert_eq!(
        handoffs.commit(transfer, successor.lease()).unwrap(),
        HandoffPhase::Committed
    );
    assert_eq!(
        handoffs.commit(transfer, successor.lease()).unwrap(),
        HandoffPhase::Committed
    );

    // Settled, the entity may transfer again; an unaccepted one aborts.
    let again = HandoffTicket {
        source: successor.lease().clone(),
        ..other
    };
    assert_eq!(
        handoffs.prepare(again.clone()).unwrap(),
        HandoffPhase::Prepared
    );
    assert_eq!(
        handoffs
            .abort(again.transfer_id, successor.lease())
            .unwrap(),
        HandoffPhase::Aborted
    );
    assert_eq!(
        handoffs.accept(again.transfer_id, &east_lease),
        Err(ControlPlaneError::TransferAborted(again.transfer_id))
    );
    assert_eq!(
        handoffs
            .prepare(HandoffTicket {
                transfer_id: TransferId::new(transfer.get() - 2),
                ..again
            })
            .unwrap(),
        HandoffPhase::Prepared
    );
}

#[test]
fn postgres_directory_persists_owner_expiry_and_epoch_without_resurrecting_writers() {
    let Ok(url) = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL") else {
        return;
    };
    let mut client = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    migrate(&mut client).unwrap();
    migrate(&mut client).unwrap();
    client
        .execute("SELECT pg_advisory_lock($1)", &[&0x4d4d_5a44_5445_5354_i64])
        .unwrap();
    let zone: i64 = client
        .query_one(
            "SELECT coalesce(max(zone_id),1000)+1 FROM mmorpg_coordination.zones",
            &[],
        )
        .unwrap()
        .get(0);
    let zone = ZoneId::new(zone.try_into().unwrap());
    let timeout = Duration::from_secs(5);
    let mut directory = PostgresZoneDirectory::connect(&url, timeout).unwrap();
    let first = directory
        .acquire(
            zone,
            HostId::new("first-process").unwrap(),
            0,
            Duration::from_secs(30),
        )
        .unwrap();
    assert_eq!(first.lease().epoch, 1);
    // The returned capability is explicitly unconfirmed, not a live permit.
    assert!(first.requested_at() <= Instant::now());
    assert_eq!(first.valid_for(), Duration::from_secs(30));
    assert!(matches!(
        directory.acquire(
            zone,
            HostId::new("racing-process").unwrap(),
            1,
            Duration::from_secs(30)
        ),
        Err(DirectoryStoreError::Conflict)
    ));
    let renewed = directory.renew(&first, Duration::from_secs(40)).unwrap();
    assert!(first.lease().same_authority(renewed.lease()));
    assert!(renewed.lease().expires_at_tick >= first.lease().expires_at_tick);
    drop(directory);
    let mut restarted = PostgresZoneDirectory::connect(&url, timeout).unwrap();
    assert_eq!(
        restarted.assignment(zone).unwrap().unwrap().lease,
        *renewed.lease()
    );
    // Simulate database expiry independently of the process-local test clock.
    client.execute("UPDATE mmorpg_coordination.zones SET expires_at=clock_timestamp()-interval '1 second' WHERE zone_id=$1", &[&i64::from(zone.get())]).unwrap();
    assert!(matches!(
        restarted.renew(&renewed, Duration::from_secs(30)),
        Err(DirectoryStoreError::Expired)
    ));
    assert!(matches!(
        restarted.acquire(
            zone,
            HostId::new("after-expiry").unwrap(),
            1,
            Duration::from_secs(30)
        ),
        Err(DirectoryStoreError::Conflict)
    ));
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        ensure_writer_in(&mut transaction, renewed.lease()),
        Err(DirectoryStoreError::Expired)
    ));
    transaction.rollback().unwrap();
    // Acknowledged source retirement permits exactly one epoch-advancing winner.
    restarted.retire(renewed.lease()).unwrap();
    restarted.retire(renewed.lease()).unwrap();
    let gate = Arc::new(Barrier::new(2));
    let attempts: Vec<_> = (0..2)
        .map(|index| {
            let url = url.clone();
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                let mut directory = PostgresZoneDirectory::connect(&url, timeout).unwrap();
                gate.wait();
                directory.acquire(
                    zone,
                    HostId::new(format!("successor-{index}")).unwrap(),
                    1,
                    Duration::from_secs(30),
                )
            })
        })
        .collect();
    let grants: Vec<_> = attempts
        .into_iter()
        .filter_map(|thread| thread.join().unwrap().ok())
        .collect();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].lease().epoch, 2);
    assert!(matches!(
        restarted.renew(&first, Duration::from_secs(30)),
        Err(DirectoryStoreError::Expired)
    ));
    assert!(matches!(
        restarted.retire(first.lease()),
        Err(DirectoryStoreError::Conflict)
    ));
    let mut transaction = client.transaction().unwrap();
    assert!(ensure_writer_in(&mut transaction, first.lease()).is_err());
    transaction.rollback().unwrap();
    let mut transaction = client.transaction().unwrap();
    transaction
        .batch_execute("SET LOCAL ROLE mmorpg_zone_fence_reader")
        .unwrap();
    ensure_writer_in(&mut transaction, grants[0].lease()).unwrap();
    transaction.commit().unwrap();
    let mut transaction = client.transaction().unwrap();
    transaction
        .batch_execute("SET LOCAL ROLE mmorpg_zone_fence_reader")
        .unwrap();
    assert!(
        transaction
            .execute(
                "UPDATE mmorpg_coordination.zones SET epoch=epoch+1 WHERE zone_id=$1",
                &[&i64::from(zone.get())]
            )
            .is_err()
    );
    transaction.rollback().unwrap();
}
