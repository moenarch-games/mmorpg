//! Dense creature storage with per-system activity (#1086).
//!
//! Creatures live in one `Vec` in ID order: the zone's creatures are its content's spawns, so
//! the store's index of a creature is its spawn's index in [`crate::ZoneContent`], and
//! iteration keeps ID order exactly as the `BTreeMap` it replaces did.
//!
//! Each per-tick system that visits creatures ([`Activity`]) holds one bit per creature: set,
//! the system visits it; clear, the visit is known to change nothing. Every mutable access to a
//! creature sets all of its bits, so a change can never leave a system blind to it; a system
//! clears a creature's bit only after a visit that left it quiet by that system's own rule. A
//! tick therefore visits the creatures that are engaged, casting, holding a timer or awake, and
//! an idle population costs nothing. Visiting a creature whose bit is set needlessly does
//! exactly what visiting every creature did, so the bits are an optimisation, never a rule:
//! [`CreatureStore::set_exhaustive`] visits every creature again, the reference the tests
//! compare against (PRINCIPLE-010).
//!
//! The store carries one revision stamp ([`crate::revisioned`]): a mutable access takes a fresh
//! one, and reads never do, so a tick that changes no creature keeps the threatened-player
//! cache. Neither the bits nor the stamp are canonical state; a recovered zone starts with every
//! bit set.

use crate::CreatureId;
use crate::creature::CreatureState;
use crate::revisioned::fresh_stamp;
use crate::{EntityRef, interest::InterestIndex};

/// A per-tick system that visits creatures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Activity {
    /// Step 2's pace and decisions; also every awake body's interest maintenance and a
    /// large creature's shove, since a body is awake only while this bit is set.
    Decide,
    /// Swing timers, combat timers and blows.
    Swing,
    /// Casts and the ability timer.
    Cast,
    /// Aura countdowns and pulses.
    Auras,
    /// Corpse despawns and respawns.
    Timers,
}

const ACTIVITIES: usize = 5;

impl Activity {
    const fn slot(self) -> usize {
        self as usize
    }
}

pub(crate) struct CreatureStore {
    ids: Vec<CreatureId>,
    states: Vec<CreatureState>,
    /// Where each resting creature sleeps, by index, and the same as a grid (#1086).
    rest_at: Vec<Option<[i32; 2]>>,
    resting: InterestIndex,
    stamp: u64,
    /// One bit per creature, by index, for each [`Activity`].
    active: [Vec<u64>; ACTIVITIES],
    /// Every creature visits every system, as before #1086.
    exhaustive: bool,
}

impl Default for CreatureStore {
    fn default() -> Self {
        Self {
            ids: Vec::new(),
            states: Vec::new(),
            rest_at: Vec::new(),
            resting: InterestIndex::default(),
            stamp: fresh_stamp(),
            active: Default::default(),
            exhaustive: false,
        }
    }
}

impl CreatureStore {
    pub(crate) fn len(&self) -> usize {
        self.ids.len()
    }

    fn index_of(&self, id: CreatureId) -> Option<usize> {
        self.ids.binary_search(&id).ok()
    }

    pub(crate) fn get(&self, id: &CreatureId) -> Option<&CreatureState> {
        self.index_of(*id).map(|index| &self.states[index])
    }

    pub(crate) fn contains_key(&self, id: &CreatureId) -> bool {
        self.index_of(*id).is_some()
    }

    /// A creature to change: it takes a fresh stamp and every system looks at it again.
    pub(crate) fn get_mut(&mut self, id: &CreatureId) -> Option<&mut CreatureState> {
        let index = self.index_of(*id)?;
        self.touch(index);
        Some(&mut self.states[index])
    }

    fn touch(&mut self, index: usize) {
        self.stamp = fresh_stamp();
        let (word, bit) = (index / 64, 1 << (index % 64));
        for bits in &mut self.active {
            bits[word] |= bit;
        }
    }

    /// Adds or replaces creature `id`; a new ID sets every creature's bits.
    pub(crate) fn insert(&mut self, id: CreatureId, state: CreatureState) {
        match self.ids.binary_search(&id) {
            Ok(index) => {
                self.states[index] = state;
                self.touch(index);
            }
            Err(index) => {
                self.ids.insert(index, id);
                self.states.insert(index, state);
                self.rest_at.insert(index, None);
                self.stamp = fresh_stamp();
                self.set_every_bit();
            }
        }
    }

    /// Every system visits every creature next time.
    fn set_every_bit(&mut self) {
        let (len, words) = (self.ids.len(), self.ids.len().div_ceil(64));
        for bits in &mut self.active {
            bits.clear();
            bits.resize(words, u64::MAX);
            // No bit beyond the last creature.
            if let Some(last) = bits.last_mut()
                && len % 64 != 0
            {
                *last = (1 << (len % 64)) - 1;
            }
        }
    }

