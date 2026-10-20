#![cfg(feature = "postgres")]
use mmorpg_control_plane::{
    HostId,
    postgres::{DirectoryStoreError, PostgresZoneDirectory, ensure_writer_in, migrate},
};
use mmorpg_core::ZoneId;
use std::{
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};

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
