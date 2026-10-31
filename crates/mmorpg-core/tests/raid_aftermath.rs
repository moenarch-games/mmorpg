//! A lost night raid's aftermath on Millbrook Farm (#531, Atlas 5.1.50–5.1.52): breaches in the
//! barn and injured gate guards at the farmhouse, an unattended restoration that settles them one by
//! one in finite time, optional aid that closes a task once and speeds the shared restoration
//! without touching the cooldown, and participation rewards granted once per incident and
//! character to defenders and helpers only.
use std::sync::Arc;

use mmorpg_core::raid::{
    AID_RESTORE_POINTS, INJURED_HEALTH_PERCENT, OBJECTIVE_INTEGRITY, RECOVERY_TICKS_PER_POINT,
    REWARD_COPPER, REWARD_EXPERIENCE,
};
use mmorpg_core::{
    AidKind, CreatureAi, EntityKind, EntityRef, ErrorCode, PlayerId, RaidOutcome, RewardKey,
    ThreatEntry, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};

/// Every aid task Greyhaven authors.
const ALL_TASKS: u8 = 0b1_1111;
/// A fresh level-1 character, and one who earned the reward: level 2 with the rest of the XP.
const FRESH: (u8, u32, u32) = (1, 0, 0);
const REWARDED: (u8, u32, u32) = (2, REWARD_EXPERIENCE - 100, REWARD_COPPER);

