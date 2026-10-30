//! Recovery through the canonical wire format during a fight continues
//! exactly: checkpoints mid-chase, mid-swing, after a death and during an
//! evade each restore a copy that runs beside the original for 300 ticks,
//! compared by canonical and player-visible bytes every tick. Independent
//! runs of the same inputs agree byte for byte.
#[path = "../../mmorpg-core/tests/support/arena.rs"]
mod arena;

use std::sync::Arc;

use mmorpg_core::{
    CanonicalCreatureSnapshot, CanonicalZoneSnapshot, CreatureAi, CreatureId, CreatureLife,
    EntityRef, ZoneCommand, ZoneContent, ZoneId, ZoneSimulation,
};
use mmorpg_protocol::{decode_canonical_snapshot, encode_canonical_snapshot, pack_snapshot};

use arena::{NORTHWARD, WOLF};

/// Every checkpoint's copy is compared for this many ticks.
const COMPARED_TICKS: u64 = mmorpg_core::golden::t30(300);

/// Two wolves 6.7 m apart (packmates) and an NPC.
fn pack() -> Arc<ZoneContent> {
    let mut wolf = arena::wolf(80, [20, 30]);
    wolf.respawn_ticks = mmorpg_core::golden::t30(900) as _;
    arena::arena(
        vec![wolf],
        vec![
            arena::spawn(1, WOLF, [0, 2_000]),
            arena::spawn(2, WOLF, [600, 2_300]),
        ],
        vec![arena::npc(9, [-500, 300])],
    )
}

/// One weak wolf that dies quickly, then corpses, despawns and respawns.
fn lone_wolf() -> Arc<ZoneContent> {
    let mut wolf = arena::wolf(6, [1, 1]);
    // Longer than the corpse's 900 ticks, so it rests despawned for a while.
    wolf.respawn_ticks = mmorpg_core::golden::t30(1_200) as _;
    arena::arena(vec![wolf], vec![arena::spawn(1, WOLF, [0, 2_000])], vec![])
}

/// Scripted inputs by tick: walk into aggro range, fight, die, release, then
/// walk back in; a second player watches from the spawn row.
fn commands(tick: u64) -> Vec<(u32, ZoneCommand)> {
    // The script was written in 30 Hz ticks; it runs at the same moments at TICK_HZ.
    let scale = u64::from(mmorpg_core::TICK_HZ) / 30;
    if !tick.is_multiple_of(scale) {
        return Vec::new();
    }
    let tick = tick / scale;
    let target = ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(1))));
    match tick {
        0 => vec![(1, arena::walk(NORTHWARD)), (2, arena::stand(NORTHWARD))],
        60 => vec![(1, arena::stand(NORTHWARD)), (1, target)],
        61 => vec![(2, ZoneCommand::SelectTarget(Some(EntityRef::Player(1))))],
        500 => vec![(1, ZoneCommand::ReleaseSpirit)],
        530 => vec![(1, arena::walk(NORTHWARD))],
        _ => Vec::new(),
    }
}

struct Run {
    zone: ZoneSimulation,
    sequences: [u32; 2],
}

impl Run {
    fn new(content: Arc<ZoneContent>) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(3), content).unwrap();
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        Self {
            zone,
            sequences: [0; 2],
        }
    }

    /// A zone restored from this one's canonical bytes.
    fn recovered(&self) -> Self {
        let bytes = encode_canonical_snapshot(&self.zone.snapshot().unwrap()).unwrap();
        let zone = ZoneSimulation::from_snapshot(
            decode_canonical_snapshot(&bytes).unwrap(),
            Arc::clone(self.zone.content()),
        )
        .expect("canonical bytes restore a fighting zone");
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
        // From the stand to the release, player 1 swings at the wolf whenever it is in reach (#517).
        let scale = u64::from(mmorpg_core::TICK_HZ) / 30;
        if (60 * scale..500 * scale).contains(&self.zone.current_tick())
            && let Some(swing) =
                arena::swing_at(&self.zone, 1, EntityRef::Creature(CreatureId::new(1)))
        {
            self.sequences[0] += 1;
            self.zone
                .apply_command(1, self.sequences[0], swing)
                .unwrap();
        }
        self.zone.advance_tick().unwrap();
    }

    fn bytes(&self) -> (Vec<u8>, Vec<Vec<u8>>) {
        let canonical = encode_canonical_snapshot(&self.zone.snapshot().unwrap()).unwrap();
        let projections = [1, 2]
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

/// Runs the script; whenever the next checkpoint holds, restores a copy that
/// then steps beside the original for [`COMPARED_TICKS`] ticks.
fn verify_checkpoints(content: Arc<ZoneContent>, checkpoints: &[Checkpoint]) {
    let mut run = Run::new(content);
    let mut copies: Vec<(&str, Run, u64)> = Vec::new();
    let mut next = 0;
    while next < checkpoints.len() || !copies.is_empty() {
        let state = run.zone.snapshot().unwrap();
        if let Some((name, reached)) = checkpoints.get(next)
            && reached(&state)
        {
            copies.push((name, run.recovered(), COMPARED_TICKS));
            next += 1;
        }
        assert!(
            state.tick < mmorpg_core::golden::t30(5_000),
            "checkpoint {:?} never reached",
            checkpoints.get(next).map(|(name, _)| name)
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

fn first_wolf(state: &CanonicalZoneSnapshot) -> &CanonicalCreatureSnapshot {
    &state.creatures[0]
}

#[test]
fn recovery_during_a_fight_reproduces_every_following_tick() {
    verify_checkpoints(
        pack(),
        &[
            ("mid-chase", |state| {
                first_wolf(state).ai == CreatureAi::Engaged
                    && first_wolf(state).velocity != [0.0; 3]
            }),
            ("mid-swing", |state| {
                first_wolf(state).swing_timer > 0
                    && state.players[0].combat.swing_timer > 0
                    && !state.players[0].combat.events.is_empty()
            }),
            ("after a death", |state| state.players[0].combat.health == 0),
            (
                "during evade",
                |state| matches!(first_wolf(state).ai, CreatureAi::Evading { ticks } if ticks > 3),
            ),
            ("released", |state| {
                state.players[0].combat.health > 0 && state.tick > mmorpg_core::golden::t30(500)
            }),
        ],
    );
}

#[test]
fn recovery_around_a_corpse_reproduces_despawn_and_respawn() {
    verify_checkpoints(
        lone_wolf(),
        &[
            ("corpse", |state| {
                matches!(first_wolf(state).life, CreatureLife::Corpse { .. })
            }),
            ("despawned", |state| {
                matches!(first_wolf(state).life, CreatureLife::Despawned { .. })
            }),
            ("respawned", |state| {
                first_wolf(state).life == CreatureLife::Alive
                    && state.tick > mmorpg_core::golden::t30(900)
            }),
        ],
    );
}

#[test]
fn identical_inputs_produce_identical_canonical_bytes() {
    let mut first = Run::new(pack());
    let mut second = Run::new(pack());
    let mut dead_ticks = 0;
    for _ in 0..mmorpg_core::golden::t30(900) {
        first.step();
        second.step();
        assert_eq!(first.bytes(), second.bytes());
        dead_ticks += usize::from(first.zone.snapshot().unwrap().players[0].combat.health == 0);
    }
    assert!(dead_ticks > 0, "the scripted fight kills the player");
}
