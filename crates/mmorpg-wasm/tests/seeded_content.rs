//! Seeded content sentinel (#855): a page whose zone content was seeded from the world-build
//! worker or the derived cache rebuilds none of its costly base parts on the way to entry
//! (`.performance/contract.json`'s `seeded-content-entry`).
#[path = "support/seeded_content.rs"]
mod seeded_content;

use std::path::PathBuf;

use seeded_content::{CHILD, SeededEntry, child, measure};

#[test]
fn a_seeded_page_builds_no_base_part() {
    if let Some(path) = std::env::var_os(CHILD) {
        child(&PathBuf::from(path));
        return;
    }
    let run = measure(&[
        "--exact",
        "a_seeded_page_builds_no_base_part",
        "--nocapture",
    ]);
    assert_eq!(
        run,
        SeededEntry {
            seeded: true,
            base_part_builds: 0
        }
    );
    // Unseeded, the same process built all three to make the seed.
    assert_eq!(mmorpg_core::greyhaven_vale::base_builds(), 3);
}
