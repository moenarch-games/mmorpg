//! Native-vs-browser line-of-sight parity (#57): the local zone host plays one scripted
//! Greyhaven session (an arcanist targets a crate and casts Firebolt in the open, then again
//! from behind the inn, refused for line of sight) and digests every projection it publishes.
//! Natively, two runs must reach the same digest. `web/tests/sight-parity.test.ts` runs this
//! test with [`DIGEST_OUT`] set, plays the same script on the WASM build in the same run and
//! must reach the digest written there, so the authoritative sight decisions (and everything
//! else the session projects but broken props' debris) agree across targets. No digest is
//! committed: it would move with every content or wire change (#487).

use std::collections::BTreeMap;

use mmorpg_characters::{EntryBackend, EntryChange, LeaseHolder, StoreError};
use mmorpg_core::{CharacterId, ClassChoice, PlayerClass, Sex, ZoneCommand, ZoneEvent};
use mmorpg_protocol::{DEBRIS_RECORD_BYTES, decode_snapshot, encode_command};
use mmorpg_wasm::host::{LOCAL_ZONE_ID, LocalZoneHost};

/// The script: the arcanist targets a crate by the smithy (prop 901) and walks south-east
/// from the spawn plaza; its Firebolt from the open starts and lands. It walks on behind the
/// inn, targets the crate beside it (prop 900), and its Firebolts are refused.
const MOVES: [(u64, i8, u16); 4] = [
    (0, 1, 24_576),
    (160, 0, 24_576),
    (290, 1, 24_576),
    (340, 0, 24_576),
];
/// `(tick, prop)`: a crate by the smithy first, then the one beside it.
const TARGETS: [(u64, u32); 2] = [(4, 901), (342, 900)];
const CASTS: [u64; 3] = [162, 344, 380];
const FIREBOLT: u8 = 9;
const TICKS: u64 = 400;
/// When set, the path this test writes the native digest to (16 hex digits and a newline).
const DIGEST_OUT: &str = "MMORPG_SIGHT_DIGEST_OUT";

#[derive(Default)]
struct Memory(BTreeMap<String, Vec<u8>>);

impl EntryBackend for Memory {
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.get(id.as_str()).cloned())
    }

    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        let next = change(self.0.get(id.as_str()).map(Vec::as_slice))?;
        self.0.insert(id.as_str().to_owned(), next);
        Ok(())
    }
}

/// FNV-1a over every byte, 64-bit.
fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn the_sight_session_digest_is_deterministic_and_handed_to_the_browser_build() {
    let digest = sight_session();
    assert_eq!(
        digest,
        sight_session(),
        "two native runs digest differently"
    );
    if let Some(path) = std::env::var_os(DIGEST_OUT) {
        std::fs::write(&path, format!("{digest:016x}\n")).unwrap_or_else(|error| {
            panic!(
                "cannot write {}: {error}",
                std::path::Path::new(&path).display()
            )
        });
    }
}

/// Plays the script once and digests every projection, failing unless the session exercises
/// both sight decisions.
fn sight_session() -> u64 {
    let mut host = LocalZoneHost::new().unwrap();
    let mut store = mmorpg_characters::CharacterStore::new(Memory::default());
    let player = host
        .enter(
            &mut store,
            CharacterId::new("local-1").unwrap(),
            ClassChoice {
                class: PlayerClass::Arcanist,
                sex: Sex::Female,
            },
            LeaseHolder {
                zone_id: LOCAL_ZONE_ID,
                zone_epoch: 1,
            },
        )
        .unwrap();
    let mut sequence = 0;
    let mut send = |host: &mut LocalZoneHost, command| {
        sequence += 1;
        host.submit(player, sequence, &encode_command(command))
            .unwrap();
    };
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    let (mut refused, mut started) = (0, 0);
    for tick in 0..TICKS {
        if let Some(&(_, forward, facing)) = MOVES.iter().find(|(at, ..)| *at == tick) {
            send(
                &mut host,
                ZoneCommand::Move {
                    forward,
                    strafe: 0,
                    facing,
                },
            );
        }
        if let Some(&(_, prop)) = TARGETS.iter().find(|(at, _)| *at == tick) {
            send(
                &mut host,
                ZoneCommand::SelectTarget(Some(mmorpg_core::EntityRef::Creature(
                    mmorpg_core::CreatureId::new(prop),
                ))),
            );
        }
        if CASTS.contains(&tick) {
            send(
                &mut host,
                ZoneCommand::UseAbility {
                    ability: FIREBOLT,
                    target: None,
                },
            );
        }
        host.tick().unwrap();
        let projection = host.projection(player).unwrap();
        let snapshot = decode_snapshot(&projection).unwrap();
        // Broken props' debris (the trailing section) is left out: Rapier's dynamic pieces are
        // not bit-identical between wasm32 and native builds (a rotation component can round
        // one step apart), and they decide nothing.
        let kept = projection.len() - snapshot.debris.len() * DEBRIS_RECORD_BYTES;
        digest = fnv(digest, &projection[..kept]);
        for event in snapshot.events {
            match event {
                ZoneEvent::Error {
                    code: mmorpg_core::ErrorCode::LineOfSight,
                    ..
                } => refused += 1,
                ZoneEvent::CastStarted { .. } => started += 1,
                _ => {}
            }
        }
    }
    // The session exercises both sight decisions.
    assert!(
        refused > 0 && started > 0,
        "refused {refused}, started {started}"
    );
    digest
}
