//! The `carry-step` work bounds (#1077, `.performance/contract.json`): the carry step visits each
//! load once, so its work follows the carriers and never the empty-handed players.
#[path = "../support/carry_step.rs"]
mod workload;

use workload::{SCALE_POINTS, measure};

#[test]
fn carry_work_follows_the_loads_and_not_the_empty_handed() {
    for (carriers, idle) in SCALE_POINTS {
        let run = measure(carriers, idle).unwrap();
        assert_eq!(run.carrying, carriers, "{carriers}/{idle}: {run:?}");
        assert_eq!(run.loads_visited, carriers, "{carriers}/{idle}: {run:?}");
    }
}