    #[cfg_attr(not(feature = "physics-ab"), allow(dead_code))]
    pub(crate) fn keys(&self) -> std::slice::Iter<'_, CreatureId> {
        self.ids.iter()
    }

    pub(crate) fn values(&self) -> std::slice::Iter<'_, CreatureState> {
        self.states.iter()
    }

    pub(crate) fn iter(&self) -> Iter<'_> {
        self.ids.iter().zip(self.states.iter())
    }

    /// Changes, in ID order, only the creatures `matches` selects, so the others keep their
    /// quiet and the store its stamp when none matches.
    pub(crate) fn update_where(
        &mut self,
        matches: impl Fn(&CreatureState) -> bool,
        mut change: impl FnMut(&mut CreatureState),
    ) {
        for index in 0..self.states.len() {
            if matches(&self.states[index]) {
                self.touch(index);
                change(&mut self.states[index]);
            }
        }
    }

    /// [`Self::update_where`] over the creatures `activity` visits: for a change that only
    /// concerns creatures known to hold that bit.
    pub(crate) fn update_active_where(
        &mut self,
        activity: Activity,
        matches: impl Fn(&CreatureState) -> bool,
        mut change: impl FnMut(&mut CreatureState),
    ) {
        let mut after = None;
        while let Some(id) = self.next_active(activity, after) {
            after = Some(id);
            if let Some(index) = self.index_of(id)
                && matches(&self.states[index])
            {
                self.touch(index);
                change(&mut self.states[index]);
            }
        }
    }

    /// The stamp of the current creatures.
    pub(crate) fn revision(&self) -> u64 {
        self.stamp
    }

    /// The first creature after `after` (from the start for `None`), in ID order, that
    /// `activity` visits. Asked again after each visit, it sees bits set meanwhile.
    pub(crate) fn next_active(
        &self,
        activity: Activity,
        after: Option<CreatureId>,
    ) -> Option<CreatureId> {
        let start = after.map_or(0, |after| self.ids.partition_point(|&id| id <= after));
        if self.exhaustive {
            return self.ids.get(start).copied();
        }
        let bits = &self.active[activity.slot()];
        let mut word = start / 64;
        let mut mask = u64::MAX.checked_shl(u32::try_from(start % 64).ok()?)?;
        while let Some(&value) = bits.get(word) {
            let set = value & mask;
            if set != 0 {
                let index = word * 64 + set.trailing_zeros() as usize;
                return self.ids.get(index).copied();
            }
            word += 1;
            mask = u64::MAX;
        }
        None
    }

    /// The creatures `activity` visits now, in ID order.
    pub(crate) fn active(&self, activity: Activity) -> impl Iterator<Item = CreatureId> + '_ {
        let mut after = None;
        std::iter::from_fn(move || {
            let id = self.next_active(activity, after)?;
            after = Some(id);
            Some(id)
        })
    }

    /// `activity` need not visit creature `id` again until it changes.
    pub(crate) fn settle(&mut self, activity: Activity, id: CreatureId) {
        if let Some(index) = self.index_of(id) {
            self.active[activity.slot()][index / 64] &= !(1 << (index % 64));
        }
    }

    /// `activity` visits creature `id` next time, although it did not change.
    pub(crate) fn wake(&mut self, activity: Activity, id: CreatureId) {
        if let Some(index) = self.index_of(id) {
            self.active[activity.slot()][index / 64] |= 1 << (index % 64);
        }
    }

    /// How many creatures `activity` would visit now; workload evidence only.
    pub(crate) fn active_count(&self, activity: Activity) -> usize {
        if self.exhaustive {
            return self.len();
        }
        self.active[activity.slot()]
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    /// With `on`, every system visits every creature every tick, as before #1086.
    pub(crate) fn set_exhaustive(&mut self, on: bool) {
        self.exhaustive = on;
    }

    /// Records where creature `id` rests asleep, or with `None` that it does not.
    pub(crate) fn rest(&mut self, id: CreatureId, at: Option<[i32; 2]>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        if self.rest_at[index] == at {
            return;
        }
        self.rest_at[index] = at;
        let entity = EntityRef::Creature(id);
        match at {
            Some([x, z]) => {
                self.resting.move_if_needed(entity, x, z);
            }
            None => {
                self.resting.remove(entity);
            }
        }
    }

    /// Where creature `id` rests asleep, if it does.
    pub(crate) fn rest_at(&self, id: CreatureId) -> Option<[i32; 2]> {
        self.index_of(id).and_then(|index| self.rest_at[index])
    }

    /// Queries of the resting grid that outgrew their buffer.
    pub(crate) fn resting_allocations(&self) -> usize {
        self.resting.allocations()
    }

    /// The resting creatures in a cell that the box `x` by `z` touches, into `out`.
    pub(crate) fn resting_in(&self, x: [i64; 2], z: [i64; 2], out: &mut Vec<EntityRef>) {
        self.resting.box_into(x, z, out);
    }
}

pub(crate) type Iter<'a> =
    std::iter::Zip<std::slice::Iter<'a, CreatureId>, std::slice::Iter<'a, CreatureState>>;

impl<'a> IntoIterator for &'a CreatureStore {
    type Item = (&'a CreatureId, &'a CreatureState);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl PartialEq for CreatureStore {
    fn eq(&self, other: &Self) -> bool {
        self.ids == other.ids && self.states == other.states
    }
}

impl Eq for CreatureStore {}

impl std::fmt::Debug for CreatureStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_map().entries(self.iter()).finish()
    }
}

/// Buffers the creature tick reuses, so a warm tick allocates nothing for them (#1086).
#[derive(Default)]
pub(crate) struct CreatureScratch {
    pub(crate) bodies: Vec<crate::physics::BodyId>,
    pub(crate) cells: Vec<(i64, i64)>,
    pub(crate) near: Vec<EntityRef>,
}
