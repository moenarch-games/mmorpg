//! Capped server rewind of timed swings' hits on bodies (#680, Atlas 5.9.i680): the zone
//! rewinds a sweep by the lateness it measured itself plus the clients' interpolation, never
//! beyond the match's cap, never for clashes, and a restored zone rewinds exactly alike.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CLIENT_INTERPOLATION_TICKS, ClashKind, EntityRef, HitRewind, PlayerCombatPolicy, PlayerId,
    StrikeAim, StrikeHeight, StrikeSide, TimedCombat, ZoneCommand, ZoneContent, ZoneEvent, ZoneId,
    ZoneSimulation,
};

const EAST: u16 = 16_384;
const WEST: u16 = 49_152;
const MIDDLE: StrikeAim = StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle);

fn arena(rewind: bool) -> Arc<ZoneContent> {
    arena_with(rewind.then_some(HitRewind::ARENA_TRIAL))
}

fn arena_with(rewind: Option<HitRewind>) -> Arc<ZoneContent> {
    let content = ZoneContent::new(
        support::arena::ground(),
        mmorpg_core::ZoneAreas::default(),
        vec![],
        vec![],
        vec![],
        [0, 0],
    )
    .unwrap()
    .with_creature_abilities(vec![])
    .unwrap()
    .with_player_combat(PlayerCombatPolicy::FreeForAll)
    .with_timed_combat(TimedCombat::ARENA)
    .unwrap();
    Arc::new(match rewind {
        Some(rewind) => content.with_hit_rewind(rewind).unwrap(),
        None => content,
    })
}

/// Two Wardens `gap` units apart on X, player 1 west facing east; 30 idle ticks pass so
/// observations may lag.
fn duel(content: &Arc<ZoneContent>, gap: i32) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(content)).unwrap();
    for player in [1, 2] {
        zone.add_player(player).unwrap();
        zone.apply_command(player, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
            .unwrap();
    }
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    for (index, player) in state.players.iter_mut().enumerate() {
        player.position = mmorpg_core::physics::from_units([index as i32 * gap, 90, -2_000]);
        player.combat.level = 10;
        player.combat.abilities.resource = 100;
        player.facing = if index == 0 { EAST } else { WEST };
    }
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(content)).unwrap();
    for _ in 0..30 {
        zone.advance_tick().unwrap();
    }
    zone
}

/// `player` begins a timed swing toward `facing`, sent after observing the tick `late` ticks
/// ago.
fn strike(zone: &mut ZoneSimulation, player: PlayerId, sequence: u32, facing: u16, late: u64) {
    let observed = zone.current_tick() - late;
    zone.apply_observed_command(
        player,
        sequence,
        ZoneCommand::DirectedStrike {
            ability: 1,
            facing,
            aim: MIDDLE,
        },
        observed,
    )
    .unwrap();
}

fn run_east(zone: &mut ZoneSimulation, player: PlayerId, sequence: u32) {
    zone.apply_command(
        player,
        sequence,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: EAST,
        },
    )
    .unwrap();
}

fn health(zone: &ZoneSimulation, player: PlayerId) -> u32 {
    zone.snapshot_for_player(player).unwrap().viewer.health
}

fn swing_ticks(zone: &ZoneSimulation) -> u16 {
    let timed = zone.timed_combat().unwrap();
    timed.startup_ticks() + timed.active_ticks()
}

/// Player 1 swings east at player 2, `gap` units away, who starts running east `flee` ticks
/// after the swing began: whether the swing struck, and its rewind.
fn chase(content: &Arc<ZoneContent>, gap: i32, flee: u16, late: u64) -> (bool, u16) {
    let mut zone = duel(content, gap);
    let before = health(&zone, 2);
    strike(&mut zone, 1, 2, EAST, late);
    let mut rewind = None;
    for tick in 0..swing_ticks(&zone) + 2 {
        if tick == flee {
            run_east(&mut zone, 2, 2);
        }
        zone.advance_tick().unwrap();
        rewind = rewind.or(zone.swing_of(1).map(|swing| swing.rewind));
    }
    (health(&zone, 2) < before, rewind.unwrap())
}

#[test]
fn the_rewind_is_the_measured_lateness_plus_interpolation_within_the_cap() {
    let cap = HitRewind::ARENA_TRIAL.max_ticks();
    assert_eq!(cap, 9);
    assert_eq!(CLIENT_INTERPOLATION_TICKS, 4);
    let content = arena(true);
    for (late, expected) in [(0, 4), (3, 7), (5, cap), (20, cap)] {
        let (_, rewind) = chase(&content, 100, u16::MAX, late);
        assert_eq!(rewind, expected, "late {late}");
    }
    // Without the match option nothing is rewound.
    assert_eq!(chase(&arena(false), 100, u16::MAX, 20).1, 0);
}

