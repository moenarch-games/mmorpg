//! `cargo run -p mmorpg-wasm --locked --example action-lab -- promote <promotion.json>`
//!
//! Turns a promotion the Action Lab exported (#383, Atlas 5.6.7) into a
//! reviewable change in this checkout: the regenerated production action
//! graph, a promotion record and a pull request description. It never
//! commits, pushes or publishes anything; production rules change only when
//! the resulting pull request is reviewed and merged.
//!
//! Exit status: 0 when the files were written, 2 for usage errors or a
//! request the checkout refuses.

use std::path::Path;
use std::process::ExitCode;

use mmorpg_wasm::lab_promotion::{self, BASELINE_SOURCE, RECORDS, Request};

const USAGE: &str =
    "usage: cargo run -p mmorpg-wasm --locked --example action-lab -- promote <promotion.json>";

fn promote(path: &str) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let request = Request::parse(&text)?;
    let source_path = root.join(BASELINE_SOURCE);
    let source = std::fs::read_to_string(&source_path).map_err(|error| error.to_string())?;
    let promotion = lab_promotion::promote(&request, &source)?;
    let record = format!("{RECORDS}/{}.json", promotion.slug);
    let body = format!("target/action-lab/{}.md", promotion.slug);
    let write = |relative: &str, text: &str| {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, text).map_err(|error| format!("{relative}: {error}"))
    };
    write(BASELINE_SOURCE, &promotion.source)?;
    write(&record, &promotion.record)?;
    write(&body, &promotion.body)?;
    let slug = &promotion.slug;
    println!(
        "Wrote {BASELINE_SOURCE}, {record} and {body}.\n\
         Production rules change only when a reviewed pull request merges. Next:\n\n  \
         cargo test -p mmorpg-core --locked\n  \
         git switch -c action-lab/{slug}\n  \
         git add {BASELINE_SOURCE} {record}\n  \
         git commit -m {title:?}\n  \
         git push -u origin action-lab/{slug}\n  \
         gh pr create --title {title:?} --body-file {body}",
        title = promotion.title,
    );
    Ok(())
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [command, path] = arguments.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if command != "promote" {
        eprintln!("unknown command {command}\n{USAGE}");
        return ExitCode::from(2);
    }
    match promote(path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("action-lab promote: {error}");
            ExitCode::from(2)
        }
    }
}
