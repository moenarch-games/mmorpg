//! Which departures a handoff host attempts in one poll (#1152).
//!
//! A host groups each poll's departures by the destination host's transfer
//! link and runs the groups concurrently, each in order
//! ([`crate::handoff_host::HandoffHost::process_departures`]). Within a
//! group, the first departure whose connect fails ends the group for this
//! poll: the rest stay due and wait, live, for a later poll. A link that
//! keeps failing backs off by polls, deterministically: it is tried again on
//! the next poll after its first failure, then after 2, 4 … up to
//! [`MAX_BACKOFF_POLLS`] polls, and a successful connect forgets it.
//!
//! Host state only, never canonical: it decides when to try, never who owns
//! a character. Every attempt is fenced by the handoff itself. The same holds
//! for [`ArrivalDeadlines`], which only decides when a destination gives up
//! waiting for an arrival's client.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Instant;

/// The most polls an unreachable destination waits before it is tried again.
pub const MAX_BACKOFF_POLLS: u64 = 32;

/// What one departure attempt learned about its destination's link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reach {
    /// It did not connect: refused before the offer, or a local destination.
    Untried,
    /// The destination answered, whatever it answered.
    Reached,
    /// The connect failed.
    Unreachable,
}

#[derive(Clone, Copy, Debug)]
struct Backoff {
    /// Consecutive polls whose connect failed.
    failures: u32,
    /// The first poll that tries the link again.
    retry_at: u64,
}

/// The polls so far and the links that failed to connect.
#[derive(Debug, Default)]
pub struct DepartureSchedule {
    poll: u64,
    backoff: BTreeMap<String, Backoff>,
    /// Departures attempted toward a link (connects), cumulative; a work counter.
    connects: u64,
}

impl DepartureSchedule {
    /// Starts the next poll.
    pub fn begin_poll(&mut self) {
        self.poll += 1;
    }

    /// Whether departures toward `link` are attempted this poll.
    #[must_use]
    pub fn due(&self, link: &str) -> bool {
        self.backoff
            .get(link)
            .is_none_or(|backoff| self.poll >= backoff.retry_at)
    }

    /// Connects attempted so far, toward every link.
    #[must_use]
    pub const fn connects(&self) -> u64 {
        self.connects
    }

    fn record(&mut self, link: &str, reach: Reach) {
        match reach {
            Reach::Untried => return,
            Reach::Reached => {
                self.backoff.remove(link);
            }
            Reach::Unreachable => {
                let backoff = self.backoff.entry(link.to_owned()).or_insert(Backoff {
                    failures: 0,
                    retry_at: 0,
                });
                backoff.failures = backoff.failures.saturating_add(1);
                let wait = 1_u64
                    .checked_shl(backoff.failures - 1)
                    .unwrap_or(u64::MAX)
                    .min(MAX_BACKOFF_POLLS);
                backoff.retry_at = self.poll + wait;
            }
        }
        self.connects += 1;
    }
}