#[test]
fn a_rewound_sweep_strikes_a_body_that_ran_out_of_reach_within_the_cap() {
    // Player 2 starts running away late in the wind-up: by the sweep it is out of reach in
    // the present but was in reach a few ticks before.
    let flee = 12;
    assert_eq!(chase(&arena(false), 200, flee, 5), (false, 0));
    assert_eq!(chase(&arena(true), 200, flee, 5), (true, 9));
    // On time (rewind 4, the interpolation alone) is not far enough back.
    assert_eq!(chase(&arena(true), 200, flee, 0), (false, 4));
}

#[test]
fn a_body_out_of_reach_further_back_than_the_cap_is_not_struck() {
    // Running from early in the wind-up, player 2 was last in reach more than the cap ago:
    // however late the strike claims to be, the rewind stops at the cap.
    let flee = 7;
    assert_eq!(chase(&arena(true), 200, flee, 20), (false, 9));
    // A match allowing the longest rewind would have struck it.
    let longest = arena_with(Some(HitRewind {
        max_ms: mmorpg_core::MAX_HIT_REWIND_MS,
    }));
    assert_eq!(chase(&longest, 200, flee, 20), (true, 12));
    // No match may allow more.
    let content = ZoneContent::new(
        support::arena::ground(),
        mmorpg_core::ZoneAreas::default(),
        vec![],
        vec![],
        vec![],
        [0, 0],
    )
    .unwrap();
    assert!(
        content
            .clone()
            .with_hit_rewind(HitRewind::ARENA_TRIAL)
            .is_err(),
        "needs timed swings"
    );
    let timed = content.with_timed_combat(TimedCombat::ARENA).unwrap();
    assert!(timed.with_hit_rewind(HitRewind { max_ms: 250 }).is_err());
}

#[test]
fn clashes_resolve_in_the_present_never_rewound() {
    // Both swing at each other; player 2 runs off during the wind-up. Rewound, the two would
    // still meet, but the clash reads the present: no clash, and only player 1's rewound
    // sweep strikes.
    let content = arena(true);
    let mut zone = duel(&content, 150);
    let before = [health(&zone, 1), health(&zone, 2)];
    strike(&mut zone, 1, 2, EAST, 5);
    strike(&mut zone, 2, 2, WEST, 5);
    let mut clashed = false;
    for tick in 0..swing_ticks(&zone) + 2 {
        if tick == 8 {
            run_east(&mut zone, 2, 3);
        }
        zone.advance_tick().unwrap();
        clashed |= zone
            .snapshot_for_player(1)
            .unwrap()
            .events
            .iter()
            .any(|event| {
                matches!(
                    event,
                    ZoneEvent::Clashed {
                        kind: ClashKind::Swings,
                        ..
                    }
                )
            });
    }
    assert!(!clashed, "no clash from rewound positions");
    assert_eq!(health(&zone, 1), before[0], "player 1 never was in reach");
    assert!(
        health(&zone, 2) < before[1],
        "player 1's rewound sweep struck"
    );

    // Standing their ground, the same swings clash in the present as without rewind.
    let mut zone = duel(&content, 150);
    strike(&mut zone, 1, 2, EAST, 5);
    strike(&mut zone, 2, 2, WEST, 5);
    let mut clashed = false;
    for _ in 0..swing_ticks(&zone) + 2 {
        zone.advance_tick().unwrap();
        clashed |= zone
            .snapshot_for_player(2)
            .unwrap()
            .events
            .iter()
            .any(|event| {
                *event
                    == ZoneEvent::Clashed {
                        source: EntityRef::Player(1),
                        target: EntityRef::Player(2),
                        kind: ClashKind::Swings,
                    }
            });
    }
    assert!(clashed);
    assert_eq!([health(&zone, 1), health(&zone, 2)], before);
}

#[test]
fn a_zone_restored_mid_swing_rewinds_exactly_like_the_original() {
    let content = arena(true);
    let mut zone = duel(&content, 200);
    let before = health(&zone, 2);
    strike(&mut zone, 1, 2, EAST, 5);
    let total = swing_ticks(&zone) + 2;
    for tick in 0..14 {
        if tick == 12 {
            run_east(&mut zone, 2, 2);
        }
        zone.advance_tick().unwrap();
    }
    let snapshot = zone.snapshot().unwrap();
    assert_eq!(
        snapshot.rewind_history.len(),
        9,
        "the history covers the cap"
    );
    assert_eq!(snapshot.players[0].combat.swing.unwrap().rewind, 9);
    // Content without the option refuses the history and the rewound swing.
    assert!(ZoneSimulation::from_snapshot(snapshot.clone(), arena(false)).is_err());
    let mut restored = ZoneSimulation::from_snapshot(snapshot, Arc::clone(&content)).unwrap();
    for _ in 14..total {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert!(
        health(&restored, 2) < before,
        "both struck through the rewind"
    );
}
