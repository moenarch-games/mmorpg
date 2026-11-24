//! Test access to the asset-tooling bundles `scripts/fetch-bundles.sh` places under `web/public/`
//! (#1158). CI fetches them before the Rust tests, so a missing bundle fails the test with the
//! fetch hint instead of passing as if it had been checked. A developer without the bundles may
//! set `MMORPG_ALLOW_MISSING_BUNDLES=1` to skip those tests; the skip is printed, never silent.

/// Opts in to skipping a test whose bundle is missing.
const ALLOW_MISSING: &str = "MMORPG_ALLOW_MISSING_BUNDLES";

/// `web/public/<relative>` read as text, or `None` when it is missing and
/// `MMORPG_ALLOW_MISSING_BUNDLES=1` allows skipping. Panics otherwise.
pub(crate) fn read(relative: &str) -> Option<String> {
    let path = format!("{}/../../web/public/{relative}", env!("CARGO_MANIFEST_DIR"));
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if std::env::var(ALLOW_MISSING).as_deref() == Ok("1") => {
            eprintln!("SKIPPED: web/public/{relative} is not fetched ({error}; {ALLOW_MISSING}=1)");
            None
        }
        Err(error) => panic!(
            "web/public/{relative} is missing ({error}): run scripts/fetch-bundles.sh, \
             or set {ALLOW_MISSING}=1 to skip the tests that need it"
        ),
    }
}
