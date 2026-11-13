//! The guard on ID-based visual lookups (#683, slice 5).
//!
//! Visuals picked by collider ID (`ids::…`, `fort::…`, `(400..=406).contains(&mesh.id)`,
//! `prop.collider == Some(…)`, `mesh.id == id`) tie art to gameplay data. New art is placed as world dressing
//! instead (`content/greyhaven/dressing`, `mmorpg_presentation::world_dressing`), so the number
//! of such lines in presentation and scenery may only go down: lower [`CAP`] when you remove
//! some, never raise it. Unit and ability visuals (`src/units`) pick by ability, not by world
//! collider, and are not counted.
use std::path::Path;

/// The most ID-based visual lookup lines allowed.
const CAP: usize = 91;

fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether `needle` starts a word in `code` (`ids::` but not `DerivedGrids::`).
fn word_start(code: &str, needle: &str) -> bool {
    code.match_indices(needle)
        .any(|(at, _)| at == 0 || !word(code.as_bytes()[at - 1]))
}

/// Whether `code` tests an ID's membership: `.contains(&id)`, `.contains(&mesh.id)`,
/// `.contains(id)`.
fn contains_id(code: &str) -> bool {
    code.match_indices("contains(").any(|(at, needle)| {
        let argument = code[at + needle.len()..].trim_start_matches('&');
        let path: String = argument
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        (path == "id" || path.ends_with(".id")) && argument[path.len()..].starts_with(')')
    })
}

/// Whether `line` looks a visual up by a collider or other world ID.
fn lookup(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    word_start(code, "ids::")
        || word_start(code, "fort::")
        || code.contains(".id ==")
        || code.contains("== id")
        || code.contains("collider == Some(")
        || contains_id(code)
}

fn count(dir: &Path, skip: &Path, found: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.expect("a readable entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path == skip {
            continue;
        }
        if path.is_dir() {
            count(&path, skip, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = std::fs::read_to_string(&path).expect("readable source");
            found.extend(
                source
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| lookup(line))
                    .map(|(index, line)| {
                        format!("{}:{}: {}", path.display(), index + 1, line.trim())
                    }),
            );
        }
    }
}

#[test]
fn id_based_visual_lookups_only_go_down() {
    let presentation = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let scenery = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mmorpg-scenery/src");
    let mut found = Vec::new();
    count(&presentation, &presentation.join("units"), &mut found);
    count(&scenery, Path::new(""), &mut found);
    assert!(
        found.len() <= CAP,
        "{} ID-based visual lookups, more than the cap of {CAP}: place new art as world dressing \
         instead (#683).\n{}",
        found.len(),
        found.join("\n")
    );
    if found.len() < CAP {
        eprintln!(
            "ID-based visual lookups fell to {}: lower CAP to match",
            found.len()
        );
    }
}

#[test]
fn lookups_are_recognised() {
    assert!(lookup("|| (400..=406).contains(&mesh.id)"));
    assert!(lookup(".filter(|c| (60_000..65_000).contains(&c.id))"));
    assert!(lookup(
        "_ if ids::SHRINE_CAVE_ROOF.contains(&id) => PropKind::CaveRoof,"
    ));
    assert!(lookup(".find(|prop| prop.collider == Some(HOUSE))"));
    assert!(lookup(".filter(|id| (103..=106).contains(id))"));
    assert!(!lookup("for x in 0..3 {"));
    assert!(!lookup("&& (-3_500..=3_500).contains(&prop.position[0])"));
    assert!(!lookup("derived: DerivedGrids::default(),"));
    assert!(!lookup("// ids::KEEP in a comment"));
}
