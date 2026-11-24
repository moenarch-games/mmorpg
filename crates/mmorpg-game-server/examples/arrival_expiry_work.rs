//! Deterministic work samples of `arrival-expiry` in `.performance/contract.json` (#1146),
//! printed as the `operations` part of `coding-tooling/work-evidence/v1`;
//! `scripts/operation-work.py` adds the envelope. Counters only; nothing is timed.
//!
//! A destination's arrival deadlines over every departure poll of the claim timeout
//! ([`ArrivalDeadlines`], as `HandoffHost::process_departures` expires them): `claimed` arrivals
//! are claimed in time and `unclaimed` ones never are, all accepted on one poll. Each poll takes
//! only the arrivals due, so the deadlines are visited once per unclaimed arrival, on the poll
//! its deadline passes, and never for the claimed ones.
use mmorpg_game_server::handoff_host::ARRIVAL_CLAIM_TIMEOUT;
use mmorpg_game_server::handoff_schedule::ArrivalDeadlines;
use std::time::{Duration, Instant};

/// The departure poll (`handoff_host::DEPARTURE_POLL`).
const POLL: Duration = Duration::from_millis(50);

fn arrival_expiry(claimed: u64, unclaimed: u64) -> String {
    let start = Instant::now();
    let deadline = start + ARRIVAL_CLAIM_TIMEOUT;
    let mut deadlines = ArrivalDeadlines::default();
    for arrival in 0..claimed + unclaimed {
        deadlines.insert(deadline, arrival);
    }
    for arrival in 0..claimed {
        deadlines.remove(deadline, arrival);
    }
    let polls = ARRIVAL_CLAIM_TIMEOUT.as_millis() / POLL.as_millis() + 2;
    let mut expired = 0;
    for poll in 0..polls {
        let now = start + POLL * u32::try_from(poll).expect("a few thousand polls");
        expired += deadlines.take_due(now).len();
    }
    assert_eq!(
        u64::try_from(expired).ok(),
        Some(unclaimed),
        "every unclaimed arrival expires once"
    );
    format!(
        "{{\"dimensions\":{{\"claimed\":{claimed},\"unclaimed\":{unclaimed}}},\"metrics\":{{\"arrival_deadlines_visited\":{}}}}}",
        deadlines.visits()
    )
}

fn main() {
    let samples: Vec<_> = [(16, 4), (256, 4), (4_096, 4), (256, 64)]
        .into_iter()
        .map(|(claimed, unclaimed)| arrival_expiry(claimed, unclaimed))
        .collect();
    println!(
        "{{\"operations\":[{{\"id\":\"arrival-expiry\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
}