/// A zone at a late tick with reward tracking and `feet.len()` players standing at `feet`.
fn zone_at(feet: &[[i32; 2]]) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.enable_reward_tracking(64, 16).unwrap();
    for (index, _) in feet.iter().enumerate() {
        zone.add_player(u32::try_from(index).unwrap() + 1).unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    state.tick = 100_000;
    for (player, at) in state.players.iter_mut().zip(feet) {
        player.position = mmorpg_core::physics::from_units([at[0], 1_190, at[1]]);
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn restored(
    zone: &ZoneSimulation,
    edit: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot),
) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    edit(&mut state);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// Commits an incident and loses it on the next tick.
fn lose_raid(mut zone: ZoneSimulation) -> ZoneSimulation {
    assert!(zone.begin_raid().unwrap());
    let mut zone = restored(&zone, |state| state.raid.damage = OBJECTIVE_INTEGRITY);
    zone.advance_tick().unwrap();
    assert_eq!(zone.raid().last.unwrap().outcome, RaidOutcome::Lost);
    zone
}

/// `(level, xp, copper)` of a player.
fn purse(zone: &ZoneSimulation, player: PlayerId) -> (u8, u32, u32) {
    let state = zone.snapshot().unwrap();
    let record = state
        .players
        .iter()
        .find(|record| record.player_id == player)
        .unwrap();
    (record.combat.level, record.combat.experience, record.copper)
}

/// A guard's projected feet XZ and health, as `viewer` sees it.
fn guard(zone: &ZoneSimulation, viewer: PlayerId, npc: u32) -> ([i32; 2], u8) {
    let view = zone.snapshot_for_player(viewer).unwrap();
    let entity = view
        .entities
        .iter()
        .find(|entity| entity.kind == EntityKind::Npc && entity.id == npc)
        .unwrap_or_else(|| panic!("guard {npc} is visible"));
    (
        [entity.position[0], entity.position[2]],
        entity.health_percent,
    )
}

/// Aids `task` as `player` and returns the refusal, if any.
fn aid(zone: &mut ZoneSimulation, player: PlayerId, sequence: u32, task: u8) -> Option<ErrorCode> {
    zone.apply_command(player, sequence, ZoneCommand::RaidAid { task })
        .unwrap();
    zone.advance_tick().unwrap();
    zone.snapshot_for_player(player)
        .unwrap()
        .events
        .into_iter()
        .find_map(|event| match event {
            ZoneEvent::Error { code, .. } => Some(code),
            _ => None,
        })
}

fn tasks() -> Vec<mmorpg_core::AidTask> {
    greyhaven_vale::content().raid().unwrap().aid.clone()
}

#[test]
fn a_lost_raid_injures_the_gate_guards_and_the_farm_recovers_on_its_own_in_order() {
    let tasks = tasks();
    let [AidKind::TendGuard(east), AidKind::TendGuard(south)] = [tasks[2].kind, tasks[4].kind]
    else {
        panic!("tasks 2 and 4 tend the gate guards");
    };
    let content = greyhaven_vale::content();
    let posts = [east, south].map(|npc| (content.npc(npc).unwrap().position, 100));
    let (east, south) = (east.get(), south.get());
    // A viewer by the farmhouse, a metre from the shelter.
    let mut zone = zone_at(&[[5_600, 4_650]]);
    zone = lose_raid(zone);
    assert_eq!(zone.raid().aftermath, ALL_TASKS);
    assert_eq!(guard(&zone, 1, east), (tasks[2].at, INJURED_HEALTH_PERCENT));
    assert_eq!(
        guard(&zone, 1, south),
        (tasks[4].at, INJURED_HEALTH_PERCENT)
    );
    let view = zone.snapshot_for_player(1).unwrap().raid.unwrap();
    assert_eq!((view.phase, view.integrity, view.aid), (0, 0, ALL_TASKS));
    // Nobody helps: the barn regrows a point at a time, and each task settles, in order, as the
    // restoration reaches its share; the last one once the barn is whole. Skip ahead to the
    // point just above each share instead of simulating the whole 20 min.
    for task in 0..5_u8 {
        let share = OBJECTIVE_INTEGRITY * u32::from(4 - task) / 5;
        zone = restored(&zone, |state| state.raid.damage = share + 1);
        assert_eq!(zone.raid().aftermath, ALL_TASKS << task & ALL_TASKS);
        for _ in 0..=RECOVERY_TICKS_PER_POINT {
            zone.advance_tick().unwrap();
        }
        assert_eq!(zone.raid().damage, share);
        assert_eq!(
            zone.raid().aftermath,
            ALL_TASKS << (task + 1) & ALL_TASKS,
            "task {task} settled on its own"
        );
    }
    for (npc, post) in [east, south].into_iter().zip(posts) {
        let placement = zone.npc_placement(mmorpg_core::NpcId::new(npc)).unwrap();
        assert_eq!(
            placement.position, post.0,
            "guard {npc} is back at his gate"
        );
    }
    assert!(
        !zone.raid_eligible(zone.snapshot().unwrap().tick),
        "restoration does not shorten the cooldown"
    );
    assert_eq!(purse(&zone, 1), FRESH, "watching earns nothing");
}

#[test]
fn aid_closes_a_task_once_speeds_the_shared_restoration_and_rewards_each_helper_once() {
    let tasks = tasks();
    // Player 1 by the east gate guard's shelter, player 2 by the north breach, player 3 in town.
    let near = |task: usize| [tasks[task].at[0] - 100, tasks[task].at[1]];
    let mut zone = lose_raid(zone_at(&[near(2), near(0), [0, 2_000]]));
    let incident = zone.raid().last.unwrap().incident;
    let before = zone.raid().damage;

    assert_eq!(aid(&mut zone, 3, 1, 0), Some(ErrorCode::OutOfRange));
    assert_eq!(aid(&mut zone, 1, 1, 7), Some(ErrorCode::InvalidTarget));
    assert_eq!(zone.raid().aftermath, ALL_TASKS);

    assert_eq!(aid(&mut zone, 1, 2, 2), None);
    assert_eq!(zone.raid().aftermath, ALL_TASKS & !0b100);
    assert!(zone.raid().damage <= before - AID_RESTORE_POINTS);
    assert_eq!(purse(&zone, 1), REWARDED);
    let history = zone.reward_history().unwrap();
    assert!(!history.allows(1, RewardKey::RaidParticipation { incident }));
    let AidKind::TendGuard(east) = tasks[2].kind else {
        panic!("task 2 tends a guard");
    };
    assert_eq!(
        zone.npc_placement(east).unwrap().position,
        greyhaven_vale::content().npc(east).unwrap().position,
        "the tended guard is back at his gate"
    );
    // Done is done: the same task again changes nothing.
    let damage = zone.raid().damage;
    assert_eq!(aid(&mut zone, 1, 3, 2), Some(ErrorCode::InvalidTarget));
    assert!(zone.raid().damage <= damage && zone.raid().damage + 1 >= damage);

    // A second helper earns its own reward; recovery and replay continue exactly.
    assert_eq!(aid(&mut zone, 2, 1, 0), None);
    assert_eq!(purse(&zone, 2), REWARDED);
    let copy = restored(&zone, |_| {});
    assert_eq!(copy.snapshot().unwrap(), zone.snapshot().unwrap());

    // A further aid by a rewarded helper speeds recovery but earns nothing more.
    let mut zone = restored(&zone, |state| {
        let player = state.players.iter_mut().find(|p| p.player_id == 1).unwrap();
        player.position =
            mmorpg_core::physics::from_units([tasks[3].at[0], 1_190, tasks[3].at[1] - 100]);
    });
    assert_eq!(aid(&mut zone, 1, 4, 3), None);
    assert_eq!(purse(&zone, 1), REWARDED);
    assert_eq!(purse(&zone, 3), FRESH);
    assert!(
        !zone.raid_eligible(zone.snapshot().unwrap().tick),
        "aid never bypasses the cooldown"
    );
}

#[test]
fn defenders_who_fought_are_rewarded_once_and_bystanders_are_not() {
    let mut zone = zone_at(&[[6_900, 3_000], [7_000, 3_000]]);
    assert!(zone.begin_raid().unwrap());
    zone = restored(&zone, |state| {
        let raider = state
            .creatures
            .iter_mut()
            .find(|creature| creature.raid.is_some())
            .unwrap();
        raider.ai = CreatureAi::Engaged;
        // Player 1 struck a raider; player 2 was only noticed by one.
        raider.threat = vec![
            ThreatEntry {
                entity: EntityRef::Player(1),
                threat: 5,
            },
            ThreatEntry {
                entity: EntityRef::Player(2),
                threat: 0,
            },
        ];
    });
    zone.advance_tick().unwrap();
    assert_eq!(zone.raid().rewarded, vec![1]);
    assert_eq!(
        zone.raid().active.as_ref().unwrap().participants,
        vec![1, 2],
        "both still count toward the next wave's size"
    );
    for _ in 0..30 {
        zone.advance_tick().unwrap();
    }
    assert_eq!(purse(&zone, 1).2, REWARD_COPPER, "granted once");
    assert_eq!(purse(&zone, 2), FRESH);
}
