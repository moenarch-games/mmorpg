//! Incident receipts expire (#1128, Atlas 3.2.i1128): a raid or cart incident's receipt is
//! dropped once the incident can no longer pay, so a long-lived character is admitted with only
//! the receipts still relevant, no incident pays twice across its expiry, and recovery across an
//! expiry continues exactly like the unbroken run.
use std::sync::Arc;

use mmorpg_core::raid::{PHASE_TICKS, RAID_PHASES, REWARD_COPPER};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureAi, DAY_TICKS, EntityRef, PlayerId, QuestId, RaidOutcome,
    RaidResolution, RewardKey, ThreatEntry, ZONE_START_TIME, ZoneId, ZoneSimulation,
    greyhaven_vale,
};

/// A zone at a late tick with reward tracking at `(known, 16)` and `players` players at the
/// barn, each having discovered every area so only incident rewards count.
fn zone_with(players: u32, known: u32) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.enable_reward_tracking(known, 16).unwrap();
    for player in 1..=players {
        zone.add_player(player).unwrap();
    }
    restored(&zone, |state| {
        state.tick = 100_000;
        for (index, player) in (0_i32..).zip(state.players.iter_mut()) {
            player.position = mmorpg_core::physics::from_units([6_900 + 60 * index, 1_190, 3_000]);
            player.discoveries = zone.content().discoveries().iter().copied().collect();
        }
    })
}

fn restored(
    zone: &ZoneSimulation,
    edit: impl FnOnce(&mut CanonicalZoneSnapshot),
) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    edit(&mut state);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// Puts `players` on the threat table of the running incident's first raider.
fn fought_by(zone: &ZoneSimulation, players: &[PlayerId]) -> ZoneSimulation {
    restored(zone, |state| {
        let raider = state
            .creatures
            .iter_mut()
            .find(|creature| creature.raid.is_some())
            .unwrap();
        raider.ai = CreatureAi::Engaged;
        raider.threat = players
            .iter()
            .map(|&player| ThreatEntry {
                entity: EntityRef::Player(player),
                threat: 5,
            })
            .collect();
    })
}

