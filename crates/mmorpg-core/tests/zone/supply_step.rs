//! The `supply-step` work bounds (#1019, `.performance/contract.json`): the supply step visits each
//! content patrol once and each member of a run under way or at rest once, so its work follows
//! the patrols and never the idle population.
#[path = "../support/supply_step.rs"]
mod workload;

use workload::{SCALE_POINTS, measure};

#[test]
fn supply_work_follows_the_patrols_and_not_the_idle_population() {
    let runs: Vec<_> = SCALE_POINTS
        .iter()
        .map(|&(patrols, idle)| ((patrols, idle), measure(patrols, idle).unwrap()))
        .collect();
    for &((patrols, idle), run) in &runs {
        assert_eq!(run.marching, patrols, "{patrols}/{idle}: {run:?}");
        assert_eq!(run.patrols_visited, patrols, "{patrols}/{idle}: {run:?}");
        // Three members a patrol, each visited once.
        assert_eq!(
            run.members_visited,
            3 * patrols,
            "{patrols}/{idle}: {run:?}"
        );
    }
    // Linear in the patrols, constant in the idle population.
    let at = |point: (usize, usize)| runs.iter().find(|(p, _)| *p == point).unwrap().1;
    for idle in [64, 160] {
        assert_eq!(
            at((4, idle)),
            at((4, 16)),
            "supply work grew with {idle} idle"
        );
    }
}
