//! One session's decoded projections (#1009).
//!
//! The page used to decode each projection into a complete plain object, sheets included, and
//! then hand the same bytes back to the unit presenter, which decoded them a second time. A
//! [`ProjectionStore`] decodes each projection once and keeps it: the unit presenter and its
//! sound cues read the decoded projection from here by id, and the page receives a view whose
//! bag and quest sheets are filled only when they changed since the previous view (the page
//! keeps its earlier copy otherwise). The local host's projection is encoded and decoded inside
//! WASM, so its bytes never cross to the page; an online projection crosses once, as the
//! network payload. [`IngestWork`] counts that work for `.performance/contract.json`'s
//! `projection-ingest` operation (`docs/PRESENTATION_WORKLOADS.md`).

use std::{collections::VecDeque, rc::Rc};

use mmorpg_core::{TICK_HZ, ZoneSnapshot};

use crate::wire;

/// How many decoded projections stay readable by id: two seconds of ticks, so a stalled page
/// that receives a burst still presents every one of them in order.
pub const RETAINED_PROJECTIONS: usize = 2 * TICK_HZ as usize;

/// Deterministic counts of a store's work since it was made.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IngestWork {
    /// Projections ingested (and views handed to the page).
    pub projections: usize,
    /// Projections decoded from bytes.
    pub decodes: usize,
    /// Projection bytes the page copied into the store: online payloads only.
    pub boundary_bytes: usize,
    /// Sheet objects (bag, quests) materialised in a view for the page.
    pub sheet_views: usize,
}

/// One ingested projection: its id for the unit presenter, and the page's view.
pub struct Ingested {
    pub id: u32,
    /// Whether the view carries the bag sheet (inventory, equipment, stats); otherwise it is
    /// unchanged since the previous view and left out.
    pub bag: bool,
    /// Whether the view carries the quest sheet; otherwise it is unchanged and left out.
    pub quests: bool,
    pub view: wire::ZoneSnapshot,
}

/// One session's projection decoder and its recently decoded projections.
#[derive(Default)]
pub struct ProjectionStore {
    decoder: mmorpg_protocol::SnapshotDecoder,
    retained: VecDeque<(u32, Rc<ZoneSnapshot>)>,
    next_id: u32,
    work: IngestWork,
}

impl ProjectionStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes a projection the page received as bytes (a network payload) strictly; a
    /// malformed one changes nothing.
    ///
    /// # Errors
    ///
    /// When the bytes are not a projection.
    pub fn ingest(&mut self, payload: &[u8]) -> Result<Ingested, String> {
        let ingested = self.decode(payload)?;
        self.work.boundary_bytes += payload.len();
        Ok(ingested)
    }

    /// Decodes a projection encoded inside WASM (the local host's), whose bytes never crossed
    /// to the page.
    ///
    /// # Errors
    ///
    /// When the bytes are not a projection.
    pub fn ingest_local(&mut self, payload: &[u8]) -> Result<Ingested, String> {
        self.decode(payload)
    }

    fn decode(&mut self, payload: &[u8]) -> Result<Ingested, String> {
        let snapshot = self
            .decoder
            .decode(payload)
            .map_err(|error| error.to_string())?;
        self.work.decodes += 1;
        Ok(self.keep(snapshot))
    }

    fn keep(&mut self, snapshot: ZoneSnapshot) -> Ingested {
        let previous = self.retained.back().map(|(_, kept)| kept.as_ref());
        let bag = previous.is_none_or(|previous| {
            (
                previous.inventory_revision,
                &previous.inventory,
                &previous.equipment,
            ) != (
                snapshot.inventory_revision,
                &snapshot.inventory,
                &snapshot.equipment,
            )
        });
        let quests = previous.is_none_or(|previous| previous.quests != snapshot.quests);
        let view = wire::ZoneSnapshot::view(&snapshot, bag, quests);
        self.work.projections += 1;
        self.work.sheet_views +=
            usize::from(view.inventory.is_some()) + usize::from(view.quests.is_some());
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.retained.push_back((id, Rc::new(snapshot)));
        if self.retained.len() > RETAINED_PROJECTIONS {
            self.retained.pop_front();
        }
        Ingested {
            id,
            bag,
            quests,
            view,
        }
    }

    /// The decoded projection `id`, while it is retained.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<Rc<ZoneSnapshot>> {
        self.retained
            .iter()
            .rev()
            .find(|(kept, _)| *kept == id)
            .map(|(_, snapshot)| Rc::clone(snapshot))
    }

    /// The sheet revisions held, for the session's acknowledged commands (#544).
    #[must_use]
    pub fn revisions(&self) -> mmorpg_protocol::SheetRevisions {
        self.decoder.revisions()
    }

    #[must_use]
    pub const fn work(&self) -> IngestWork {
        self.work
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = mmorpg_core::golden::workspace_root()
            .join("fixtures/protocol")
            .join(name);
        let hex = std::fs::read_to_string(path).expect("fixture");
        let hex = hex.trim();
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn sheets_reach_the_page_only_when_they_change() {
        let first = fixture("player-snapshot.hex");
        let mut store = ProjectionStore::new();
        let ingested = store.ingest(&first).unwrap();
        assert!(ingested.bag && ingested.quests);
        let full = wire::decode_snapshot(&first).unwrap();
        assert_eq!(
            ingested.view, full,
            "the first view is the complete projection"
        );
        let again = store.ingest(&first).unwrap();
        assert!(!again.bag && !again.quests);
        assert_eq!((&again.view.inventory, &again.view.quests), (&None, &None));
        assert_ne!(ingested.id, again.id);
        let work = store.work();
        assert_eq!(work.projections, 2);
        assert_eq!(work.decodes, 2);
        assert_eq!(work.boundary_bytes, 2 * first.len());
        assert_eq!(
            work.sheet_views,
            usize::from(full.inventory.is_some()) + usize::from(full.quests.is_some())
        );
        // A projection with another bag carries it again.
        let other = fixture("player-loot.hex");
        let decoded = mmorpg_protocol::decode_snapshot(&other).unwrap();
        let kept = store.get(again.id).unwrap();
        let changed = store.ingest(&other).unwrap();
        assert_eq!(
            changed.bag,
            (kept.inventory_revision, &kept.inventory, &kept.equipment)
                != (
                    decoded.inventory_revision,
                    &decoded.inventory,
                    &decoded.equipment
                )
        );
        assert_eq!(*store.get(changed.id).unwrap(), decoded);
    }

    #[test]
    fn a_malformed_projection_changes_nothing() {
        let bytes = fixture("player-snapshot.hex");
        let mut store = ProjectionStore::new();
        assert!(store.ingest(&bytes[..bytes.len() - 1]).is_err());
        assert_eq!(store.work(), IngestWork::default());
        assert!(store.get(0).is_none());
    }

    #[test]
    fn only_recent_projections_stay_readable() {
        let bytes = fixture("player-snapshot.hex");
        let mut store = ProjectionStore::new();
        let first = store.ingest_local(&bytes).unwrap().id;
        for _ in 0..RETAINED_PROJECTIONS {
            store.ingest_local(&bytes).unwrap();
        }
        assert!(store.get(first).is_none());
        assert!(store.get(first + 1).is_some());
        assert_eq!(store.work().boundary_bytes, 0);
    }
}
