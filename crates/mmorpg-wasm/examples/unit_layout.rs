//! Deterministic work samples of the browser's unit frame layout (#1057) and snapshot
//! interpolation (#1058), printed as the `unit-frame` and `client-interpolate` operations of
//! `coding-tooling/work-evidence/v1` for `.performance/contract.json` (`scripts/operation-work.py`
//! merges them with `presentation_work`'s). Nothing here is timed.
#[path = "../tests/support/unit_layout.rs"]
mod unit_layout;

fn main() {
    let samples: Vec<_> = [8, 16, 32, 64]
        .into_iter()
        .map(|characters| {
            let run = unit_layout::crowd(characters, 64);
            format!(
                "{{\"dimensions\":{{\"characters\":{characters}}},\"metrics\":{{\"layout_entity_lookups\":{},\"layout_bytes\":{},\"warm_frame_allocations\":{}}}}}",
                run.layout_entity_lookups, run.layout_bytes, run.warm_frame_allocations,
            )
        })
        .collect();
    // The viewer and up to 63 others: a full projection at the largest point.
    let interpolated: Vec<_> = [8, 16, 32, 63]
        .into_iter()
        .map(|others| {
            let run = unit_layout::interpolation(others, 64);
            format!(
                "{{\"dimensions\":{{\"entities\":{}}},\"metrics\":{{\"samples_visited\":{},\"warm_frame_allocations\":{}}}}}",
                run.units, run.samples_visited, run.warm_frame_allocations,
            )
        })
        .collect();
    println!(
        "{{\"operations\":[{{\"id\":\"unit-frame\",\"samples\":[{}]}},{{\"id\":\"client-interpolate\",\"samples\":[{}]}}]}}",
        samples.join(","),
        interpolated.join(","),
    );
}
