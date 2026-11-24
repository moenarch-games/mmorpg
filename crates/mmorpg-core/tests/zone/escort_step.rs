//! The `escort-step` work bounds (#1019, `.performance/contract.json`): the escort step visits
//! each running escort once and examines a step's worth of its trail, so its work follows the
//! escorts and never the idle population; the confrontation check is the content's, constant.
#[path = "../support/escort_step.rs"]
mod workload;

use workload::{SCALE_POINTS, measure};

#[test]
fn escort_work_follows_the_running_escorts_and_not_the_idle_population() {
    let runs: Vec<_> = SCALE_POINTS
        .iter()
        .map(|&(escorts, idle)| ((escorts, idle), measure(escorts, idle).unwrap()))
        .collect();
    for &((escorts, idle), run) in &runs {
        assert_eq!(run.running, escorts, "{escorts}/{idle}: {run:?}");
        assert_eq!(run.escorts_advanced, escorts, "{escorts}/{idle}: {run:?}");
        // Every follower limps toward its player: at least one trail point, at most two a step.
        assert!(
            (escorts..=2 * escorts).contains(&run.escort_trail_points),
            "{escorts}/{idle}: {run:?}"
        );
        assert_eq!(run.confrontations_checked, 1, "{escorts}/{idle}: {run:?}");
    }
    // Linear in the escorts with a constant rate, constant in the idle population.
    let at = |point: (usize, usize)| runs.iter().find(|(p, _)| *p == point).unwrap().1;
    let rate = at((4, 16)).escort_trail_points / 4;
    for escorts in [16, 64] {
        assert_eq!(at((escorts, 16)).escort_trail_points, rate * escorts);
    }
    for idle in [64, 160] {
        assert_eq!(
            at((16, idle)),
            at((16, 16)),
            "escort work grew with {idle} idle"
        );
    }
}
