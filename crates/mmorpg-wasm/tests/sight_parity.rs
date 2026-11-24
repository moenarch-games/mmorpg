//! Native-vs-browser parity (#57, #1150). The local zone host plays scripted Greyhaven sessions
//! and digests what they publish:
//! - the sight session: an arcanist faces a crate and casts Firebolt in the open, whose missile
//!   strikes a crate, then again from behind the inn, whose missile the inn stops (#517); every
//!   projection is digested;
//! - the physical sessions of `parity-sessions.json` (#1150), which exercise the paths whose float
//!   calls #1109 moved to `libm`: a Warden walks down a slope, shoulders the boulder trap and
//!   fights a bandit; an Arcanist climbs into Wolfrun Woods, breathes a Frost Breath cone and
//!   fights a wolf to its death with the staff; two characters walk into each other. Every
//!   projection and the canonical snapshot (unrounded positions and velocities) are digested
//!   after each tick, and each session must exercise what it is there for.
//!
//! Natively, two runs must reach the same digests. `web/tests/sight-parity.test.ts` runs these
//! tests with [`DIGEST_OUT`] and [`PHYSICAL_DIGESTS_OUT`] set, plays the same scripts on the
//! WASM build in the same run and must reach the digests written there, so the authoritative
//! decisions (and everything else the sessions project, broken props' debris included, #1109)
//! agree across targets. No digest is committed: it would move with every content or wire
//! change (#487).

use std::collections::BTreeMap;

use mmorpg_characters::{EntryBackend, EntryChange, LeaseHolder, StoreError};
use mmorpg_core::{CharacterId, ClassChoice, PlayerClass, Sex, ZoneCommand, ZoneEvent};
use mmorpg_protocol::{decode_snapshot, encode_command};
use mmorpg_wasm::host::{LOCAL_ZONE_ID, LocalZoneHost};

/// The script: the arcanist targets a crate by the smithy (prop 901) and walks south-east
/// from the spawn plaza, then turns toward the crates; its Firebolt from the open strikes one.
/// It walks on behind the inn, targets the crate beside it (prop 900) and turns toward it; the
/// inn stops that Firebolt (#517: missiles fly where the caster faces).
const MOVES: [(u64, i8, u16); 4] = [
    (0, 1, 24_576),
    (160, 0, 13_725),
    (290, 1, 24_576),
    (340, 0, 11_649),
];
/// `(tick, prop)`: a crate by the smithy first, then the one beside it.
const TARGETS: [(u64, u32); 2] = [(4, 901), (342, 900)];
const CASTS: [u64; 2] = [162, 344];
const FIREBOLT: u32 = 9;
const TICKS: u64 = 520;
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
    let (mut struck, mut blocked_hits, mut started) = (0, 0, 0);
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
                    facing: None,
                },
            );
        }
        host.tick().unwrap();
        let projection = host.projection(player).unwrap();
        let snapshot = decode_snapshot(&projection).unwrap();
        // The whole projection, broken props' debris included (#1109).
        digest = fnv(digest, &projection);
        for event in snapshot.events {
            match event {
                ZoneEvent::CastStarted { .. } => started += 1,
                // Only the crates can take the missiles' damage.
                ZoneEvent::DamageDealt { .. } if tick < CASTS[1] => struck += 1,
                ZoneEvent::DamageDealt { .. } => blocked_hits += 1,
                _ => {}
            }
        }
    }
    // The session exercises both sight decisions: a clear line and the inn in the way.
    assert!(
        started == 2 && struck > 0 && blocked_hits == 0,
        "started {started}, struck {struck}, behind the inn {blocked_hits}"
    );
    digest
}

/// When set, the path this test writes the physical sessions' native digests to (#1150): one
/// `<session> <16 hex digits>` line per session, in the script's order.
const PHYSICAL_DIGESTS_OUT: &str = "MMORPG_PHYSICAL_DIGESTS_OUT";
/// Frost Breath, the cone (#292).
const FROST_BREATH: u32 = 21;

/// `parity-sessions.json`, which the browser test plays too.
#[derive(serde::Deserialize)]
struct Script {
    sessions: Vec<Session>,
}

