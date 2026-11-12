//! Predicted own movement reconciles to the zone's state (#680, Atlas 5.9.i680): over a link
//! with fixed uplink and downlink delays, the prediction shows now what the zone computes one
//! uplink later, and whatever the zone decided that the prediction could not know wins once
//! the round trip completes.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use mmorpg_client_model::prediction::PredictedMovement;
use mmorpg_core::{
    PlayerCombatPolicy, SpawnGrid, StaticCollider, ZoneAreas, ZoneCommand, ZoneContent,
    ZoneDefinition, ZoneId, ZoneSimulation, ZoneSnapshot,
};

/// Flat ground with a wall 6 m south (-Z) of the spawn rows, which run north.
fn content() -> Arc<ZoneContent> {
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [0, 0],
            columns: 8,
            spacing: 200,
        },
        vec![
            StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [10_000, 50, 10_000],
            },
            StaticCollider {
                id: 2,
                position: [0, 200, -600],
                half_extents: [2_000, 200, 50],
            },
        ],
    )
    .unwrap();
    Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![],
            vec![],
            vec![],
            [0, 0],
        )
        .unwrap()
        .with_player_combat(PlayerCombatPolicy::FreeForAll),
    )
}

const fn run(forward: i8, strafe: i8, facing: u16) -> ZoneCommand {
    ZoneCommand::Move {
        forward,
        strafe,
        facing,
    }
}

/// A zone and a client joined by a link `up` ticks one way and `down` ticks back.
struct Link {
    zone: ZoneSimulation,
    client: PredictedMovement,
    up: u64,
    down: u64,
    clock: u64,
    sequence: u32,
    uplink: VecDeque<(u64, u32, ZoneCommand)>,
    downlink: VecDeque<(u64, ZoneSnapshot)>,
    /// The zone's exact own position by zone tick.
    zone_at: BTreeMap<u64, [f32; 3]>,
    /// The prediction's position by local tick.
    predicted_at: BTreeMap<u64, [f32; 3]>,
}

impl Link {
    fn new(up: u64, down: u64, blocker: bool) -> Self {
        let content = content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        if blocker {
            // Someone the prediction cannot know stands 2 m east of the spawn.
            zone.add_player(2).unwrap();
        }
        Self {
            client: PredictedMovement::new(ZoneId::new(1), content).unwrap(),
            zone,
            up,
            down,
            clock: 0,
            sequence: 0,
            uplink: VecDeque::new(),
            downlink: VecDeque::new(),
            zone_at: BTreeMap::new(),
            predicted_at: BTreeMap::new(),
        }
    }

    /// The client sends `command` after the current tick.
    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.client.sent(self.sequence, command).unwrap();
        self.uplink
            .push_back((self.clock + 1 + self.up, self.sequence, command));
    }

    /// One tick on both sides.
    fn tick(&mut self) {
        self.clock += 1;
        while self
            .uplink
            .front()
            .is_some_and(|(due, ..)| *due <= self.clock)
        {
            let (_, sequence, command) = self.uplink.pop_front().unwrap();
            self.zone.apply_command(1, sequence, command).unwrap();
        }
        self.zone.advance_tick().unwrap();
        let tick = self.zone.current_tick();
        self.zone_at
            .insert(tick, self.zone.player_motion(1).unwrap().0);
        self.downlink.push_back((
            self.clock + self.down,
            self.zone.snapshot_for_player(1).unwrap(),
        ));

        self.client.step().unwrap();
        let mut newest = None;
        while self
            .downlink
            .front()
            .is_some_and(|(due, _)| *due <= self.clock)
        {
            let (_, snapshot) = self.downlink.pop_front().unwrap();
            self.client
                .acknowledge(snapshot.tick, snapshot.acknowledged_sequence);
            newest = Some(snapshot);
        }
        if let Some(snapshot) = newest {
            self.client.reconcile_snapshot(&snapshot).unwrap();
        }
        self.predicted_at
            .insert(self.clock, self.client.position().unwrap().0);
    }

    fn ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }
}

fn gap(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f32>()
        .sqrt()
}

#[test]
fn the_prediction_shows_now_what_the_zone_computes_one_uplink_later() {
    for (up, down) in [(0, 0), (2, 1), (3, 3), (5, 4)] {
        let mut link = Link::new(up, down, false);
        link.ticks(10);
        link.send(run(1, 0, 0));
        link.ticks(25);
        link.send(run(1, 1, 8_000));
        link.ticks(15);
        link.send(ZoneCommand::Jump);
        link.ticks(20);
        // South into the wall: the prediction stops where the zone stops.
        link.send(run(1, 0, 32_768));
        link.ticks(120);
        link.send(run(0, 0, 0));
        link.ticks(20);
        let last = link.clock - up;
        // Projections round velocity to whole units per tick, so in the air the replay of a
        // round trip may drift by up to half a unit per replayed tick; on the ground it is
        // exact to the projection's rounding.
        #[allow(clippy::cast_precision_loss)]
        let airborne = 0.5 * (up + down) as f32 + 1.0;
        for local in 1..=last {
            let predicted = link.predicted_at[&local];
            let zone = link.zone_at[&(local + up)];
            let tolerance = if zone[1].max(predicted[1]) > 90.5 {
                airborne
            } else {
                1.0
            };
            assert!(
                gap(predicted, zone) <= tolerance,
                "up {up} down {down}: local tick {local} predicted {predicted:?}, zone {zone:?}"
            );
        }
        let still = link.zone_at[&link.zone.current_tick()];
        assert!(
            still[2] > -600.0 + 50.0 && still[2] < -500.0,
            "the wall stopped the zone's body: {still:?}"
        );
    }
}

#[test]
fn what_the_zone_decided_alone_wins_within_a_round_trip() {
    let (up, down) = (4, 3);
    let mut link = Link::new(up, down, true);
    link.ticks(5);
    // East into the other player, whom only the zone has.
    link.send(run(1, 0, 16_384));
    link.ticks(40);
    link.send(run(0, 0, 16_384));
    link.ticks(up + down + 2);
    let zone = link.zone.player_motion(1).unwrap().0;
    let predicted = link.client.position().unwrap().0;
    assert!(
        zone[2].abs() > 1.0,
        "the other player pushed the zone's body aside: {zone:?}"
    );
    assert!(
        gap(predicted, zone) <= 1.0,
        "predicted {predicted:?} reconciles to the zone's {zone:?}"
    );
}