/// Attempts one poll's departures toward `link` (`None`: this host's own
/// zones, which need no connect) in order with `depart`, until one finds the
/// link unreachable; the rest wait for a later poll. Nothing is attempted
/// while the link backs off.
pub fn depart_toward<T>(
    schedule: &Mutex<DepartureSchedule>,
    link: Option<&str>,
    departures: impl IntoIterator<Item = T>,
    mut depart: impl FnMut(T) -> Reach,
) {
    let schedule = |body: &mut dyn FnMut(&mut DepartureSchedule)| {
        body(
            &mut schedule
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    };
    if let Some(link) = link {
        let mut due = true;
        schedule(&mut |state| due = state.due(link));
        if !due {
            return;
        }
    }
    for departure in departures {
        let reach = depart(departure);
        if let Some(link) = link {
            schedule(&mut |state| state.record(link, reach));
        }
        if reach == Reach::Unreachable {
            return;
        }
    }
}

/// Accepted arrivals by the deadline their client has to claim them (#1146): a destination
/// takes the due ones each departure poll and visits only those, never the arrivals still in
/// time nor the ones already claimed ([`Self::remove`]).
#[derive(Debug)]
pub struct ArrivalDeadlines<K> {
    due: BTreeSet<(Instant, K)>,
    visits: u64,
}

impl<K> Default for ArrivalDeadlines<K> {
    fn default() -> Self {
        Self {
            due: BTreeSet::new(),
            visits: 0,
        }
    }
}

impl<K: Ord + Copy> ArrivalDeadlines<K> {
    pub fn insert(&mut self, deadline: Instant, arrival: K) {
        self.due.insert((deadline, arrival));
    }

    /// Forgets a claimed or discarded arrival.
    pub fn remove(&mut self, deadline: Instant, arrival: K) {
        self.due.remove(&(deadline, arrival));
    }

    /// Takes every arrival whose deadline passed by `now`, earliest first.
    pub fn take_due(&mut self, now: Instant) -> Vec<K> {
        let mut taken = Vec::new();
        while let Some(&(deadline, arrival)) = self.due.first() {
            if deadline > now {
                break;
            }
            self.due.pop_first();
            self.visits += 1;
            taken.push(arrival);
        }
        taken
    }

    /// Arrivals taken so far, for `arrival-expiry` in `.performance/contract.json`.
    #[must_use]
    pub const fn visits(&self) -> u64 {
        self.visits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrivals_come_due_in_deadline_order_and_claimed_ones_never_do() {
        let start = Instant::now();
        let at = |seconds| start + std::time::Duration::from_secs(seconds);
        let mut deadlines = ArrivalDeadlines::default();
        deadlines.insert(at(20), 2);
        deadlines.insert(at(10), 1);
        deadlines.insert(at(15), 3);
        deadlines.remove(at(15), 3);
        assert!(deadlines.take_due(at(9)).is_empty());
        assert_eq!(deadlines.take_due(at(30)), [1, 2]);
        assert_eq!(deadlines.visits(), 2);
    }

    /// Polls `players` departures toward one link whose connects all fail;
    /// the connects each poll made.
    fn unreachable_polls(players: usize, polls: usize) -> Vec<u64> {
        let schedule = Mutex::new(DepartureSchedule::default());
        (0..polls)
            .map(|_| {
                schedule.lock().unwrap().begin_poll();
                let before = schedule.lock().unwrap().connects();
                depart_toward(&schedule, Some("down:1"), 0..players, |_| {
                    Reach::Unreachable
                });
                schedule.lock().unwrap().connects() - before
            })
            .collect()
    }

    #[test]
    fn an_unreachable_link_is_tried_once_a_poll_then_backs_off() {
        let expected = [1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1];
        assert_eq!(unreachable_polls(1, expected.len()), expected);
        assert_eq!(
            unreachable_polls(50, expected.len()),
            expected,
            "however many players wait"
        );
    }

    #[test]
    fn a_reached_link_forgets_its_failures_and_untried_departures_count_nothing() {
        let schedule = Mutex::new(DepartureSchedule::default());
        for _ in 0..2 {
            schedule.lock().unwrap().begin_poll();
            depart_toward(&schedule, Some("flaky:1"), [()], |()| Reach::Unreachable);
        }
        schedule.lock().unwrap().begin_poll();
        assert!(!schedule.lock().unwrap().due("flaky:1"));
        schedule.lock().unwrap().begin_poll();
        let mut attempted = 0;
        depart_toward(&schedule, Some("flaky:1"), 0..3, |_| {
            attempted += 1;
            Reach::Reached
        });
        assert_eq!(attempted, 3, "a reached link runs every departure");
        assert!(schedule.lock().unwrap().due("flaky:1"));
        let before = schedule.lock().unwrap().connects();
        depart_toward(&schedule, Some("flaky:1"), 0..3, |_| Reach::Untried);
        depart_toward(&schedule, None, 0..3, |_| Reach::Unreachable);
        assert_eq!(schedule.lock().unwrap().connects(), before);
    }
}