fn receipts(zone: &ZoneSimulation, player: PlayerId) -> Vec<RewardKey> {
    zone.reward_history()
        .unwrap()
        .players
        .get(&player)
        .map(|keys| keys.iter().copied().collect())
        .unwrap_or_default()
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
fn a_character_of_many_past_incidents_is_admitted_with_only_live_receipts() {
    // The zone's fortieth raid is over and its bound holds 8 receipts.
    let zone = restored(&zone_with(1, 8), |state| {
        state.raid.incidents = 40;
        state.raid.last = Some(RaidResolution {
            incident: 40,
            at: state.tick - 1,
            outcome: RaidOutcome::Defended,
        });
    });
    let floors = zone.incident_floors();
    assert_eq!(floors.raid, 41, "no raid incident so far can pay again");
    let mut zone = zone;
    for incident in 1..=40 {
        for key in [
            RewardKey::RaidParticipation { incident },
            RewardKey::RaidDefence { incident },
        ] {
            zone.load_reward_receipt(1, key).unwrap();
        }
    }
    for incident in 1..floors.cart {
        zone.load_reward_receipt(1, RewardKey::CartDefence { incident })
            .unwrap();
    }
    assert!(receipts(&zone, 1).is_empty(), "every past incident is shed");
    // Content receipts and the incidents still able to pay load as before.
    let live = [
        RewardKey::QuestCompletion {
            quest: QuestId::new(1),
        },
        RewardKey::RaidParticipation { incident: 41 },
        RewardKey::CartDefence {
            incident: floors.cart,
        },
    ];
    for key in live {
        zone.load_reward_receipt(1, key).unwrap();
    }
    assert_eq!(receipts(&zone, 1), live.to_vec());
}

#[test]
fn a_raid_pays_once_across_its_expiry_and_recovery_continues_exactly() {
    let mut zone = zone_with(3, 4_096);
    assert!(zone.begin_raid().unwrap());
    let mut zone = fought_by(&zone, &[1, 2]);
    zone.advance_tick().unwrap();
    let incident = zone.raid().active.as_ref().unwrap().id;
    let participation = RewardKey::RaidParticipation { incident };
    assert_eq!(copper(&zone, 1), REWARD_COPPER);
    assert_eq!(receipts(&zone, 1), vec![participation]);

    // Player 1's character leaves mid-incident and comes back as player 3: its stored receipt
    // is still live, so it loads, and fighting on pays nothing more.
    let stored = receipts(&zone, 1);
    assert!(zone.remove_player(1));
    for key in stored {
        zone.load_reward_receipt(3, key).unwrap();
    }
    let mut zone = fought_by(&zone, &[2, 3]);
    zone.advance_tick().unwrap();
    assert_eq!(copper(&zone, 3), 0);
    assert_eq!(receipts(&zone, 3), vec![participation]);

    // The last phase ends on the next tick: the raid is defended and pays its bonus.
    let mut zone = restored(&zone, |state| {
        let running = state.raid.active.as_mut().unwrap();
        running.phase = RAID_PHASES;
        running.scaled_for = vec![1; usize::from(RAID_PHASES)];
        running.phase_started = state.tick + 1 - u64::from(PHASE_TICKS);
    });
    assert_eq!(zone.incident_floors().raid, incident);
    let mut recovered = restored(&zone, |_| {});
    let defence = RewardKey::RaidDefence { incident };
    for tick in 0..4 {
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), zone.snapshot().unwrap());
        assert_eq!(zone.incident_floors().raid, incident + 1);
        match tick {
            0 => {
                assert_eq!(zone.raid().last.unwrap().outcome, RaidOutcome::Defended);
                // The bonus this tick granted keeps its receipt until the next tick begins;
                // every other receipt of the incident is gone.
                assert_eq!(receipts(&zone, 2), vec![defence]);
                assert!(receipts(&zone, 3).is_empty());
                assert!(zone.tick_work().incident_receipts_visited > 0);
                // Recovery between the expiry and the next tick continues exactly.
                recovered = restored(&zone, |_| {});
            }
            _ => {
                assert!(receipts(&zone, 2).is_empty());
                assert!(zone.reward_history().unwrap().grants.is_empty());
                // Only the bonus's receipt expired as this tick began, then nothing more.
                let dropped = usize::from(tick == 1);
                assert_eq!(zone.tick_work().incident_receipts_visited, dropped);
            }
        }
    }
    // Without its receipts, the ended incident still pays no one again.
    assert_eq!(copper(&zone, 3), 0);
    assert!(zone.load_reward_receipt(3, participation).is_ok());
    assert!(
        receipts(&zone, 3).is_empty(),
        "an ended incident is not loaded"
    );
}

/// The tick of `time` (ticks since midnight) on zone day `day`.
fn tick_at(day: u64, time: u32) -> u64 {
    day * u64::from(DAY_TICKS) + u64::from(time - ZONE_START_TIME)
}

#[test]
fn a_cart_day_expires_as_its_ambush_hold_ends_and_recovers_exactly() {
    let content = greyhaven_vale::content();
    let seed = content.rng_seed();
    let ambush = (1..)
        .find(|&day| mmorpg_core::cart::ambush_day(seed, tick_at(day, ZONE_START_TIME)))
        .unwrap();
    let run = content.cart().unwrap();
    let arrival = run.arrival(content.npc(run.merchant).unwrap().position);
    // The last tick of the hold comes next.
    let last = tick_at(ambush, arrival + mmorpg_core::cart::HOLD_TICKS - 1);
    let mut zone = restored(&zone_with(1, 64), |state| state.tick = last - 1);
    let day = mmorpg_core::cart::incident(last);
    assert_eq!(zone.incident_floors().cart, day, "the day can still pay");
    let defence = RewardKey::CartDefence { incident: day };
    zone.load_reward_receipt(1, defence).unwrap();
    assert_eq!(receipts(&zone, 1), vec![defence]);

    let mut recovered = restored(&zone, |_| {});
    zone.advance_tick().unwrap();
    recovered.advance_tick().unwrap();
    assert_eq!(zone.incident_floors().cart, day + 1);
    assert!(receipts(&zone, 1).is_empty(), "the hold is over");
    assert_eq!(recovered.snapshot().unwrap(), zone.snapshot().unwrap());
    zone.load_reward_receipt(1, defence).unwrap();
    assert!(receipts(&zone, 1).is_empty(), "an ended day is not loaded");
}
