//! Recovery through the canonical wire format during a class fight
//! continues exactly: checkpoints mid-cast, mid-channel, during a creature
//! cast and while damage over time, heal over time, a root and an absorb
//! shield are active each restore a copy that runs beside the original,
//! compared by canonical and player-visible bytes every tick. Independent
//! runs of the same inputs agree byte for byte.
#[path = "../../mmorpg-core/tests/support/mod.rs"]
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    AbilityId, CanonicalZoneSnapshot, EntityRef, ZoneCommand, ZoneContent, ZoneId, ZoneSimulation,
};
use mmorpg_protocol::{decode_canonical_snapshot, encode_canonical_snapshot, pack_snapshot};

use support::arena::{self, WOLF};
use support::classes::{self, LURKER};

/// Every checkpoint's copy is compared for this many ticks.
const COMPARED_TICKS: u64 = 200;
const ARCANIST: u32 = 1;
const WARDEN: u32 = 2;
const RANGER: u32 = 3;

/// Two wolves within Frost Nova reach of the arcanist's spawn slot and a
/// lurker a little farther away.
fn den() -> Arc<ZoneContent> {
    classes::arena(
        vec![arena::wolf(400, [2, 3]), classes::lurker(400, [1, 2])],
        vec![
            arena::spawn(1, WOLF, [-250, 0]),
            arena::spawn(2, WOLF, [-350, 150]),
            arena::spawn(3, LURKER, [-300, -400]),
        ],
    )
}

fn ability(ability: AbilityId) -> ZoneCommand {
    ZoneCommand::UseAbility {
        ability: ability.get(),
        target: None,
    }
}

/// Scripted inputs by tick for the arcanist, the warden and the ranger.
fn commands(tick: u64) -> Vec<(u32, ZoneCommand)> {
    let wolf =
        ZoneCommand::SelectTarget(Some(EntityRef::Creature(mmorpg_core::CreatureId::new(1))));
    match tick {
        0 => vec![
            (ARCANIST, ZoneCommand::ChooseClass { class: 2, sex: 0 }),
            (ARCANIST, wolf),
            (WARDEN, ZoneCommand::ChooseClass { class: 0, sex: 1 }),
            (WARDEN, wolf),
            (WARDEN, ZoneCommand::StartAttack),
            (RANGER, ZoneCommand::ChooseClass { class: 1, sex: 0 }),
            (RANGER, wolf),
        ],
        1 => vec![
            (ARCANIST, ability(ids::FROST_NOVA)),
            (RANGER, ability(ids::SERPENT_STING)),
        ],
        50 => vec![(ARCANIST, ability(ids::ARCANE_BARRIER))],
        100 => vec![(ARCANIST, ability(ids::BLIZZARD))],
        120 | 220 | 320 | 420 => vec![(WARDEN, ability(ids::RALLYING_CRY))],
        300 => vec![(ARCANIST, ability(ids::FIREBOLT))],
        _ => Vec::new(),
    }
}

struct Run {
    zone: ZoneSimulation,
    sequences: [u32; 3],
}

impl Run {
    fn new() -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(4), den()).unwrap();
        for player in [ARCANIST, WARDEN, RANGER] {
            zone.add_player(player).unwrap();
            zone = classes::at_level(zone, player, 6);
        }
        Self {
            zone,
            sequences: [0; 3],
        }
    }

    /// A zone restored from this one's canonical bytes.
    fn recovered(&self) -> Self {
        let bytes = encode_canonical_snapshot(&self.zone.snapshot().unwrap()).unwrap();
        let zone = ZoneSimulation::from_snapshot(
            decode_canonical_snapshot(&bytes).unwrap(),
            Arc::clone(self.zone.content()),
        )
        .expect("canonical bytes restore a class fight");
        assert_eq!(
            encode_canonical_snapshot(&zone.snapshot().unwrap()).unwrap(),
            bytes
        );
        Self {
            zone,
            sequences: self.sequences,
        }
    }

    fn step(&mut self) {
        for (player, command) in commands(self.zone.current_tick()) {
            let sequence = &mut self.sequences[usize::try_from(player).unwrap() - 1];
            *sequence += 1;
            self.zone.apply_command(player, *sequence, command).unwrap();
        }
        self.zone.advance_tick().unwrap();
    }

    fn bytes(&self) -> (Vec<u8>, Vec<Vec<u8>>) {
        let canonical = encode_canonical_snapshot(&self.zone.snapshot().unwrap()).unwrap();
        let projections = [ARCANIST, WARDEN, RANGER]
            .map(|player| {
                pack_snapshot(&self.zone.snapshot_for_player(player).unwrap())
                    .unwrap()
                    .payload
            })
            .to_vec();
        (canonical, projections)
    }
}

