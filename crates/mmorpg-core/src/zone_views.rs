//! The zone-wide parts of every player projection, built once per tick (#1056).
//!
//! Every viewer sees the same doors, damaged pieces, spent ore veins, seals and wards. Projecting for P
//! recipients used to rebuild them P times for one unchanged state; they are now derived once
//! per tick and shared, like the threatened-player set (#1006).
//!
//! The cached views are keyed by the tick (a vein refills at a tick without a mutation) and the
//! revision stamps of `doors`, `pieces` and `gathering`, so any phase, command, admission or
//! in-place recovery that touches a contributor rebuilds them. A rebuild that yields the same
//! views keeps their [`ZoneViews::stamp`], which therefore changes only when what players see
//! changes: a viewer's world sheet is rebuilt only then. The cache is never canonical state and
//! never feeds back into gameplay; a zone built from a checkpoint starts with an empty one.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::{DoorView, PieceView, ZoneSimulation};

/// What every player sees of the zone's doors, pieces and veins at one tick.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ZoneViews {
    pub(crate) doors: Vec<DoorView>,
    pub(crate) pieces: Vec<PieceView>,
    pub(crate) depleted_veins: Vec<u32>,
    /// The shrine's seals and wards (#1019).
    pub(crate) seals: Vec<crate::SealView>,
    pub(crate) wards: Vec<crate::WardView>,
}

#[derive(Debug, Default)]
pub(crate) struct ZoneViewCache {
    entry: Mutex<Option<CachedViews>>,
    /// How many times the views were built rather than reused; workload evidence only.
    builds: AtomicU64,
}

#[derive(Debug)]
struct CachedViews {
    key: [u64; 4],
    views: Arc<ZoneViews>,
    /// Unchanged while rebuilt views equal the previous ones.
    stamp: u64,
}

impl ZoneSimulation {
    /// The zone-wide views at the current tick and their content stamp, built at most once per
    /// tick and unchanged state and shared by every recipient's projection.
    pub(crate) fn zone_views(&self) -> (Arc<ZoneViews>, u64) {
        let key = [
            self.tick,
            self.doors.revision(),
            self.pieces.revision(),
            self.gathering.revision(),
        ];
        let mut entry = self
            .zone_view_cache
            .entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = entry.as_ref()
            && cached.key == key
        {
            return (Arc::clone(&cached.views), cached.stamp);
        }
        let views = ZoneViews {
            doors: self.door_views(),
            pieces: self.piece_views(),
            depleted_veins: self.depleted_veins(),
            seals: self.seal_views(),
            wards: self.ward_views(),
        };
        self.zone_view_cache.builds.fetch_add(1, Ordering::Relaxed);
        let cached = match entry.take() {
            Some(previous) if *previous.views == views => CachedViews { key, ..previous },
            _ => CachedViews {
                key,
                views: Arc::new(views),
                stamp: crate::revisioned::fresh_stamp(),
            },
        };
        let shared = (Arc::clone(&cached.views), cached.stamp);
        *entry = Some(cached);
        shared
    }

    /// How many times the zone-wide projection views were built instead of reused (#1056).
    /// Deterministic for a given call sequence; workload evidence, never gameplay.
    pub fn zone_view_builds(&self) -> u64 {
        self.zone_view_cache.builds.load(Ordering::Relaxed)
    }
}
