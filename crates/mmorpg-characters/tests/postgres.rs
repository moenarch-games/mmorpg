//! The PostgreSQL backend against a real server. Runs only with the
//! `postgres` feature and `MMORPG_CHARACTERS_POSTGRES_URL` set to a
//! disposable database (it is migrated and its entries are rewritten), e.g.
//! `docker run --rm -e POSTGRES_PASSWORD=pw -p 5432:5432 postgres:17` and
//! `postgres://postgres:pw@localhost/postgres`.
#![cfg(feature = "postgres")]

use std::sync::{Arc, Barrier};
use std::time::Duration;

use mmorpg_characters::postgres::migrate;
use mmorpg_characters::{CharacterStore, LeaseHolder, PostgresEntries, StoreError};
use mmorpg_core::{
    CharacterId, ClassChoice, PlayerClass, Sex, ZoneId, ZoneSimulation, greyhaven_vale,
};

const TIMEOUT: Duration = Duration::from_secs(5);

fn url() -> Option<String> {
    let url = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL").ok()?;
    let mut client = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    migrate(&mut client).unwrap();
    // Twice: an applied migration is skipped.
    migrate(&mut client).unwrap();
    Some(url)
}

fn holder(zone: u32, epoch: u64) -> LeaseHolder {
    LeaseHolder {
        zone_id: ZoneId::new(zone),
        zone_epoch: epoch,
    }
}

/// A fresh character ID per run, so reruns against one database start clean.
fn fresh(name: &str) -> CharacterId {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    CharacterId::new(format!("{name}-{nanos}")).unwrap()
}

fn record(character: &CharacterId) -> mmorpg_core::CharacterRecord {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.create_character(
        7,
        ClassChoice {
            class: PlayerClass::Warden,
            sex: Sex::Male,
        },
    )
    .unwrap();
    zone.capture_character(7, character.clone()).unwrap()
}

#[test]
fn leases_commits_and_fencing_hold_in_postgres() {
    let Some(url) = url() else { return };
    let mut store = CharacterStore::new(PostgresEntries::connect(&url, TIMEOUT).unwrap());
    let character = fresh("pg-lease");
    let first = store.acquire(&character, holder(1, 5)).unwrap();
    assert_eq!((first.revision, first.record.clone()), (0, None));
    assert_eq!(
        store.acquire(&character, holder(2, 9)),
        Err(StoreError::Leased(holder(1, 5)))
    );
    let saved = record(&character);
    assert_eq!(store.commit(&first.lease, &saved).unwrap(), 1);

    // Another host reads the committed record without the zone.
    let other = CharacterStore::new(PostgresEntries::connect(&url, TIMEOUT).unwrap());
    let committed = other.committed(&character).unwrap().unwrap();
    assert_eq!(
        (committed.record, committed.leased_by),
        (saved.clone(), Some(holder(1, 5)))
    );

    // A restarted epoch of the same zone fences its predecessor.
    let current = store.acquire(&character, holder(1, 6)).unwrap();
    assert_eq!(
        store.commit(&first.lease, &saved),
        Err(StoreError::StaleLease)
    );
    store.release(&current.lease, None).unwrap();
    assert_eq!(
        store.acquire(&character, holder(1, 5)),
        Err(StoreError::FencedEpoch(holder(1, 6)))
    );
}

#[test]
fn racing_hosts_never_both_acquire_in_postgres() {
    let Some(url) = url() else { return };
    let character = fresh("pg-race");
    let barrier = Arc::new(Barrier::new(8));
    let winners: usize = (0..8_u32)
        .map(|zone| {
            let (url, character, barrier) = (url.clone(), character.clone(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                let mut store =
                    CharacterStore::new(PostgresEntries::connect(&url, TIMEOUT).unwrap());
                barrier.wait();
                usize::from(store.acquire(&character, holder(zone + 1, 1)).is_ok())
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .sum();
    assert_eq!(winners, 1);
}

#[test]
fn corrupt_rows_fail_closed() {
    let Some(url) = url() else { return };
    let mut client = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    let character = fresh("pg-corrupt");
    client
        .execute(
            "INSERT INTO mmorpg_characters.entries (character_id, entry) VALUES ($1, $2)",
            &[&character.to_string(), &b"MMCH\x09".to_vec()],
        )
        .unwrap();
    let mut store = CharacterStore::new(PostgresEntries::connect(&url, TIMEOUT).unwrap());
    assert!(matches!(
        store.committed(&character),
        Err(StoreError::Corrupt(_))
    ));
    assert!(matches!(
        store.acquire(&character, holder(1, 1)),
        Err(StoreError::Corrupt(_))
    ));
}
