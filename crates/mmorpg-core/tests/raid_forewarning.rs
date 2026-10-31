//! Forewarning and defensive preparation before Greyhaven's night raid (#532, Atlas 5.1.59–5.1.62,
//! 5.1.69): uncertain signs on the approach before the nightly check, tracks a player reads and
//! reports to the east gate guard, braces on the barn once the guards are warned, a readiness that
//! softens every raider's hit without saving an unopposed barn, and that retires with its incident
//! or expires with a false alarm. None of it cancels, postpones or rewards anything.
use std::sync::Arc;

use mmorpg_core::raid::{
    FOREWARNING_TIME, OBJECTIVE_INTEGRITY, PHASE_TICKS, RAID_PHASES, READINESS_WARNED,
    TRIGGER_TIME, raid_night, signs_night,
};
use mmorpg_core::{
    DAY_TICKS, ErrorCode, PlayerId, RaidNotice, RaidOutcome, RaidPrep, ZONE_START_TIME,
    ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};

/// Every preparation Greyhaven authors: the warning and both braces.
const FULL: u8 = 0b111;

fn seed() -> u64 {
    greyhaven_vale::content().rng_seed()
}

/// The first tick at `time` of day after `from` on an evening whose signs and raid rolls match.
fn evening(from: u64, time: u32, signs: bool, raid: bool) -> u64 {
    let first = u64::from(time + DAY_TICKS - ZONE_START_TIME) % u64::from(DAY_TICKS);
    (0..)
        .map(|night| first + night * u64::from(DAY_TICKS))
        .find(|&tick| {
            tick > from && signs_night(seed(), tick) == signs && raid_night(seed(), tick) == raid
        })
        .unwrap()
}

