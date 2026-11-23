//! Deterministic work sample of the browser's seeded world build (#855), printed as the
//! `operations` part of `coding-tooling/work-evidence/v1` for `.performance/contract.json`
//! (`scripts/operation-work.py` merges it with the others). Nothing here is timed.
#[path = "../tests/support/seeded_content.rs"]
mod seeded_content;

fn main() {
    if let Some(path) = std::env::var_os(seeded_content::CHILD) {
        seeded_content::child(std::path::Path::new(&path));
        return;
    }
    let run = seeded_content::measure(&[]);
    assert!(run.seeded, "the page took its seed");
    println!(
        "{{\"operations\":[{{\"id\":\"seeded-content-entry\",\"samples\":[{{\"dimensions\":{{\"seeds\":1}},\"metrics\":{{\"base_part_builds\":{}}}}}]}}]}}",
        run.base_part_builds
    );
}
