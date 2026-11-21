//! The threatened-player set shared by every recipient projection (#1006).
//!
//! [`ZoneSimulation::threatened_players`] scans every creature's threat table. Projecting for P
//! recipients used to repeat that scan P times for one unchanged state. The set is now derived
//! once and reused until a contributing collection changes.
//!
//! Invalidation is structural, not tick-based: the set reads only `creatures` (life and threat
//! tables) and `minions` (summoners), and both live in a [`Revisioned`] wrapper whose every
//! mutable access bumps its revision. A cached set is reused only while both revisions match
//! the ones it was derived from, so any phase, admission, removal or in-place recovery that can
//! change a contributor refreshes it; a zone built from a checkpoint starts with an empty cache.
//! The cache is never canonical state and never feeds back into gameplay.

use std::collections::BTreeSet;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

use crate::{EntityRef, PlayerId, ZoneSimulation};

/// A collection whose every mutable access bumps a revision, so derived caches can tell
/// whether it may have changed. Reads go through [`Deref`] without touching the revision.
#[derive(Debug, Default)]
pub(crate) struct Revisioned<T> {
    value: T,
    revision: u64,
}

impl<T> Revisioned<T> {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}

impl<T> Deref for Revisioned<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Revisioned<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.revision = self.revision.wrapping_add(1);
        &mut self.value
    }
}

/// The derived set and the contributor revisions it was derived from.
#[derive(Debug, Default)]
pub(crate) struct ThreatCache {
    entry: Mutex<Option<CachedThreat>>,
    /// How many times the set was derived rather than reused; workload evidence only.
    derivations: std::sync::atomic::AtomicU64,
}

#[derive(Debug)]
struct CachedThreat {
    creatures: u64,
    minions: u64,
    players: Arc<BTreeSet<PlayerId>>,
}

impl ZoneSimulation {
    /// Players on any living creature's threat table, themselves or through a minion (#295).
    /// Derived once per unchanged creature/minion state and shared by every caller (#1006).
    pub(crate) fn threatened_players(&self) -> Arc<BTreeSet<PlayerId>> {
        let creatures = self.creatures.revision();
        let minions = self.minions.revision();
        let mut entry = self
            .threat_cache
            .entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = entry.as_ref()
            && cached.creatures == creatures
            && cached.minions == minions
        {
            return Arc::clone(&cached.players);
        }
        let players = Arc::new(self.derive_threatened_players());
        self.threat_cache
            .derivations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *entry = Some(CachedThreat {
            creatures,
            minions,
            players: Arc::clone(&players),
        });
        players
    }

    /// How many times the threatened-player set was derived instead of reused (#1006).
    /// Deterministic for a given call sequence; workload evidence, never gameplay.
    pub fn threat_derivations(&self) -> u64 {
        self.threat_cache
            .derivations
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The uncached scan behind [`Self::threatened_players`].
    pub(crate) fn derive_threatened_players(&self) -> BTreeSet<PlayerId> {
        self.creatures
            .values()
            .filter(|creature| creature.is_alive())
            .flat_map(|creature| &creature.threat)
            .filter_map(|entry| match entry.entity {
                EntityRef::Player(player_id) => Some(player_id),
                unit @ EntityRef::Minion(_) => self.controller(unit),
                EntityRef::Creature(_) | EntityRef::Npc(_) => None,
            })
            .collect()
    }
}

impl<'a, T> IntoIterator for &'a Revisioned<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.value.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Revisioned<T>
where
    &'a mut T: IntoIterator,
{
    type Item = <&'a mut T as IntoIterator>::Item;
    type IntoIter = <&'a mut T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.deref_mut().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ZoneId;

    /// Every recipient's projection, each derived from a fresh set as before #1006.
    fn per_recipient(zone: &mut ZoneSimulation, players: &[PlayerId]) -> Vec<crate::ZoneSnapshot> {
        players
            .iter()
            .map(|&player| {
                zone.threat_cache = ThreatCache::default();
                zone.snapshot_for_player(player).unwrap()
            })
            .collect()
    }

    fn shared(zone: &ZoneSimulation, players: &[PlayerId]) -> Vec<crate::ZoneSnapshot> {
        players
            .iter()
            .map(|&player| zone.snapshot_for_player(player).unwrap())
            .collect()
    }

    #[test]
    fn recipients_share_one_derivation_with_identical_projections() {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), crate::content::greyhaven_vale::content())
                .unwrap();
        let players: Vec<PlayerId> = (1..=4).collect();
        for &player in &players {
            zone.add_player(player).unwrap();
        }
        // A living creature far outside every viewer's interest still marks its foe threatened.
        let distant = zone
            .creatures
            .iter()
            .filter(|(_, creature)| creature.is_alive())
            .map(|(&id, _)| id)
            .last()
            .expect("the zone has a living creature");
        zone.creatures
            .get_mut(&distant)
            .unwrap()
            .add_threat(EntityRef::Player(players[1]), 5);

        let expected = per_recipient(&mut zone, &players);
        zone.threat_cache = ThreatCache::default();
        assert_eq!(shared(&zone, &players), expected);
        assert_eq!(zone.threat_derivations(), 1, "one scan for every recipient");
        assert!(zone.threatened_players().contains(&players[1]));

        // A threat change refreshes the shared set; recipients still match fresh derivation.
        zone.creatures.get_mut(&distant).unwrap().threat.clear();
        let after = shared(&zone, &players);
        assert_eq!(zone.threat_derivations(), 2);
        assert!(zone.threatened_players().is_empty());
        assert_eq!(after, per_recipient(&mut zone, &players));

        // Recovery builds a zone with its own, empty cache.
        let restored =
            ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(&zone.content))
                .unwrap();
        assert_eq!(restored.threat_derivations(), 0);
        assert_eq!(shared(&restored, &players), after);
        assert_eq!(restored.threat_derivations(), 1);
    }
}