type Checkpoint = (&'static str, fn(&CanonicalZoneSnapshot) -> bool);

fn player_has(state: &CanonicalZoneSnapshot, player: u32, aura: AbilityId) -> bool {
    state.players[usize::try_from(player).unwrap() - 1]
        .combat
        .abilities
        .auras
        .iter()
        .any(|candidate| candidate.ability == aura)
}

fn creature_has(state: &CanonicalZoneSnapshot, aura: AbilityId) -> bool {
    state.creatures.iter().any(|creature| {
        creature
            .abilities
            .auras
            .iter()
            .any(|candidate| candidate.ability == aura)
    })
}

fn casting(state: &CanonicalZoneSnapshot, ability: AbilityId) -> bool {
    state.players[0]
        .combat
        .abilities
        .cast
        .is_some_and(|cast| cast.ability == ability && cast.elapsed > 0)
}

const CHECKPOINTS: [Checkpoint; 7] = [
    ("root", |state| creature_has(state, ids::FROST_NOVA)),
    ("damage over time", |state| {
        creature_has(state, ids::SERPENT_STING)
    }),
    ("absorb", |state| {
        player_has(state, ARCANIST, ids::ARCANE_BARRIER)
    }),
    ("mid-channel", |state| casting(state, ids::BLIZZARD)),
    ("heal over time", |state| {
        player_has(state, WARDEN, ids::RALLYING_CRY)
    }),
    ("mid-cast", |state| casting(state, ids::FIREBOLT)),
    ("creature cast", |state| {
        state
            .creatures
            .iter()
            .any(|creature| creature.abilities.cast.is_some())
    }),
];

#[test]
fn recovery_during_a_class_fight_reproduces_every_following_tick() {
    let mut run = Run::new();
    let mut taken = [false; CHECKPOINTS.len()];
    let mut copies: Vec<(&str, Run, u64)> = Vec::new();
    while taken.contains(&false) || !copies.is_empty() {
        let state = run.zone.snapshot().unwrap();
        for ((name, reached), taken) in CHECKPOINTS.iter().zip(&mut taken) {
            if !*taken && reached(&state) {
                copies.push((name, run.recovered(), COMPARED_TICKS));
                *taken = true;
            }
        }
        assert!(
            state.tick < 3_000,
            "checkpoints never reached: {:?}",
            CHECKPOINTS
                .iter()
                .zip(taken)
                .filter(|(_, taken)| !taken)
                .map(|((name, _), _)| name)
                .collect::<Vec<_>>()
        );
        run.step();
        let expected = run.bytes();
        for (name, copy, remaining) in &mut copies {
            copy.step();
            assert_eq!(
                copy.bytes(),
                expected,
                "{name} copy diverged at tick {}",
                run.zone.current_tick()
            );
            *remaining -= 1;
        }
        copies.retain(|(_, _, remaining)| *remaining > 0);
    }
}

#[test]
fn identical_class_inputs_produce_identical_canonical_bytes() {
    let mut first = Run::new();
    let mut second = Run::new();
    for _ in 0..600 {
        first.step();
        second.step();
        assert_eq!(first.bytes(), second.bytes());
    }
    let state = first.zone.snapshot().unwrap();
    assert!(
        state
            .players
            .iter()
            .all(|player| player.combat.abilities.class.is_some()),
        "every player chose a class"
    );
}