/// A zone at `tick` with reward tracking and players standing at `feet`.
fn zone_at(tick: u64, feet: &[[i32; 2]]) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.enable_reward_tracking(64, 16).unwrap();
    for (index, _) in feet.iter().enumerate() {
        zone.add_player(u32::try_from(index).unwrap() + 1).unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    state.tick = tick;
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

fn move_to(zone: &ZoneSimulation, player: PlayerId, at: [i32; 2]) -> ZoneSimulation {
    restored(zone, |state| {
        let record = state
            .players
            .iter_mut()
            .find(|record| record.player_id == player)
            .unwrap();
        record.position = mmorpg_core::physics::from_units([at[0], 1_190, at[1]]);
    })
}

/// The raid feedback `player` got this tick: a refusal or a notice.
#[derive(Debug, Eq, PartialEq)]
enum Reply {
    Refused(ErrorCode),
    Notice(RaidNotice),
    Nothing,
}

fn reply(zone: &ZoneSimulation, player: PlayerId) -> Reply {
    zone.snapshot_for_player(player)
        .unwrap()
        .events
        .into_iter()
        .find_map(|event| match event {
            ZoneEvent::Error { code, .. } => Some(Reply::Refused(code)),
            ZoneEvent::Raid { notice } => Some(Reply::Notice(notice)),
            _ => None,
        })
        .unwrap_or(Reply::Nothing)
}

fn prepare(zone: &mut ZoneSimulation, player: PlayerId, sequence: u32, prep: RaidPrep) -> Reply {
    zone.apply_command(player, sequence, ZoneCommand::RaidPrepare { prep })
        .unwrap();
    zone.advance_tick().unwrap();
    reply(zone, player)
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

/// By the east gate guard, the lookout, within 150 m of the barn but well away from the raiders.
const BY_LOOKOUT: [i32; 2] = [2_900, 2_800];

fn site() -> mmorpg_core::RaidSite {
    greyhaven_vale::content().raid().unwrap().clone()
}

#[test]
fn signs_come_before_every_raid_and_some_false_alarms_and_never_change_the_trigger() {
    let seed = seed();
    let nights = 600_u64;
    let at = |night: u64| night * u64::from(DAY_TICKS);
    let raids = (0..nights).filter(|&n| raid_night(seed, at(n))).count();
    let signs = (0..nights).filter(|&n| signs_night(seed, at(n))).count();
    let false_alarms = (0..nights)
        .filter(|&n| signs_night(seed, at(n)) && !raid_night(seed, at(n)))
        .count();
    assert!((0..nights).all(|n| !raid_night(seed, at(n)) || signs_night(seed, at(n))));
    assert!(
        (60..=140).contains(&false_alarms),
        "{false_alarms} false alarms"
    );
    assert_eq!(signs, raids + false_alarms);

    for raid in [true, false] {
        let dusk = evening(0, FOREWARNING_TIME, true, raid);
        // Not yet before 19:00; then the tracks show near the farm, with no timer or phase.
        let mut zone = zone_at(dusk - 2, &[BY_LOOKOUT]);
        zone.advance_tick().unwrap();
        assert_eq!(zone.snapshot_for_player(1).unwrap().raid, None);
        zone.advance_tick().unwrap();
        let view = zone.snapshot_for_player(1).unwrap().raid.unwrap();
        assert!(view.signs && !view.scouted);
        assert_eq!(
            (view.phase, view.integrity, view.aid, view.readiness),
            (0, 100, 0, 0)
        );
        // Fully prepared or not, the nightly check decides exactly as its roll says.
        for readiness in [0, FULL] {
            let check = dusk + u64::from(TRIGGER_TIME - FOREWARNING_TIME);
            let mut zone = restored(&zone_at(check - 1, &[BY_LOOKOUT]), |state| {
                state.raid.readiness = readiness;
                state.raid.scouted = vec![1];
            });
            zone.advance_tick().unwrap();
            assert_eq!(zone.raid().active.is_some(), raid, "readiness {readiness}");
            assert!(
                zone.raid().scouted.is_empty(),
                "the evening's tracks are moot"
            );
            // The readiness serves the raid; a false alarm's expires.
            assert_eq!(zone.raid().readiness, if raid { readiness } else { 0 });
            let view = zone.snapshot_for_player(1).unwrap().raid;
            assert!(view.is_none_or(|view| !view.signs));
        }
    }
    // An evening without signs shows nothing.
    let quiet = evening(0, FOREWARNING_TIME + 10, false, false);
    let zone = zone_at(quiet, &[BY_LOOKOUT]);
    assert_eq!(zone.snapshot_for_player(1).unwrap().raid, None);
}

#[test]
fn players_read_the_tracks_warn_the_lookout_and_brace_the_barn_once_each() {
    let site = site();
    let dusk = evening(0, FOREWARNING_TIME, true, true) + 600;
    let near_tracks = [site.tracks[0], site.tracks[1] - 100];
    let near_brace = |index: usize| [site.braces[index][0], site.braces[index][1] + 100];
    let mut zone = zone_at(dusk, &[BY_LOOKOUT, BY_LOOKOUT]);
    let before = [purse(&zone, 1), purse(&zone, 2)];

    // Out of order or out of reach: nothing changes.
    assert_eq!(
        prepare(&mut zone, 1, 1, RaidPrep::Investigate),
        Reply::Refused(ErrorCode::OutOfRange)
    );
    assert_eq!(
        prepare(&mut zone, 1, 2, RaidPrep::Report),
        Reply::Refused(ErrorCode::InvalidTarget),
        "nothing to report yet"
    );
    assert_eq!(
        prepare(&mut zone, 1, 3, RaidPrep::Brace(0)),
        Reply::Refused(ErrorCode::InvalidTarget),
        "the guards ask for braces once warned"
    );
    assert_eq!(zone.raid().readiness, 0);

    // Player 1 reads the tracks, once.
    let mut zone = move_to(&zone, 1, near_tracks);
    assert_eq!(
        prepare(&mut zone, 1, 4, RaidPrep::Investigate),
        Reply::Notice(RaidNotice::Tracks)
    );
    assert_eq!(zone.raid().scouted, vec![1]);
    assert!(zone.snapshot_for_player(1).unwrap().raid.unwrap().scouted);
    assert_eq!(
        prepare(&mut zone, 1, 5, RaidPrep::Investigate),
        Reply::Refused(ErrorCode::InvalidTarget)
    );
    // Far from the lookout, the report waits; at his post, he takes it.
    assert_eq!(
        prepare(&mut zone, 1, 6, RaidPrep::Report),
        Reply::Refused(ErrorCode::OutOfRange)
    );
    let mut zone = move_to(&zone, 1, BY_LOOKOUT);
    assert_eq!(
        prepare(&mut zone, 2, 1, RaidPrep::Report),
        Reply::Refused(ErrorCode::InvalidTarget),
        "player 2 saw no tracks"
    );
    assert_eq!(
        prepare(&mut zone, 1, 7, RaidPrep::Report),
        Reply::Notice(RaidNotice::Warned)
    );
    assert_eq!(zone.raid().readiness, READINESS_WARNED);
    assert_eq!(
        prepare(&mut zone, 1, 8, RaidPrep::Report),
        Reply::Refused(ErrorCode::InvalidTarget),
        "warned is warned"
    );

    // Either player braces the barn's walls, each brace once.
    let mut zone = move_to(&move_to(&zone, 1, near_brace(0)), 2, near_brace(1));
    assert_eq!(
        prepare(&mut zone, 2, 2, RaidPrep::Brace(0)),
        Reply::Refused(ErrorCode::OutOfRange)
    );
    assert_eq!(
        prepare(&mut zone, 1, 9, RaidPrep::Brace(0)),
        Reply::Notice(RaidNotice::Braced)
    );
    assert_eq!(
        prepare(&mut zone, 1, 10, RaidPrep::Brace(0)),
        Reply::Refused(ErrorCode::InvalidTarget)
    );
    assert_eq!(
        prepare(&mut zone, 2, 3, RaidPrep::Brace(1)),
        Reply::Notice(RaidNotice::Braced)
    );
    assert_eq!(
        prepare(&mut zone, 2, 4, RaidPrep::Brace(7)),
        Reply::Refused(ErrorCode::InvalidTarget)
    );
    assert_eq!(zone.raid().readiness, FULL);
    let view = zone.snapshot_for_player(2).unwrap().raid.unwrap();
    assert_eq!((view.readiness, view.scouted), (FULL, false));
    assert_eq!(
        [purse(&zone, 1), purse(&zone, 2)],
        before,
        "preparing earns nothing"
    );
    let copy = restored(&zone, |_| {});
    assert_eq!(copy.snapshot().unwrap(), zone.snapshot().unwrap());

    // Once the raid is on, the evening is over: no more preparing.
    let mut zone = restored(&zone, |state| {
        state.tick = dusk - 600 + u64::from(TRIGGER_TIME - FOREWARNING_TIME) - 1;
    });
    zone.advance_tick().unwrap();
    assert!(zone.raid().active.is_some());
    assert_eq!(
        prepare(&mut zone, 1, 11, RaidPrep::Brace(0)),
        Reply::Refused(ErrorCode::InvalidTarget)
    );
}

/// Runs an unwatched raid from `zone` to its resolution; the ticks it took.
fn run_raid(zone: &mut ZoneSimulation) -> u64 {
    let bound = u64::from(PHASE_TICKS) * u64::from(RAID_PHASES) + 10;
    for step in 0..bound {
        zone.advance_tick().unwrap();
        if zone.raid().active.is_none() {
            return step;
        }
    }
    panic!("the raid resolves within its phases");
}

#[test]
fn readiness_slows_an_unopposed_raid_without_saving_the_barn_and_retires_with_it() {
    let start = zone_at(100_000, &[BY_LOOKOUT]);
    let mut plain = restored(&start, |_| {});
    assert!(plain.begin_raid().unwrap());
    let mut prepared = restored(&start, |state| state.raid.readiness = FULL);
    assert!(prepared.begin_raid().unwrap());
    let plain_ticks = run_raid(&mut plain);
    let prepared_ticks = run_raid(&mut prepared);
    for zone in [&plain, &prepared] {
        assert_eq!(zone.raid().last.unwrap().outcome, RaidOutcome::Lost);
        assert_eq!(zone.raid().damage, OBJECTIVE_INTEGRITY);
        assert_eq!(zone.raid().readiness, 0, "readiness retires with its raid");
    }
    assert!(
        prepared_ticks > plain_ticks + 600,
        "the braced barn held out longer: {prepared_ticks} vs {plain_ticks}"
    );
    // The observer hears that the preparations were not enough, and nothing without them.
    assert_eq!(reply(&prepared, 1), Reply::Notice(RaidNotice::Fell));
    assert_eq!(reply(&plain, 1), Reply::Nothing);

    // A prepared raid the defenders hold: the notice says so, and the next raid starts bare.
    let mut held = restored(&start, |state| state.raid.readiness = READINESS_WARNED);
    assert!(held.begin_raid().unwrap());
    let mut held = restored(&held, |state| {
        let incident = state.raid.active.as_mut().unwrap();
        incident.phase = RAID_PHASES;
        incident.scaled_for = vec![0, 0, 0];
        incident.phase_started = state.tick + 1 - u64::from(PHASE_TICKS);
    });
    held.advance_tick().unwrap();
    assert_eq!(held.raid().last.unwrap().outcome, RaidOutcome::Defended);
    assert_eq!(reply(&held, 1), Reply::Notice(RaidNotice::Held));
    assert_eq!(held.raid().readiness, 0);
}
