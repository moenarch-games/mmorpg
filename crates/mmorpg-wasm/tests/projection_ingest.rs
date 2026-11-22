//! Projection ingest sentinel (#1009): every projection the browser receives is decoded once,
//! its unchanged sheets are not materialised for the page again, and no bytes cross the WASM
//! boundary beyond the online network payload (`.performance/contract.json`'s
//! `projection-ingest`, `docs/PRESENTATION_WORKLOADS.md`).
#[path = "support/projection_ingest.rs"]
mod projection_ingest;

use projection_ingest::ingest;

#[test]
fn each_projection_is_decoded_once_and_unchanged_sheets_stay_in_wasm() {
    let run = ingest(32);
    assert_eq!(run.projections, 64, "{run:?}");
    assert_eq!(run.largest_decodes_per_projection, 1, "{run:?}");
    assert_eq!(run.unchanged_sheet_views, 0, "{run:?}");
    assert_eq!(run.extra_boundary_bytes, 0, "{run:?}");
    // The first view of each session carries its sheets; nothing is hidden to get there.
    assert!(run.sheet_views >= 2, "{run:?}");
}