#[derive(serde::Deserialize)]
struct Session {
    name: String,
    world: String,
    #[serde(default)]
    lab: Option<Lab>,
    characters: Vec<Character>,
    ticks: u64,
    steps: Vec<Step>,
}

#[derive(serde::Deserialize)]
struct Lab {
    class: String,
    level: u8,
}

#[derive(serde::Deserialize)]
struct Character {
    id: String,
    class: u8,
    sex: u8,
}

#[derive(serde::Deserialize)]
struct Step {
    tick: u64,
    player: usize,
    command: mmorpg_wasm::wire::WorldCommand,
}

/// What a session did, to check that it exercises the paths it is there for.
#[derive(Default)]
struct Seen {
    /// Hits the players dealt creatures, and took from them.
    struck: u32,
    struck_by: u32,
    /// Frost Breath uses, and creatures it hit in the same tick.
    cones: u32,
    cone_hits: u32,
    creature_deaths: u32,
    /// Each player's lowest and highest body height, and furthest sideways (Z) drift.
    heights: Vec<(f32, f32)>,
    drift: Vec<f32>,
    /// Ticks a boulder had a player as its pusher, and its furthest distance from its spawn.
    boulder_pushed: u32,
    boulder_travel: f32,
}

#[test]
fn the_physical_sessions_digests_are_deterministic_and_handed_to_the_browser_build() {
    let script: Script = serde_json::from_str(include_str!("parity-sessions.json")).unwrap();
    let mut lines = String::new();
    for session in &script.sessions {
        let (digest, seen) = physical_session(session);
        assert_eq!(
            digest,
            physical_session(session).0,
            "two native runs of {} digest differently",
            session.name
        );
        check_coverage(&session.name, &seen);
        lines.push_str(&format!("{} {digest:016x}\n", session.name));
    }
    if let Some(path) = std::env::var_os(PHYSICAL_DIGESTS_OUT) {
        std::fs::write(&path, lines).unwrap_or_else(|error| {
            panic!(
                "cannot write {}: {error}",
                std::path::Path::new(&path).display()
            )
        });
    }
}

/// Fails unless the session exercised what it is in the script for.
fn check_coverage(name: &str, seen: &Seen) {
    let climbed = |player: usize| seen.heights[player].1 - seen.heights[player].0;
    match name {
        // Downhill off the rise, a boulder shouldered and a bandit fought hand to hand.
        "boulder-trap" => assert!(
            climbed(0) > 100.0
                && seen.boulder_pushed > 0
                && seen.boulder_travel > 100.0
                && seen.struck > 0
                && seen.struck_by > 0,
            "boulder-trap: height {}, pushed {} ticks, travel {}, struck {}, struck by {}",
            climbed(0),
            seen.boulder_pushed,
            seen.boulder_travel,
            seen.struck,
            seen.struck_by
        ),
        // Up the cutting, a cone that hits and a melee fight to the wolf's death.
        "wolf-pack" => assert!(
            climbed(0) > 300.0
                && seen.cones == 1
                && seen.cone_hits > 0
                && seen.struck > seen.cone_hits
                && seen.struck_by > 0
                && seen.creature_deaths > 0,
            "wolf-pack: height {}, cones {}, cone hits {}, struck {}, struck by {}, deaths {}",
            climbed(0),
            seen.cones,
            seen.cone_hits,
            seen.struck,
            seen.struck_by,
            seen.creature_deaths
        ),
        // Walking along X, only the other body turns either aside.
        "greyhaven-crowd" => assert!(
            seen.drift.iter().all(|&drift| drift > 10.0),
            "greyhaven-crowd: sideways drift {:?}",
            seen.drift
        ),
        other => panic!("no coverage check for session {other:?}"),
    }
}

