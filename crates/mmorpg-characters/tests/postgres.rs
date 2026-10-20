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

#[test]
fn character_and_journal_updates_commit_or_roll_back_together() {
    let Some(url) = url() else { return };
    let mut entries = PostgresEntries::connect(&url, TIMEOUT).unwrap();
    entries
        .transaction(|store| {
            store.backend().with_sql(|transaction| {
                transaction
                    .batch_execute(
                        "CREATE TEMP TABLE character_test_journal (id integer PRIMARY KEY)",
                    )
                    .map_err(|error| StoreError::Backend(error.to_string()))
            })
        })
        .unwrap();
    let character = fresh("pg-journal");
    let acquired = entries
        .transaction(|store| store.acquire(&character, holder(1, 1)))
        .unwrap();
    let saved = record(&character);
    let insert_journal =
        |store: &CharacterStore<mmorpg_characters::postgres::PostgresTransactionEntries<'_>>,
         id: i32| {
            store.backend().with_sql(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO character_test_journal (id) VALUES ($1)",
                        &[&id],
                    )
                    .map_err(|error| StoreError::Backend(error.to_string()))?;
                Ok(())
            })
        };
    let aborted: Result<(), StoreError> = entries.transaction(|store| {
        insert_journal(store, 1)?;
        store.commit_at_revision(&acquired.lease, 0, &saved)?;
        Err(StoreError::Backend("abort after both writes".into()))
    });
    assert!(aborted.is_err());
    let observer = CharacterStore::new(PostgresEntries::connect(&url, TIMEOUT).unwrap());
    assert_eq!(observer.committed(&character).unwrap(), None);
    entries
        .transaction(|store| {
            store.backend().with_sql(|transaction| {
                let count: i64 = transaction
                    .query_one("SELECT count(*) FROM character_test_journal", &[])
                    .map_err(|error| StoreError::Backend(error.to_string()))?
                    .get(0);
                assert_eq!(count, 0);
                Ok(())
            })?;
            insert_journal(store, 1)?;
            store.commit_at_revision(&acquired.lease, 0, &saved)
        })
        .unwrap();
    assert_eq!(observer.committed(&character).unwrap().unwrap().revision, 1);

    // A caller cannot accidentally commit the journal after ignoring a failed
    // character compare-and-set in the same transaction.
    let error = entries
        .transaction(|store| {
            insert_journal(store, 2)?;
            let stale = store.commit_at_revision(&acquired.lease, 0, &saved);
            assert_eq!(
                stale,
                Err(StoreError::RevisionConflict {
                    expected: 0,
                    actual: 1
                })
            );
            Ok(())
        })
        .unwrap_err();
    assert_eq!(
        error,
        StoreError::RevisionConflict {
            expected: 0,
            actual: 1
        }
    );
    entries
        .transaction(|store| {
            store.backend().with_sql(|transaction| {
                let ids: Vec<i32> = transaction
                    .query("SELECT id FROM character_test_journal ORDER BY id", &[])
                    .map_err(|error| StoreError::Backend(error.to_string()))?
                    .into_iter()
                    .map(|row| row.get(0))
                    .collect();
                assert_eq!(ids, [1]);
                Ok(())
            })
        })
        .unwrap();
    assert_eq!(observer.committed(&character).unwrap().unwrap().revision, 1);

    // PostgreSQL's COMMIT command can otherwise silently roll back an aborted
    // transaction when SQL errors have been swallowed inside an application callback.
    let swallowed = entries.transaction(|store| {
        insert_journal(store, 3)?;
        store.backend().with_sql(|transaction| {
            assert!(transaction.simple_query("SELECT 1 / 0").is_err());
            Ok(())
        })
    });
    assert!(swallowed.is_err());
    entries
        .transaction(|store| {
            store.backend().with_sql(|transaction| {
                let count: i64 = transaction
                    .query_one("SELECT count(*) FROM character_test_journal", &[])
                    .map_err(|error| StoreError::Backend(error.to_string()))?
                    .get(0);
                assert_eq!(count, 1);
                Ok(())
            })
        })
        .unwrap();
}
