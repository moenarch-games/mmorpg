//! The `flock-step` work bounds (#1019, `.performance/contract.json`): the flock step visits each
//! content stray once, tests at most five ways for each one a player drives (one on open ground)
//! and visits each prowler of a running watch once, so its work follows the strays and never the
//! idle population.
#[path = "../support/flock_step.rs"]
mod workload;

use workload::{SCALE_POINTS, measure};

#[test]
fn flock_work_follows_the_strays_and_not_the_idle_population() {
    let runs: Vec<_> = SCALE_POINTS
        .iter()
        .map(|&(strays, idle)| ((strays, idle), measure(strays, idle).unwrap()))
        .collect();
    for &((strays, idle), run) in &runs {
        assert_eq!(run.driven, strays, "{strays}/{idle}: {run:?}");
        assert_eq!(run.strays_visited, strays, "{strays}/{idle}: {run:?}");
        // Open ground: the first way tried is clear for every driven stray.
        assert_eq!(run.stray_ways_tested, strays, "{strays}/{idle}: {run:?}");
        assert_eq!(run.prowlers_visited, 2, "{strays}/{idle}: {run:?}");
    }
    // Linear in the strays, constant in the idle population.
    let at = |point: (usize, usize)| runs.iter().find(|(p, _)| *p == point).unwrap().1;
    for idle in [64, 160] {
        assert_eq!(
            at((16, idle)),
            at((16, 16)),
            "flock work grew with {idle} idle"
        );
    }
}
