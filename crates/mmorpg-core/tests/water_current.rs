//! Floating and drifting in water (#667): pieces and props in Greyhaven's brook and Stillwater
//! float at the depth their density gives, drift with a channel's current and settle where a bank
//! or obstacle stops them; stone sinks. Characters feel a current by depth (Decision 17): a wader
//! in shallow water not at all, a swimmer fully. Replay and canonical recovery stay exact.
mod support;

use std::sync::Arc;

use mmorpg_core::physics::{DebrisBody, PieceShape, Vec3i};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureId, CreatureSpawn, Debris, PropKind, WaterChannel, WaterVolume,
    ZoneAreas, ZoneContent, ZoneId, ZoneSimulation, greyhaven_vale,
};
use support::arena::{self, GRAVEYARD};

/// Ticks per second.
const HZ: u64 = mmorpg_core::TICK_HZ as u64;

/// Under the rock the brook sinks beneath, on its last authored segment, 1.8 m upstream of the
/// segment's end at z 8800.
const BROOK: [f32; 2] = [-805.0, 8_620.0];
/// Where the brook's water ends: its last point and round cap.
const BROOK_END_Z: f32 = 8_800.0;
/// Deep in Stillwater, west of its centre.
const LAKE: [f32; 2] = [-9_200.0, -5_300.0];

const HEAVY_CRATE_ID: u32 = 9_200;
const BOULDER_ID: u32 = 9_201;

/// Greyhaven with a heavy crate and a boulder to drop into Stillwater.
fn greyhaven() -> Arc<ZoneContent> {
    let template = |kind| {
        greyhaven_vale::content()
            .creature_templates()
            .iter()
            .find(|template| template.prop() == Some(kind))
            .expect("Greyhaven has the prop")
            .id
    };
    let spawn = |id, kind, position| CreatureSpawn {
        id: CreatureId::new(id),
        template: template(kind),
        position,
        facing: 0,
        wander_radius: 0,
        levels: None,
    };
    greyhaven_vale::content_with_extra_spawns([
        spawn(HEAVY_CRATE_ID, PropKind::HeavyCrate, [-9_000, -6_700]),
        spawn(BOULDER_ID, PropKind::Boulder, [-9_200, -6_700]),
    ])
    .expect("content is valid")
}

/// A zone of `content` with player 1 standing at `at` (XZ), its state then edited by `edit`.
fn zone(
    content: Arc<ZoneContent>,
    at: [f32; 2],
    edit: impl FnOnce(&mut CanonicalZoneSnapshot),
) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    #[allow(clippy::cast_possible_truncation)]
    let ground = content.definition().ground_at(at[0] as i32, at[1] as i32);
    #[allow(clippy::cast_precision_loss)]
    let y = ground.max(greyhaven_vale::brook::SURFACE) as f32 + 90.0;
    state.players[0].position = [at[0], y, at[1]];
    edit(&mut state);
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

/// A barrel's lid (piece 6, 44×6×44 cm) lying flat at `position`.
fn lid(id: u32, position: [f32; 3]) -> Debris {
    Debris {
        id,
        kind: PropKind::Barrel,
        piece: 6,
        body: DebrisBody {
            position,
            rotation: [0.0, 0.0, 0.0, 1.0],
            velocity: [0.0; 3],
            spin: [0.0; 3],
            half_extents: Vec3i::new(22, 3, 22),
            shape: PieceShape::Cuboid,
            resting: false,
        },
        age: 0,
    }
}

/// Where a box `half_height` tall of `kind` floats: its centre's height over the surface.
fn float_offset(kind: PropKind, half_height: f32) -> f32 {
    half_height
        - 2.0 * half_height * mmorpg_core::buoyancy::density(kind)
            / mmorpg_core::buoyancy::WATER_DENSITY
}

/// Runs `zone` `ticks` ticks; checks that a twin replays it exactly and that a zone recovered
/// from its canonical snapshot halfway continues it exactly. Returns the snapshots.
fn run(mut zone: ZoneSimulation, ticks: u64) -> Vec<CanonicalZoneSnapshot> {
    let content = Arc::clone(zone.content());
    let mut twin =
        ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(&content)).unwrap();
    let mut recovered: Option<ZoneSimulation> = None;
    let mut snapshots = Vec::new();
    for tick in 0..ticks {
        zone.advance_tick().unwrap();
        twin.advance_tick().unwrap();
        let snapshot = zone.snapshot().unwrap();
        assert_eq!(twin.snapshot().unwrap(), snapshot, "replay parts at {tick}");
        if let Some(recovered) = recovered.as_mut() {
            recovered.advance_tick().unwrap();
            assert_eq!(
                recovered.snapshot().unwrap(),
                snapshot,
                "recovery parts at {tick}"
            );
        }
        if tick == ticks / 2 {
            recovered = Some(
                ZoneSimulation::from_snapshot(snapshot.clone(), Arc::clone(&content)).unwrap(),
            );
        }
        snapshots.push(snapshot);
    }
    snapshots
}

