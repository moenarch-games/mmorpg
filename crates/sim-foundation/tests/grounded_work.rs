//! The `grounded-reads` work bounds (#1148, `.performance/contract.json`): a steady tick answers
//! every grounded read from the moves' own support tests, and a tick whose static geometry
//! changed after the moves casts at most once per unit.
#[path = "support/grounded_reads.rs"]
mod grounded_reads;

use grounded_reads::{SCALE_POINTS, measure};

#[test]
fn steady_ticks_cast_nothing_and_a_geometry_change_casts_once_per_unit() {
    for (units, ticks) in SCALE_POINTS {
        let run = measure(units, ticks).unwrap();
        assert_eq!(run.steady_tick_casts, 0, "{units}/{ticks}: {run:?}");
        assert!(
            run.geometry_change_casts > 0 && run.geometry_change_casts <= units as u64,
            "{units}/{ticks}: {run:?}"
        );
        assert_eq!(run.grounded, units, "{units}/{ticks}: every unit stands");
        // Every read but the change tick's casts came from the cache.
        let reads = units as u64 * (ticks as u64 + 2);
        assert_eq!(
            run.cached_reads + run.geometry_change_casts,
            reads,
            "{units}/{ticks}: {run:?}"
        );
    }
}
