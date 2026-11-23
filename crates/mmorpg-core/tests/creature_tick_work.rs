//! The `creature-tick` work bounds (#1086, `.performance/contract.json`): creature visits follow
//! the active creatures, never the idle population; the tick's interest queries reuse warm
//! buffers; a warm tick allocates the same whatever the idle population.
#[path = "support/creature_tick.rs"]
mod creature_tick;

use creature_tick::{SCALE_POINTS, measure, warm_zone};

#[test]
fn creature_visits_follow_the_active_creatures_and_warm_ticks_allocate_a_constant_amount() {
    let runs: Vec<_> = SCALE_POINTS
        .iter()
        .map(|&(idle, active)| ((idle, active), measure(idle, active).unwrap()))
        .collect();
    for &((idle, active), run) in &runs {
        assert_eq!(
            run.interest_query_allocations, 0,
            "{idle}/{active}: {run:?}"
        );
        // Each fighter is visited by a bounded number of systems, the sleepers by none.
        assert!(
            run.creatures_visited <= 5 * active,
            "{idle}/{active}: {run:?}"
        );
        assert!(run.creatures_visited >= active, "{idle}/{active}: {run:?}");
        assert_eq!(
            run.awake, active,
            "{idle}/{active}: only fighters stay awake"
        );
    }
    // Constant in the idle population at a fixed active count.
    let at = |point: (usize, usize)| runs.iter().find(|(p, _)| *p == point).unwrap().1;
    for idle in [128, 256] {
        assert_eq!(
            at((idle, 4)).creatures_visited,
            at((64, 4)).creatures_visited
        );
        assert_eq!(
            at((idle, 4)).warm_tick_allocations,
            at((64, 4)).warm_tick_allocations,
            "warm tick allocations grew with {idle} sleepers"
        );
    }
}

#[test]
fn an_idle_tick_visits_no_creature_and_keeps_the_threat_cache() {
    let mut zone = warm_zone(256, 0).unwrap();
    zone.snapshot_for_player(1).unwrap();
    let derivations = zone.threat_derivations();
    for _ in 0..8 {
        zone.advance_tick().unwrap();
        assert_eq!(zone.tick_work().creatures_visited, 0);
        zone.snapshot_for_player(1).unwrap();
    }
    assert_eq!(
        zone.threat_derivations(),
        derivations,
        "a tick that changes no creature keeps the threatened-player set"
    );
}
