//! Deterministic work samples of the browser's unit frame layout (#1057), printed as the
//! `unit-frame` operation of `coding-tooling/work-evidence/v1` for `.performance/contract.json`
//! (`scripts/operation-work.py` merges them with `presentation_work`'s). Nothing here is timed.
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
    println!(
        "{{\"operations\":[{{\"id\":\"unit-frame\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
}
