//! The `crossing-step` work bounds (#1019, `.performance/contract.json`): the crossing step visits
//! each content crossing once and each raider of a crossing under way or settling once, so its
//! work follows the crossings and never the idle population.
#[path = "../support/crossing_step.rs"]
mod workload;

use workload::{SCALE_POINTS, measure};

#[test]
fn crossing_work_follows_the_crossings_and_not_the_idle_population() {
    let runs: Vec<_> = SCALE_POINTS
        .iter()
        .map(|&(crossings, idle)| ((crossings, idle), measure(crossings, idle).unwrap()))
        .collect();
    for &((crossings, idle), run) in &runs {
        assert_eq!(run.rolling, crossings, "{crossings}/{idle}: {run:?}");
        assert_eq!(
            run.crossings_visited, crossings,
            "{crossings}/{idle}: {run:?}"
        );
        // Three raiders a crossing, each visited once.
        assert_eq!(
            run.raiders_visited,
            3 * crossings,
            "{crossings}/{idle}: {run:?}"
        );
    }
    // Linear in the crossings, constant in the idle population.
    let at = |point: (usize, usize)| runs.iter().find(|(p, _)| *p == point).unwrap().1;
    for idle in [64, 160] {
        assert_eq!(
            at((4, idle)),
            at((4, 16)),
            "crossing work grew with {idle} idle"
        );
    }
}
