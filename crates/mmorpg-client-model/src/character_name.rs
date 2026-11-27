//! The local character-name policy both clients apply (#1245).
//!
//! A name is trimmed and its inner whitespace collapsed to single spaces. It
//! has 2–24 code points: a letter first, then letters, combining marks,
//! spaces, apostrophes (`'`, `’`) and hyphens. "Letter" is the Unicode
//! Alphabetic property, so every name either client accepted before still
//! loads. Two names are the same character when their NFKC forms match
//! case-insensitively.
//!
//! The browser loads its roster before the wasm module, so it keeps an
//! equivalent regular expression in `web/src/character-selection.ts`; both
//! check the shared corpus in `fixtures/character-names.json`.

use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;

/// Shortest and longest name, in code points.
pub const NAME_LENGTH: std::ops::RangeInclusive<usize> = 2..=24;

/// Why a name was refused, with the text both clients show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Length,
    Characters,
}

impl NameError {
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Length => "Character name must be between 2 and 24 characters.",
            Self::Characters => {
                "Character name may contain letters, spaces, apostrophes, and hyphens."
            }
        }
    }
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

fn is_mark(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
    )
}

/// The name as stored, or why it is refused.
pub fn normalize_character_name(raw: &str) -> Result<String, NameError> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if !NAME_LENGTH.contains(&name.chars().count()) {
        return Err(NameError::Length);
    }
    let mut chars = name.chars();
    let first_is_letter = chars.next().is_some_and(char::is_alphabetic);
    let rest_allowed =
        chars.all(|c| c.is_alphabetic() || is_mark(c) || matches!(c, ' ' | '\'' | '’' | '-'));
    if first_is_letter && rest_allowed {
        Ok(name)
    } else {
        Err(NameError::Characters)
    }
}

/// The form two names are compared in: NFKC, then lowercase.
#[must_use]
pub fn comparable_character_name(name: &str) -> String {
    name.nfkc().collect::<String>().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Corpus {
        names: Vec<NameCase>,
        same: Vec<[String; 2]>,
        different: Vec<[String; 2]>,
    }

    #[derive(Deserialize)]
    struct NameCase {
        raw: String,
        stored: Option<String>,
    }

    #[test]
    fn both_clients_share_one_name_corpus() {
        let corpus: Corpus =
            serde_json::from_str(include_str!("../../../fixtures/character-names.json")).unwrap();
        for case in &corpus.names {
            assert_eq!(
                normalize_character_name(&case.raw).ok(),
                case.stored,
                "{:?}",
                case.raw
            );
        }
        for [a, b] in &corpus.same {
            assert_eq!(comparable_character_name(a), comparable_character_name(b));
        }
        for [a, b] in &corpus.different {
            assert_ne!(comparable_character_name(a), comparable_character_name(b));
        }
    }
}
