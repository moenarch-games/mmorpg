//! Revision stamps for derived caches (#1006, #1056).
//!
//! A [`Revisioned`] value carries a stamp that every mutable access replaces with a fresh one
//! from a process-wide counter, so two equal stamps always mean the same value with no
//! mutable access in between: a derived cache (the threatened-player set, the zone-wide views,
//! a viewer's encoded sheets) may reuse what it derived from a stamp while the stamp matches.
//! Stamps are unique across values, zones and recovery, so a replaced or restored character
//! never shares a stamp with the one it replaced. A clone keeps its stamp, since it holds the
//! same value. Stamps are never canonical state, never travel and never feed back into
//! gameplay; equality and debug output see the value alone.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

/// The next unused stamp; `0` stays reserved for "none held".
static NEXT_STAMP: AtomicU64 = AtomicU64::new(1);

/// A stamp no value held before.
pub(crate) fn fresh_stamp() -> u64 {
    NEXT_STAMP.fetch_add(1, Ordering::Relaxed)
}

/// A value whose every mutable access takes a fresh stamp. Reads go through [`Deref`] without
/// touching it.
#[derive(Clone)]
pub(crate) struct Revisioned<T> {
    value: T,
    stamp: u64,
}

impl<T> Revisioned<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            value,
            stamp: fresh_stamp(),
        }
    }

    /// The stamp of the current value.
    pub(crate) fn revision(&self) -> u64 {
        self.stamp
    }
}

impl<T: Default> Default for Revisioned<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> From<T> for Revisioned<T> {
    fn from(value: T) -> Self {
        Self::new(value)
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
        self.stamp = fresh_stamp();
        &mut self.value
    }
}

impl<T: PartialEq> PartialEq for Revisioned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Eq> Eq for Revisioned<T> {}

impl<T: std::fmt::Debug> std::fmt::Debug for Revisioned<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(formatter)
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

    #[test]
    fn mutable_access_takes_a_fresh_stamp_and_reads_keep_it() {
        let mut value = Revisioned::new(vec![1]);
        let first = value.revision();
        assert_eq!(value.len(), 1);
        assert_eq!(value.revision(), first, "a read keeps the stamp");
        value.push(2);
        assert_ne!(value.revision(), first, "a mutable access replaces it");
        let copy = value.clone();
        assert_eq!(
            copy.revision(),
            value.revision(),
            "a clone holds the same value"
        );
        let other = Revisioned::new(vec![1, 2]);
        assert_eq!(other, value, "equality sees the value alone");
        assert_ne!(
            other.revision(),
            value.revision(),
            "stamps are unique per value"
        );
    }
}
