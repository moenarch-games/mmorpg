//! The `defence-step` work bounds (#1019, `.performance/contract.json`): in a running defence the
//! raid step visits each assault raider once, checking it against the farmfolk and the barn, and
//! the construction step each of the content's sites once, so neither follows the idle population.
#[path = "../support/defence_step.rs"]
mod workload;

use workload::{SCALE_POINTS, measure};

#[test]
fn defence_work_follows_the_raiders_and_not_the_idle_population() {
    for (raiders, idle) in SCALE_POINTS {
        let run = measure(raiders, idle).unwrap();
        assert!(run.running, "{raiders}/{idle}: {run:?}");
        assert_eq!(
            run.raid_raiders_visited, raiders,
            "{raiders}/{idle}: {run:?}"
        );
        // Greyhaven's two barricades, every tick.
        assert_eq!(run.constructions_visited, 2, "{raiders}/{idle}: {run:?}");
    }
}
