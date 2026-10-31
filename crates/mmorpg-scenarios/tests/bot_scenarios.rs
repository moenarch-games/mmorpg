//! One trial per checked-in bot scenario (`scenarios/bots/<name>.toml`, #540), so nextest runs
//! them in parallel and a failure names its scenario. Each trial runs the scenario twice: the
//! runs must agree (determinism), pass and reproduce `<name>.expected` byte for byte. After an
//! intentional behavior change, regenerate with:
//!
//! ```sh
//! MMORPG_SCENARIOS_UPDATE=1 cargo test -p mmorpg-scenarios --locked
//! ```

mod support;

use std::path::Path;

use libtest_mimic::{Arguments, Failed, Trial};
use mmorpg_scenarios::bots;

fn check(path: &Path) -> Result<(), Failed> {
    let text = std::fs::read_to_string(path)?;
    let run = || bots::load(&text).and_then(|scenario| bots::run(&scenario));
    let report = run()?;
    if report.to_json_lines() != run()?.to_json_lines() {
        return Err("two runs of the scenario differ".into());
    }
    let actual = report.to_text();
    let expected_path = path.with_extension("expected");
    if support::updating() {
        std::fs::write(&expected_path, &actual)?;
    } else {
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
        if expected != actual {
            return Err(format!(
                "scenario output changed; rerun with MMORPG_SCENARIOS_UPDATE=1 if intentional:\n\
                 --- expected\n{expected}--- actual\n{actual}"
            )
            .into());
        }
    }
    if !report.passed() {
        return Err(format!("checked-in scenario must pass:\n{actual}").into());
    }
    support::check_json_matches_text(&report);
    Ok(())
}

fn main() {
    let arguments = Arguments::from_args();
    let trials = support::scenario_files("bots")
        .into_iter()
        .map(|path| {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("scenario file names are UTF-8")
                .to_owned();
            Trial::test(name, move || check(&path))
        })
        .collect();
    libtest_mimic::run(&arguments, trials).exit();
}
