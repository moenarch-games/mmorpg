use std::sync::{Arc, Barrier};

use mmorpg_characters::{
    CharacterStore, EntryBackend, FileEntries, LeaseHolder, StoreError, decode_entry,
};
use mmorpg_core::{
    CharacterId, CharacterRecord, CharacterTransfer, ClassChoice, PlayerClass, Sex, TransferId,
    TransferMotion, ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale,
};
use mmorpg_protocol::{decode_character_record, encode_character_record};

const ZONE: ZoneId = ZoneId::new(1);

fn holder(zone: u32, epoch: u64) -> LeaseHolder {
    LeaseHolder {
        zone_id: ZoneId::new(zone),
        zone_epoch: epoch,
    }
}

fn id(value: &str) -> CharacterId {
    CharacterId::new(value).unwrap()
}

/// A Ranger who walked, earned copper and experience and is level 3.
fn played_record(character: &CharacterId) -> CharacterRecord {
    let mut zone = ZoneSimulation::with_content(ZONE, greyhaven_vale::content()).unwrap();
    zone.create_character(
        7,
        ClassChoice {
            class: PlayerClass::Ranger,
            sex: Sex::Female,
        },
    )
    .unwrap();
    zone.apply_command(
        7,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 900,
        },
    )
    .unwrap();
    for _ in 0..20 {
        zone.advance_tick().unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    let player = &mut state.players[0];
    player.copper = 1_234;
    player.combat.level = 3;
    player.combat.experience = 40;
    player.combat.health = 50;
    let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.capture_character(7, character.clone()).unwrap()
}

