//! The `seal-step` work bounds (#1019, `.performance/contract.json`): the mechanism step visits
//! the content's seals and wards once each tick, and an open seal due to close asks one overlap
//! query of its chamber, whose bodies follow the players inside it, never the idle population.
#[path = "../support/seal_step.rs"]
mod workload;

use workload::{SCALE_POINTS, measure};

#[test]
fn seal_work_follows_the_vaults_occupants_and_not_the_idle_population() {
    let base = measure(SCALE_POINTS[0].0, SCALE_POINTS[0].1).unwrap();
    for (occupants, idle) in SCALE_POINTS {
        let run = measure(occupants, idle).unwrap();
        assert!(run.open, "{occupants}/{idle}: the occupants hold it open");
        // Greyhaven's two seals (the shrine's and the mill sluice, #1019) and its ward, every tick.
        assert_eq!(run.mechanisms_visited, 3, "{occupants}/{idle}: {run:?}");
        // The chamber's own bodies, then one per occupant.
        assert_eq!(
            run.seal_bodies_checked,
            base.seal_bodies_checked + occupants - SCALE_POINTS[0].0,
            "{occupants}/{idle}: {run:?}"
        );
    }
}
