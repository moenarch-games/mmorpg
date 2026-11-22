//! Deterministic work samples of the browser's projection ingest (#1009), printed as the
//! `operations` part of `coding-tooling/work-evidence/v1` for `.performance/contract.json`
//! (`scripts/operation-work.py` merges them with the others). Nothing here is timed.
#[path = "../tests/support/projection_ingest.rs"]
mod projection_ingest;

fn main() {
    let samples = [16, 32, 64]
        .into_iter()
        .map(|ticks| {
            let run = projection_ingest::ingest(ticks);
            format!(
                "{{\"dimensions\":{{\"ticks\":{ticks}}},\"metrics\":{{\"largest_decodes_per_projection\":{},\"unchanged_sheet_views\":{},\"extra_boundary_bytes\":{},\"sheet_views\":{}}}}}",
                run.largest_decodes_per_projection,
                run.unchanged_sheet_views,
                run.extra_boundary_bytes,
                run.sheet_views,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    println!("{{\"operations\":[{{\"id\":\"projection-ingest\",\"samples\":[{samples}]}}]}}");
}