#[test]
fn a_record_round_trips_through_bytes_and_a_new_session() {
    let character = id("local-1");
    let record = played_record(&character);
    assert_eq!(record.copper, 1_234);
    assert_eq!(record.level, 3);
    assert_ne!(record.location.position, [0; 3]);

    let bytes = encode_character_record(&record).unwrap();
    let decoded = decode_character_record(&bytes).unwrap();
    assert_eq!(decoded, record);

    // A new process and session: another zone instance, another player ID.
    let mut zone = ZoneSimulation::with_content(ZONE, greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    zone.admit_character(42, &decoded).unwrap();
    assert_eq!(zone.capture_character(42, character).unwrap(), record);

    // Strict decoding: truncation, trailing bytes and another schema fail closed.
    assert!(decode_character_record(&bytes[..bytes.len() - 1]).is_err());
    assert!(decode_character_record(&[bytes.as_slice(), &[0]].concat()).is_err());
    let mut other_schema = bytes.clone();
    other_schema[2] ^= 0xff;
    assert!(decode_character_record(&other_schema).is_err());
}

#[test]
fn an_invalid_record_is_refused_without_admitting_anyone() {
    let mut record = played_record(&id("local-2"));
    record.health = 1_000_000;
    let mut zone = ZoneSimulation::with_content(ZONE, greyhaven_vale::content()).unwrap();
    assert!(zone.admit_character(1, &record).is_err());
    assert_eq!(zone.player_count(), 0);
}

#[test]
fn one_zone_holds_a_character_and_commits_under_its_lease() {
    let directory = tempdir("lease");
    let mut store = CharacterStore::new(FileEntries::open(&directory).unwrap());
    let character = id("local-3");
    let first = store.acquire(&character, holder(1, 5)).unwrap();
    assert_eq!((first.revision, first.record.clone()), (0, None));

    // Another zone, or the same epoch again, is refused while the lease is held.
    assert_eq!(
        store.acquire(&character, holder(2, 9)),
        Err(StoreError::Leased(holder(1, 5)))
    );
    assert_eq!(
        store.acquire(&character, holder(1, 5)),
        Err(StoreError::Leased(holder(1, 5)))
    );

    let record = played_record(&character);
    assert_eq!(store.commit(&first.lease, &record).unwrap(), 1);
    let committed = store.committed(&character).unwrap().unwrap();
    assert_eq!(committed.record, record);
    assert_eq!(committed.leased_by, Some(holder(1, 5)));
    assert_eq!(store.release(&first.lease, None).unwrap(), 1);
    assert_eq!(
        store.commit(&first.lease, &record),
        Err(StoreError::StaleLease)
    );

    // Released: any zone may now take it, and receives the committed record.
    let second = store.acquire(&character, holder(2, 1)).unwrap();
    assert_eq!(second.record, Some(record));
    assert_eq!(second.revision, 1);
}

#[test]
fn a_newer_epoch_of_the_same_zone_fences_its_crashed_predecessor() {
    let directory = tempdir("fence");
    let mut store = CharacterStore::new(FileEntries::open(&directory).unwrap());
    let character = id("local-4");
    let record = played_record(&character);
    let crashed = store.acquire(&character, holder(1, 5)).unwrap();
    store.commit(&crashed.lease, &record).unwrap();

    // The zone restarted at epoch 6 without releasing the character.
    let current = store.acquire(&character, holder(1, 6)).unwrap();
    assert_eq!(current.record.as_ref(), Some(&record));
    let mut later = record.clone();
    later.copper += 1;
    assert_eq!(
        store.commit(&crashed.lease, &later),
        Err(StoreError::StaleLease)
    );
    assert_eq!(
        store.release(&crashed.lease, Some(&later)),
        Err(StoreError::StaleLease)
    );
    assert_eq!(store.committed(&character).unwrap().unwrap().record, record);
    store.release(&current.lease, Some(&later)).unwrap();
    // The old epoch never gets the character back.
    assert_eq!(
        store.acquire(&character, holder(1, 5)),
        Err(StoreError::FencedEpoch(holder(1, 6)))
    );
}

#[test]
fn racing_hosts_never_both_acquire() {
    let directory = tempdir("race");
    let character = id("local-5");
    let barrier = Arc::new(Barrier::new(8));
    let winners: usize = (0..8_u32)
        .map(|zone| {
            let directory = directory.clone();
            let character = character.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                // Each host opens its own handles, as separate processes would.
                let mut store = CharacterStore::new(FileEntries::open(directory).unwrap());
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
fn corrupt_entries_fail_closed() {
    let directory = tempdir("corrupt");
    let character = id("local-6");
    let mut store = CharacterStore::new(FileEntries::open(&directory).unwrap());
    let acquired = store.acquire(&character, holder(1, 1)).unwrap();
    store
        .commit(&acquired.lease, &played_record(&character))
        .unwrap();
    let bytes = store.backend().read(&character).unwrap().unwrap();
    assert!(decode_entry(&character, &bytes).is_ok());
    // An entry filed under another character's name is not that character.
    assert!(decode_entry(&id("local-7"), &bytes).is_err());

    std::fs::write(
        directory.join("local-6.character"),
        &bytes[..bytes.len() - 3],
    )
    .unwrap();
    assert!(matches!(
        store.committed(&character),
        Err(StoreError::Corrupt(_))
    ));
    assert!(matches!(
        store.acquire(&character, holder(1, 2)),
        Err(StoreError::Corrupt(_))
    ));
}

/// `record` frozen by zone 1 for zone 2 under `transfer`.
fn transfer(record: &CharacterRecord, transfer: u64) -> CharacterTransfer {
    let mut record = record.clone();
    record.location.zone_id = ZoneId::new(2);
    CharacterTransfer {
        transfer_id: TransferId::new(transfer).unwrap(),
        source_zone: ZONE,
        cutover_tick: 20,
        cutover_sequence: 1,
        record,
        motion: TransferMotion {
            forward: 1,
            strafe: 0,
            rise: 0,
            velocity: [3, 0, -4],
        },
    }
}

#[test]
fn a_handoff_moves_the_lease_and_record_atomically_once() {
    let directory = tempdir("handoff");
    let mut store = CharacterStore::new(FileEntries::open(&directory).unwrap());
    let character = id("local-8");
    let record = played_record(&character);
    let source = store.acquire(&character, holder(1, 5)).unwrap().lease;
    store.commit(&source, &record).unwrap();
    let frozen = transfer(&record, 31);
    let id_31 = frozen.transfer_id;

    // A handoff must leave for another zone, with the record placed there.
    assert!(matches!(
        store.begin_handoff(&source, &frozen, holder(1, 6)),
        Err(StoreError::InvalidHandoff(_))
    ));
    store.begin_handoff(&source, &frozen, holder(2, 3)).unwrap();
    store.begin_handoff(&source, &frozen, holder(2, 3)).unwrap();
    assert_eq!(
        store.begin_handoff(&source, &transfer(&record, 32), holder(2, 3)),
        Err(StoreError::HandoffPending(id_31))
    );
    // The pending transfer survives in the entry bytes.
    let pending = store.pending_handoff(&character).unwrap().unwrap();
    assert_eq!(
        (&pending.transfer, pending.to, pending.from),
        (&frozen, holder(2, 3), holder(1, 5))
    );

    // Nobody writes, releases or takes the character while it is pending,
    // not even a newer epoch of the source zone.
    for refused in [
        store.commit(&source, &record),
        store.release(&source, None),
        store.acquire(&character, holder(1, 9)).map(|_| 0),
        store.acquire(&character, holder(2, 3)).map(|_| 0),
    ] {
        assert_eq!(refused, Err(StoreError::HandoffPending(id_31)));
    }

    let completed = store.complete_handoff(&character, id_31).unwrap();
    assert_eq!(completed.lease.holder, holder(2, 3));
    assert_eq!(completed.revision, 2);
    assert_eq!(completed.record.as_ref(), Some(&frozen.record));
    assert_eq!(
        store.complete_handoff(&character, id_31).unwrap(),
        completed
    );
    assert_eq!(
        store.cancel_handoff(&character, id_31),
        Err(StoreError::HandoffCompleted(id_31))
    );
    assert_eq!(store.commit(&source, &record), Err(StoreError::StaleLease));
    store.commit(&completed.lease, &frozen.record).unwrap();
    // Once a later acquisition supersedes it, the old completion is no proof.
    let restarted = store.acquire(&character, holder(2, 4)).unwrap();
    assert_eq!(
        store.complete_handoff(&character, id_31),
        Err(StoreError::StaleLease)
    );
    store.release(&restarted.lease, None).unwrap();
}

#[test]
fn a_cancelled_handoff_leaves_the_source_holding_the_character() {
    let directory = tempdir("cancel");
    let mut store = CharacterStore::new(FileEntries::open(&directory).unwrap());
    let character = id("local-9");
    let record = played_record(&character);
    let source = store.acquire(&character, holder(1, 5)).unwrap().lease;
    store.commit(&source, &record).unwrap();
    let frozen = transfer(&record, 41);
    store.begin_handoff(&source, &frozen, holder(2, 3)).unwrap();

    assert_eq!(
        store.cancel_handoff(&character, frozen.transfer_id),
        Ok(true)
    );
    assert_eq!(
        store.cancel_handoff(&character, frozen.transfer_id),
        Ok(false)
    );
    assert_eq!(
        store.complete_handoff(&character, frozen.transfer_id),
        Err(StoreError::UnknownHandoff(frozen.transfer_id))
    );
    let committed = store.committed(&character).unwrap().unwrap();
    assert_eq!((committed.revision, committed.record), (1, record.clone()));
    assert_eq!(committed.leased_by, Some(holder(1, 5)));
    store.commit(&source, &record).unwrap();
}

fn tempdir(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("mmorpg-characters-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    directory
}
