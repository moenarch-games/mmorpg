//! Unit frame layout sentinel (#1057): the browser's unit layout is O(characters) with one
//! entity lookup per sampled and imported unit, carries no JSON, and a warm frame's sampling
//! and layout allocate a constant amount whatever the crowd (`.performance/contract.json`'s
//! `unit-frame`, `docs/PRESENTATION_WORKLOADS.md`).
#[path = "support/unit_layout.rs"]
mod unit_layout;

use unit_layout::crowd;

/// Allocations the adapter may make in a warm frame: the breathing torsos' new resource keys
/// (one interned string each, and the table's amortised growth), never one per character.
const WARM_FRAME_ALLOCATIONS: usize = 8;

#[test]
fn layout_work_is_linear_and_warm_frames_allocate_a_constant_amount() {
    // The hub's own residents and the viewer, without a crowd.
    let base = crowd(0, 16);
    assert!(
        base.imported >= 1,
        "the viewer is drawn on its model: {base:?}"
    );
    let runs: Vec<_> = [8, 16, 32]
        .map(|characters| (characters, crowd(characters, 16)))
        .into();
    for &(characters, run) in &runs {
        assert_eq!(run.units, base.units + characters, "{run:?}");
        // One lookup per sampled wolf, none per pair.
        assert_eq!(
            run.layout_entity_lookups,
            base.layout_entity_lookups + characters,
            "{run:?}"
        );
        assert!(
            run.warm_frame_allocations <= WARM_FRAME_ALLOCATIONS,
            "{characters}: {run:?}"
        );
    }
    // Bytes grow with the crowd, no faster: doubling it at most doubles them.
    for pair in runs.windows(2) {
        let ((_, smaller), (_, larger)) = (pair[0], pair[1]);
        assert!(larger.layout_bytes > smaller.layout_bytes, "{runs:?}");
        assert!(larger.layout_bytes <= 2 * smaller.layout_bytes, "{runs:?}");
    }
}

/// The browser's snapshot interpolation (#1058, `client-interpolate`): each frame walks the two
/// bracketing projections once, so the records it visits grow with the projected units, never
/// with their pairs, and a warm frame allocates nothing.
#[test]
fn interpolation_visits_each_bracketing_record_once_and_warm_frames_never_allocate() {
    for entities in [8, 16, 32, 63] {
        let run = unit_layout::interpolation(entities, 16);
        assert_eq!(run.samples_visited, 2 * run.units, "{entities}: {run:?}");
        assert_eq!(run.warm_frame_allocations, 0, "{entities}: {run:?}");
    }
}
