//! Deterministic work samples of `directory-poll` in `.performance/contract.json` (#1146),
//! printed as the `operations` part of `coding-tooling/work-evidence/v1`;
//! `scripts/operation-work.py` adds the envelope. Counters only; nothing is timed.
//!
//! A host's party reconciliation over the shared directory ([`SharedDirectory`] over the
//! in-memory row): every poll syncs the zone's roster, sweeps and computes each character's
//! view, as `PartyService::reconcile` does. Over `polls` polls, `changes` of them bring one more
//! character into the zone. Only those take the row's lock, and the host decodes the stored
//! encoding once, however many polls read it.
use mmorpg_control_plane::party::{PartyDirectory, PartyRosterEntry};
use mmorpg_control_plane::shared_directory::{MemoryRow, SharedDirectory, StoredDirectory};
use mmorpg_core::{CharacterId, ZoneId};

const ZONE: ZoneId = ZoneId::new(901);
const RESIDENTS: u32 = 8;

fn entry(index: u32) -> PartyRosterEntry {
    PartyRosterEntry {
        character: CharacterId::new(format!("character-{index}")).expect("a valid ID"),
        player: index + 1,
        name: format!("Character {index}"),
    }
}

fn directory_poll(polls: u32, changes: u32) -> String {
    let mut directory: SharedDirectory<PartyDirectory, MemoryRow> =
        SharedDirectory::new(MemoryRow::new(PartyDirectory::FORMAT));
    let mut roster: Vec<_> = (0..RESIDENTS).map(entry).collect();
    let every = polls / changes.max(1);
    for poll in 0..polls {
        if changes > 0 && poll % every == 0 && poll / every < changes {
            roster.push(entry(RESIDENTS + poll / every));
        }
        directory
            .transact(None, &mut |parties, now| {
                parties.sync_zone(ZONE, &roster, now);
                parties.sweep(now);
                for resident in &roster {
                    let _ = parties.view(&resident.character, 0);
                }
            })
            .expect("the in-memory row never fails");
    }
    let work = directory.work();
    format!(
        "{{\"dimensions\":{{\"polls\":{polls},\"changes\":{changes}}},\"metrics\":{{\"row_locks\":{},\"directory_decodes\":{}}}}}",
        work.locked_writes, work.decodes
    )
}

fn main() {
    let samples: Vec<_> = [(100, 4), (1_000, 4), (10_000, 4), (1_000, 40)]
        .into_iter()
        .map(|(polls, changes)| directory_poll(polls, changes))
        .collect();
    println!(
        "{{\"operations\":[{{\"id\":\"directory-poll\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
}
