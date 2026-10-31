//! Shared by the scenario golden tests: the checked-in scenario files and the report checks.

use std::path::{Path, PathBuf};

use mmorpg_scenarios::Report;
use serde_json::Value;

/// The `scenarios/<tool>/*.toml` files, sorted.
pub fn scenario_files(tool: &str) -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios")
        .join(tool);
    let mut files = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect::<Vec<_>>();
    files.sort();
    assert!(!files.is_empty(), "no scenarios in {}", directory.display());
    files
}

/// Whether expected outputs are being rewritten instead of compared.
pub fn updating() -> bool {
    std::env::var_os("MMORPG_SCENARIOS_UPDATE").is_some() || mmorpg_core::golden::updating()
}

/// `report`'s JSON lines agree with its text lines and end with its result.
pub fn check_json_matches_text(report: &Report) {
    let json = report.to_json_lines();
    let lines = json
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), report.to_text().lines().count());
    let result = lines.last().unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["passed"], report.passed());
}
