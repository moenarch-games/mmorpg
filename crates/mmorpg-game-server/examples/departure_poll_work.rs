//! Deterministic work samples of `departure-poll` in `.performance/contract.json` (#1152),
//! printed as the `operations` part of `coding-tooling/work-evidence/v1`;
//! `scripts/operation-work.py` adds the envelope. Counters only; nothing is timed.
//!
//! A host's departure polls ([`depart_toward`], as `HandoffHost::process_departures` runs it
//! for each destination): `waiting` players stay due toward a destination whose connects all
//! fail, and `reachable` players are due toward one that answers. Over [`POLLS`] polls the
//! unreachable destination costs at most one connect a poll however many players wait, and
//! the reachable one a connect per player.
use mmorpg_game_server::handoff_schedule::{DepartureSchedule, Reach, depart_toward};
use std::sync::Mutex;

const POLLS: usize = 16;

fn departure_poll(waiting: usize, reachable: usize) -> String {
    let schedule = Mutex::new(DepartureSchedule::default());
    let connects = || {
        schedule
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .connects()
    };
    let mut reachable_left = reachable;
    let (mut unreachable_connects, mut largest_poll, mut reachable_connects) = (0, 0, 0);
    for _ in 0..POLLS {
        schedule
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .begin_poll();
        let before = connects();
        depart_toward(&schedule, Some("down:1"), 0..waiting, |_| {
            Reach::Unreachable
        });
        let tried = connects() - before;
        unreachable_connects += tried;
        largest_poll = largest_poll.max(tried);
        let before = connects();
        depart_toward(&schedule, Some("up:1"), 0..reachable_left, |_| {
            Reach::Reached
        });
        reachable_connects += connects() - before;
        reachable_left = 0;
    }
    format!(
        "{{\"dimensions\":{{\"waiting\":{waiting},\"reachable\":{reachable}}},\"metrics\":{{\"unreachable_connects\":{unreachable_connects},\"largest_poll_unreachable_connects\":{largest_poll},\"reachable_connects\":{reachable_connects}}}}}"
    )
}

fn main() {
    let samples: Vec<_> = [(4, 4), (64, 4), (512, 4), (64, 64)]
        .into_iter()
        .map(|(waiting, reachable)| departure_poll(waiting, reachable))
        .collect();
    println!(
        "{{\"operations\":[{{\"id\":\"departure-poll\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
}
