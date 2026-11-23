//! The `seeded-content-entry` workload (#855, `.performance/contract.json`): the browser's page
//! seeds the zone content's costly base parts (terrain heights, colliders, solid dressing) from
//! its world-build worker or from the release-keyed derived cache, then builds what it needs
//! before entry: the hosted content and catalog, the presentation scenery and a joined local zone.
//! None of that may rebuild a seeded part. Each part is built at most once per process, so the
//! seeded page runs in a fresh process of this same binary. Counters only; nothing is timed.

use std::path::Path;
use std::process::Command;
use std::{env, fs};

use mmorpg_wasm::host::{LocalZoneHost, hosted_content_seed, seed_hosted_content};

/// Set (to the seed's file) in the fresh process that plays the seeded page.
pub const CHILD: &str = "MMORPG_SEEDED_CONTENT";

/// What the seeded page built.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeededEntry {
    /// Whether the page took the seed.
    pub seeded: bool,
    /// Base parts the page built itself although seeded.
    pub base_part_builds: usize,
}

/// Plays the seeded page in this (fresh) process from the seed at `path`, and prints the result
/// for [`measure`].
pub fn child(path: &Path) {
    let bytes = fs::read(path).expect("the parent wrote the seed");
    let seed: Vec<i32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&word| i32::from_le_bytes(word))
        .collect();
    let seeded = seed_hosted_content(&seed);
    let _ = mmorpg_wasm::catalog::hosted_catalog_json();
    let _ = mmorpg_wasm::scenery::hosted_scenery();
    let mut host = LocalZoneHost::new().expect("the hosted zone builds");
    host.join().expect("a player joins");
    host.tick().expect("the zone ticks");
    let builds = mmorpg_core::greyhaven_vale::base_builds();
    println!("seeded-content-entry {} {builds}", u8::from(seeded));
}

/// Builds the seed in this process, then plays the seeded page in a fresh one: this binary run
/// with `args` and [`CHILD`] set.
#[must_use]
pub fn measure(args: &[&str]) -> SeededEntry {
    let seed = hosted_content_seed();
    let path = env::temp_dir().join(format!("mmorpg-content-seed-{}.bin", std::process::id()));
    let bytes: Vec<u8> = seed.iter().flat_map(|value| value.to_le_bytes()).collect();
    fs::write(&path, bytes).expect("the seed is written");
    let output = Command::new(env::current_exe().expect("this binary"))
        .args(args)
        .env(CHILD, &path)
        .output()
        .expect("the seeded page runs");
    let _ = fs::remove_file(&path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix("seeded-content-entry "))
        .unwrap_or_else(|| {
            panic!(
                "no result from the seeded page: {stdout}{}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    let mut fields = line
        .split(' ')
        .map(|field| field.parse::<usize>().expect("a count"));
    SeededEntry {
        seeded: fields.next() == Some(1),
        base_part_builds: fields.next().expect("the build count"),
    }
}