/// Plays one scripted session and digests every player's projection and the canonical
/// snapshot (every f32 position and velocity, which projections round) after each tick.
fn physical_session(session: &Session) -> (u64, Seen) {
    let mut store = mmorpg_characters::CharacterStore::new(Memory::default());
    let (mut host, players) = match (session.world.as_str(), &session.lab) {
        ("greyhaven", _) => {
            let mut host = LocalZoneHost::new().unwrap();
            let players: Vec<_> = session
                .characters
                .iter()
                .map(|character| {
                    let choice = ClassChoice {
                        class: PlayerClass::from_code(character.class).unwrap(),
                        sex: Sex::from_code(character.sex).unwrap(),
                    };
                    let id = CharacterId::new(&character.id).unwrap();
                    let holder = LeaseHolder {
                        zone_id: LOCAL_ZONE_ID,
                        zone_epoch: 1,
                    };
                    host.enter(&mut store, id, choice, holder).unwrap()
                })
                .collect();
            (host, players)
        }
        ("action-lab", Some(lab)) => {
            // Standard gear and a full resource bar, as `forActionLab` in the browser test.
            let setup = mmorpg_wasm::action_lab::LabSetup::parse(
                &lab.class,
                lab.level,
                "standard",
                0,
                Some(u16::MAX),
            )
            .unwrap();
            let (host, player) = LocalZoneHost::action_lab(setup, 0).unwrap();
            (host, vec![player])
        }
        (name, _) => {
            let scenario = mmorpg_wasm::scenario::Scenario::from_name(name)
                .unwrap_or_else(|| panic!("unknown scenario {name:?}"));
            let (host, player) = LocalZoneHost::scenario(scenario, 0).unwrap();
            (host, vec![player])
        }
    };
    let mut sequences = vec![0_u32; players.len()];
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    let start = host.zone().snapshot().unwrap();
    let spawns: BTreeMap<_, _> = start
        .creatures
        .iter()
        .map(|creature| (creature.creature_id, creature.position))
        .collect();
    let mut seen = Seen {
        heights: vec![(f32::MAX, f32::MIN); players.len()],
        drift: vec![0.0; players.len()],
        ..Seen::default()
    };
    for tick in 0..session.ticks {
        for step in session.steps.iter().filter(|step| step.tick == tick) {
            sequences[step.player] += 1;
            let command = mmorpg_wasm::wire::encode_command(&step.command).unwrap();
            host.submit(players[step.player], sequences[step.player], &command)
                .unwrap();
        }
        host.tick().unwrap();
        for &player in &players {
            let projection = host.projection(player).unwrap();
            digest = fnv(digest, &projection);
            let mut cone = false;
            for event in decode_snapshot(&projection).unwrap().events {
                use mmorpg_core::EntityRef::{Creature, Player};
                match event {
                    ZoneEvent::AbilityUsed { ability, .. } if ability.get() == FROST_BREATH => {
                        seen.cones += 1;
                        cone = true;
                    }
                    ZoneEvent::DamageDealt {
                        source: Player(_),
                        target: Creature(_),
                        ..
                    } => {
                        seen.struck += 1;
                        seen.cone_hits += u32::from(cone);
                    }
                    ZoneEvent::DamageTaken {
                        source: Creature(_),
                        target: Player(_),
                        ..
                    } => seen.struck_by += 1,
                    ZoneEvent::Died {
                        entity: Creature(_),
                        killer: Some(Player(_)),
                    } => seen.creature_deaths += 1,
                    _ => {}
                }
            }
        }
        let canonical = mmorpg_wasm::lab_experiment::digest(&host).unwrap();
        digest = fnv(digest, format!("{canonical:016x}").as_bytes());
        let state = host.zone().snapshot().unwrap();
        for (index, body) in state.players.iter().enumerate() {
            let (low, high) = &mut seen.heights[index];
            *low = low.min(body.position[1]);
            *high = high.max(body.position[1]);
            let from = start.players[index].position[2];
            seen.drift[index] = seen.drift[index].max((body.position[2] - from).abs());
        }
        for boulder in &state.boulders {
            seen.boulder_pushed += u32::from(boulder.pusher.is_some());
            let at = state
                .creatures
                .iter()
                .find(|creature| creature.creature_id == boulder.creature_id)
                .unwrap()
                .position;
            let from = spawns[&boulder.creature_id];
            let travel = (at[0] - from[0]).hypot(at[2] - from[2]);
            seen.boulder_travel = seen.boulder_travel.max(travel);
        }
    }
    (digest, seen)
}
