//! World-activity experience (#916): discovering a named Greyhaven area pays its discovery
//! experience once per character, through replay, canonical recovery and a character record's
//! save and reload; a defended night raid pays its participants one bounded bonus.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::vale;
use mmorpg_core::raid::{DEFENCE_EXPERIENCE, PHASE_TICKS, RAID_PHASES, REWARD_EXPERIENCE};
use mmorpg_core::{
    CharacterId, ClassChoice, PLAYER_HALF_EXTENTS_UNITS, PlayerClass, RaidOutcome, RewardKey, Sex,
    ZoneId, ZoneSimulation, discovery_experience, greyhaven_vale,
};

const WARDEN: ClassChoice = ClassChoice {
    class: PlayerClass::Warden,
    sex: Sex::Male,
};

/// `zone` restored with player 1 standing on the ground at `at`.
fn moved(zone: &ZoneSimulation, at: [i32; 2]) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    let y = zone.definition().ground_at(at[0], at[1]) + PLAYER_HALF_EXTENTS_UNITS[1];
    state.players[0].position = mmorpg_core::physics::from_units([at[0], y, at[1]]);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// `(level, experience, discovered areas)` of player 1.
fn progress(zone: &ZoneSimulation) -> (u8, u32, Vec<mmorpg_core::AreaId>) {
    let player = zone.snapshot().unwrap().players.remove(0);
    (
        player.combat.level,
        player.combat.experience,
        player.discoveries.into_iter().collect(),
    )
}

/// Player 1's experience since level 1.
fn total(zone: &ZoneSimulation) -> u32 {
    let (level, experience, _) = progress(zone);
    (1..level)
        .map(|below| mmorpg_core::experience_to_next_level(below).unwrap())
        .sum::<u32>()
        + experience
}

fn ticks(zone: &mut ZoneSimulation, count: u32) {
    for _ in 0..count {
        zone.advance_tick().unwrap();
    }
}

#[test]
fn an_area_is_discovered_once_through_replay_recovery_and_a_saved_character() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.enable_reward_tracking(64, 16).unwrap();
    zone.create_character(1, WARDEN).unwrap();
    // The Outpost, where every character starts, is no discovery.
    ticks(&mut zone, 5);
    assert_eq!(progress(&zone), (1, 0, vec![]));

    // Into the Wolfrun Woods: discovered on the next tick, paid once.
    let entering = moved(&zone, [-7_600, 0]);
    let mut zone =
        ZoneSimulation::from_snapshot(entering.snapshot().unwrap(), Arc::clone(entering.content()))
            .unwrap();
    ticks(&mut zone, 1);
    let reward = discovery_experience(1).unwrap();
    assert_eq!(reward, 75);
    assert_eq!(progress(&zone), (1, reward, vec![vale::WOLFRUN_WOODS]));
    let history = zone.reward_history().unwrap();
    assert!(!history.allows(
        1,
        RewardKey::Discovery {
            area: vale::WOLFRUN_WOODS
        }
    ));
    ticks(&mut zone, 30);
    assert_eq!(progress(&zone).1, reward, "staying pays nothing more");

    // Replay from before the discovery reaches exactly the same state.
    let mut replay =
        ZoneSimulation::from_snapshot(entering.snapshot().unwrap(), Arc::clone(entering.content()))
            .unwrap();
    ticks(&mut replay, 31);
    assert_eq!(replay.snapshot().unwrap(), zone.snapshot().unwrap());

    // Canonical recovery keeps the discovery: leaving and coming back pays nothing.
    let mut recovered = moved(&zone, [0, 2_000]);
    ticks(&mut recovered, 5);
    let mut recovered = moved(&recovered, [-7_600, 0]);
    ticks(&mut recovered, 5);
    assert_eq!(progress(&recovered).1, reward);

    // The discovery is a durable character fact: a saved and reloaded character in the woods
    // earns nothing again.
    let record = recovered
        .capture_character(1, CharacterId::new("explorer").unwrap())
        .unwrap();
    assert_eq!(
        record.discoveries,
        std::collections::BTreeSet::from([vale::WOLFRUN_WOODS])
    );
    let mut reloaded =
        ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    reloaded.admit_character(7, &record).unwrap();
    ticks(&mut reloaded, 5);
    let player = reloaded.snapshot().unwrap().players.remove(0);
    assert_eq!(
        (player.combat.experience, player.discoveries),
        (reward, record.discoveries.clone())
    );
}

#[test]
fn a_defended_raid_pays_its_rewarded_participants_one_bounded_bonus() {
    for outcome in [RaidOutcome::Defended, RaidOutcome::Lost] {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        zone.enable_reward_tracking(64, 16).unwrap();
        zone.add_player(1).unwrap();
        let mut state = zone.snapshot().unwrap();
        state.tick = 100_000;
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        // Beside the barn, after the farm's own discovery has been paid.
        let mut zone = moved(&zone, [6_900, 3_000]);
        ticks(&mut zone, 1);
        let before = total(&zone);
        assert!(zone.begin_raid().unwrap());
        // The player already earned this incident's participation reward, and the last phase
        // ends on the next tick, with the barn standing or fallen.
        let mut state = zone.snapshot().unwrap();
        state.raid.rewarded = vec![1];
        let incident = state.raid.active.as_mut().unwrap();
        incident.participants = vec![1];
        incident.phase = RAID_PHASES;
        incident.scaled_for = vec![0; usize::from(RAID_PHASES)];
        incident.phase_started = state.tick + 1 - u64::from(PHASE_TICKS);
        if outcome == RaidOutcome::Lost {
            state.raid.damage = mmorpg_core::raid::OBJECTIVE_INTEGRITY;
        }
        let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        ticks(&mut zone, 1);
        let last = zone.raid().last.unwrap();
        assert_eq!(last.outcome, outcome);
        let bonus = if outcome == RaidOutcome::Defended {
            DEFENCE_EXPERIENCE
        } else {
            0
        };
        assert_eq!(total(&zone), before + bonus, "{outcome:?}");
        let receipt = RewardKey::RaidDefence {
            incident: last.incident,
        };
        assert_eq!(
            zone.reward_history().unwrap().allows(1, receipt),
            outcome == RaidOutcome::Lost
        );
        // Bounded: the incident resolved once, so later ticks and a recovery pay nothing more.
        ticks(&mut zone, 60);
        let mut recovered =
            ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()))
                .unwrap();
        ticks(&mut recovered, 60);
        assert_eq!(total(&recovered), before + bonus);
        assert!(
            bonus + REWARD_EXPERIENCE < 1_000,
            "less than the level-4 threshold"
        );
    }
}