#[test]
fn a_lid_in_the_brook_floats_drifts_downstream_and_beaches_where_it_ends() {
    let surface = greyhaven_vale::brook::SURFACE as f32;
    let start = [BROOK[0], surface + 10.0, BROOK[1]];
    let zone = zone(greyhaven(), [BROOK[0] + 400.0, BROOK[1]], |state| {
        state.debris = vec![lid(1, start)];
        state.next_debris = 2;
    });
    let snapshots = run(zone, u64::from(mmorpg_core::DEBRIS_TICKS) - 1);
    let lid = |snapshot: &CanonicalZoneSnapshot| snapshot.debris.first().map(|piece| piece.body);
    // After 2 s it floats at the depth its density gives and drifts at about the current's
    // speed, downstream (the last segment runs north).
    let afloat = lid(&snapshots[usize::try_from(2 * HZ).unwrap()]).expect("the lid floats");
    let depth = afloat.position[1] - (surface + float_offset(PropKind::Barrel, 3.0));
    #[allow(clippy::cast_precision_loss)]
    let current = greyhaven_vale::brook::CURRENT as f32 / HZ as f32;
    assert!(depth.abs() < 1.0, "the lid floats {depth} units off");
    assert!(
        afloat.velocity[2] > 0.9 * current && afloat.velocity[2] < 1.05 * current,
        "the lid drifts at {:?}",
        afloat.velocity
    );
    // It runs aground where the brook ends, and settles there.
    let last = lid(snapshots.last().unwrap()).expect("the lid lasts");
    assert!(last.resting, "the lid still moves: {last:?}");
    assert!(
        (BROOK_END_Z..BROOK_END_Z + 100.0).contains(&last.position[2]),
        "the lid rests at {:?}, not where the brook ends",
        last.position
    );
}

#[test]
fn a_crate_floats_in_stillwater_and_a_boulder_sinks() {
    let surface = greyhaven_vale::lake::SURFACE as f32;
    // Both dropped in from just above the surface, drifting east.
    let zone = zone(greyhaven(), [LAKE[0], LAKE[1] - 1_400.0], |state| {
        for creature in &mut state.creatures {
            let at = match creature.creature_id.get() {
                HEAVY_CRATE_ID => [LAKE[0], surface + 40.0, LAKE[1]],
                BOULDER_ID => [LAKE[0] + 1_200.0, surface + 80.0, LAKE[1] + 300.0],
                _ => continue,
            };
            creature.position = at;
            creature.velocity = [1.0, 0.0, 0.0];
        }
    });
    let snapshots = run(zone, 8 * HZ);
    let creature = |snapshot: &CanonicalZoneSnapshot, id: u32| {
        snapshot
            .creatures
            .iter()
            .find(|creature| creature.creature_id.get() == id)
            .map(|creature| (creature.position, creature.velocity))
            .unwrap()
    };
    let last = snapshots.last().unwrap();
    // The crate bobs, then settles at the depth its density gives.
    let (crate_at, crate_velocity) = creature(last, HEAVY_CRATE_ID);
    let depth = crate_at[1] - (surface + float_offset(PropKind::HeavyCrate, 35.0));
    assert!(depth.abs() < 1.0, "the crate floats {depth} units off");
    assert_eq!(crate_velocity, [0.0; 3], "the crate still moves");
    // The boulder sinks at a steady speed, slowed by the water: 4 m down after 8 s and going.
    let (boulder_at, boulder_velocity) = creature(last, BOULDER_ID);
    assert!(
        boulder_at[1] < surface - 400.0 && boulder_velocity[1] < -0.5,
        "the boulder does not sink: {boulder_at:?} at {boulder_velocity:?}"
    );
}

/// Flat ground under three straight channels running east at 1.5 m/s: one 3 m deep (swim), one
/// 1 m deep (waist-deep wading) and one 25 cm deep (the shallow brook's depth).
fn channels() -> Arc<ZoneContent> {
    let channel = |name: &str, z: i32, surface: i32| WaterVolume {
        name: name.into(),
        centre: [0, z],
        radii: [200, 200],
        surface,
        air_pockets: Vec::new(),
        channel: Some(WaterChannel {
            points: vec![[-3_000, z], [3_000, z]],
            half_width: 200,
            current: 150,
        }),
    };
    Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_waters(vec![
            channel("deep", 0, 300),
            channel("waist", 1_000, 100),
            channel("shallow", 2_000, 25),
        ])
        .unwrap(),
    )
}

#[test]
fn the_current_carries_a_swimmer_and_a_deep_wader_but_not_a_shallow_one() {
    let content = channels();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    for player in 1..=3 {
        zone.add_player(player).unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    // The swimmer floats with its feet 1.4 m down; the waders stand on the bed.
    for (player, at) in state.players.iter_mut().zip([
        [0.0, 300.0 - 140.0 + 90.0, 0.0],
        [0.0, 90.0, 1_000.0],
        [0.0, 90.0, 2_000.0],
    ]) {
        player.position = at;
    }
    let zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    let snapshots = run(zone, 2 * HZ);
    let last = snapshots.last().unwrap();
    let drift: Vec<f32> = last
        .players
        .iter()
        .map(|player| player.position[0])
        .collect();
    // 1.5 m/s for 2 s; the waist-deep wader feels 4/7 of it, the shallow one none.
    assert!(
        (drift[0] - 300.0).abs() < 10.0,
        "the swimmer drifts {}",
        drift[0]
    );
    assert!(
        (drift[1] - 171.0).abs() < 10.0,
        "the wader drifts {}",
        drift[1]
    );
    assert!(
        drift[2].abs() < 0.5,
        "the shallow wader drifts {}",
        drift[2]
    );
}
