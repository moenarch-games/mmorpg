//! Reward capacity never aborts a tick (#1112, Atlas 3.2.i1112): zone payouts that do not fit
//! a tick wait in the canonical deferred backlog, are paid oldest first within a bounded number
//! of ticks, survive recovery exactly, and a zone whose receipt bound is exhausted keeps
//! ticking until room frees.
use std::sync::Arc;

use mmorpg_core::raid::REWARD_COPPER;
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureAi, EntityRef, MAX_PLAYERS_PER_ZONE, PlayerId, RewardKey,
    ThreatEntry, ZoneId, ZoneSimulation, greyhaven_vale,
};

/// A zone at a late tick with reward tracking at `(known, grants)` and `players` players at the
/// barn, each having discovered every area so only raid rewards count.
fn zone_with(players: u32, known: u32, grants: u16) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.enable_reward_tracking(known, grants).unwrap();
    for player in 1..=players {
        zone.add_player(player).unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    state.tick = 100_000;
    for (index, player) in (0_i32..).zip(state.players.iter_mut()) {
        // A row of defenders a little apart, west of the barn.
        player.position = mmorpg_core::physics::from_units([
            6_900 + 60 * (index % 16),
            1_190,
            3_000 + 60 * (index / 16),
        ]);
        player.discoveries = zone.content().discoveries().iter().copied().collect();
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn restored(
    zone: &ZoneSimulation,
    edit: impl FnOnce(&mut CanonicalZoneSnapshot),
) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    edit(&mut state);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// Starts a raid whose first raider was struck by every one of `defenders`.
fn defended_raid(zone: &ZoneSimulation, defenders: &[PlayerId]) -> ZoneSimulation {
    let mut zone = restored(zone, |_| {});
    assert!(zone.begin_raid().unwrap());
    restored(&zone, |state| {
        let raider = state
            .creatures
            .iter_mut()
            .find(|creature| creature.raid.is_some())
            .unwrap();
        raider.ai = CreatureAi::Engaged;
        raider.threat = defenders
            .iter()
            .map(|&player| ThreatEntry {
                entity: EntityRef::Player(player),
                threat: 5,
            })
            .collect();
    })
}

fn copper(zone: &ZoneSimulation, player: PlayerId) -> u32 {
    zone.snapshot()
        .unwrap()
        .players
        .iter()
        .find(|record| record.player_id == player)
        .unwrap()
        .copper
}

#[test]
fn a_payout_to_more_players_than_the_tick_bound_pays_everyone_and_recovers_exactly() {
    const PLAYERS: u32 = 10;
    const TICK_GRANTS: u16 = 4;
    let defenders: Vec<PlayerId> = (1..=PLAYERS).collect();
    let mut zone = defended_raid(&zone_with(PLAYERS, 4_096, TICK_GRANTS), &defenders);
    zone.advance_tick().unwrap();
    assert_eq!(zone.raid().rewarded, defenders);
    let history = zone.reward_history().unwrap();
    assert_eq!(history.grants.len(), usize::from(TICK_GRANTS));
    assert_eq!(history.deferred.len(), 6, "the rest wait for later ticks");
    assert_eq!(
        defenders
            .iter()
            .filter(|&&player| copper(&zone, player) == REWARD_COPPER)
            .count(),
        4
    );

    // Recovery mid-backlog continues exactly like the unbroken run.
    let mut recovered = restored(&zone, |_| {});
    assert_eq!(recovered.snapshot().unwrap(), zone.snapshot().unwrap());
    let ticks = (PLAYERS as usize).div_ceil(usize::from(TICK_GRANTS)) - 1;
    for _ in 0..ticks {
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), zone.snapshot().unwrap());
        assert!(zone.tick_work().deferred_rewards_visited <= usize::from(TICK_GRANTS));
    }
    assert!(zone.reward_history().unwrap().deferred.is_empty());
    for &player in &defenders {
        assert_eq!(
            copper(&zone, player),
            REWARD_COPPER,
            "player {player} paid once"
        );
        assert!(!zone.reward_history().unwrap().allows(
            player,
            RewardKey::RaidParticipation {
                incident: zone.raid().active.as_ref().unwrap().id
            }
        ));
    }
    zone.advance_tick().unwrap();
    assert_eq!(zone.tick_work().deferred_rewards_visited, 0);
}

#[test]
fn a_full_zone_with_full_receipt_sets_keeps_ticking_and_pays_once_room_frees() {
    const PER_PLAYER: u32 = 2;
    let players = u32::try_from(MAX_PLAYERS_PER_ZONE).unwrap();
    let mut zone = zone_with(players, players * PER_PLAYER, 16);
    for player in 1..=players {
        for incident in 1..=PER_PLAYER {
            zone.load_reward_receipt(player, RewardKey::RaidDefence { incident })
                .unwrap();
        }
    }
    assert!(!zone.reward_history().unwrap().fits(1));
    assert!(
        zone.load_reward_receipt(1, RewardKey::Cache { cache: 1 })
            .is_err(),
        "a further stored receipt would refuse its admission"
    );

    let defenders: Vec<PlayerId> = (1..=8).collect();
    let mut zone = defended_raid(&zone, &defenders);
    for _ in 0..5 {
        zone.advance_tick().unwrap();
    }
    assert_eq!(
        zone.reward_history().unwrap().deferred.len(),
        defenders.len()
    );
    assert!(defenders.iter().all(|&player| copper(&zone, player) == 0));

    // Players leaving free their receipts; the waiting payouts are paid on the next tick.
    for player in players - 3..=players {
        assert!(zone.remove_player(player));
    }
    zone.advance_tick().unwrap();
    assert!(zone.reward_history().unwrap().deferred.is_empty());
    assert!(
        defenders
            .iter()
            .all(|&player| copper(&zone, player) == REWARD_COPPER)
    );
}
