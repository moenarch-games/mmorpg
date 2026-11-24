//! Sessions waiting to claim a character, by the tick their wait ends (#1141).
//!
//! Live host state only: never canonical, rebuilt on restore. Expiry visits only the
//! sessions whose wait has ended, so a tick without one costs nothing.

use game_server::PlayerId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub(crate) struct ClaimWaits {
    deadlines: BTreeMap<PlayerId, u64>,
    by_deadline: BTreeSet<(u64, PlayerId)>,
    /// Entries visited by [`ClaimWaits::expired`], cumulative; a work counter.
    visits: AtomicU64,
}

impl ClaimWaits {
    pub(crate) fn len(&self) -> usize {
        self.deadlines.len()
    }

    pub(crate) fn insert(&mut self, player: PlayerId, deadline: u64) {
        self.remove(&player);
        self.deadlines.insert(player, deadline);
        self.by_deadline.insert((deadline, player));
    }

    pub(crate) fn remove(&mut self, player: &PlayerId) {
        if let Some(deadline) = self.deadlines.remove(player) {
            self.by_deadline.remove(&(deadline, *player));
        }
    }

    pub(crate) fn clear(&mut self) {
        self.deadlines.clear();
        self.by_deadline.clear();
    }

    /// The sessions whose wait ended at or before `now`, by deadline.
    pub(crate) fn expired(&self, now: u64) -> Vec<PlayerId> {
        let expired: Vec<_> = self
            .by_deadline
            .range(..(now.saturating_add(1), 0))
            .map(|&(_, player)| player)
            .collect();
        self.visits
            .fetch_add(expired.len() as u64, Ordering::Relaxed);
        expired
    }

    pub(crate) fn visits(&self) -> u64 {
        self.visits.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_visits_only_ended_waits() {
        let mut waits = ClaimWaits::default();
        waits.insert(3, 10);
        waits.insert(1, 20);
        waits.insert(2, 10);
        assert!(waits.expired(9).is_empty());
        assert_eq!(waits.visits(), 0);
        assert_eq!(waits.expired(10), vec![2, 3]);
        assert_eq!(waits.visits(), 2);
        waits.remove(&2);
        waits.insert(3, 30);
        assert_eq!(waits.expired(25), vec![1]);
        assert_eq!(waits.len(), 2);
    }
}
