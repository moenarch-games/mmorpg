//! The `mover-pairs` work bounds (#1149, `.performance/contract.json`): a walker's slide-around
//! examines only the movers near it, so a tick's pair work grows linearly with walkers at a fixed
//! density and not at all with asleep bodies away from them.
#[path = "support/mover_pairs.rs"]
mod mover_pairs;

use mover_pairs::{SCALE_POINTS, measure};

/// The most bodies one walker in the workload's grid (100 units apart) examines: its own and the
/// neighbouring cells' walkers, never the whole zone.
const PER_WALKER: usize = 9;

#[test]
fn pair_work_is_linear_in_walkers_and_constant_in_sleepers() {
    for (walkers, sleepers) in SCALE_POINTS {
        let run = measure(walkers, sleepers).unwrap();
        assert_eq!(
            run.moved, walkers,
            "{walkers}/{sleepers}: every walker walks"
        );
        assert!(
            run.tick_pairs >= walkers && run.tick_pairs <= PER_WALKER * walkers,
            "{walkers}/{sleepers}: {run:?}"
        );
        // Asleep bodies away from the walkers add nothing.
        assert_eq!(run, measure(walkers, 0).unwrap(), "{walkers}/{sleepers}");
    }
}
