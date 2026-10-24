//! The native local character roster (#183): names, class, sex and headwear of
//! characters created on this machine, with the browser's local-roster rules
//! (`web/src/character-roster.ts`, `character-customization.ts`).
//!
//! This is presentation choice only. The zone owns the class choice once a
//! character enters the world; nothing here stores position, quests, items or
//! any other world progress. Character hosts receive a persisted online alias
//! for the local slot and its class/sex choice; anonymous hosts retain the
//! existing `ChooseClass` path. Names and headwear remain presentation choices.
use mmorpg_core::{PlayerClass, Sex};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Character slots on one machine, as the browser's `MAX_CHARACTER_SLOTS`.
pub const MAX_CHARACTERS: usize = 8;
const SCHEMA_VERSION: u64 = 1;
const MAX_ROSTER_BYTES: u64 = 8192;
const FILE_NAME: &str = "roster.json";

/// Local headwear, as the browser's `HAT_OPTIONS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Headwear {
    WayfarerHood,
    RangerCap,
    IroncrestHelm,
}

impl Headwear {
    pub const ALL: [Self; 3] = [Self::WayfarerHood, Self::RangerCap, Self::IroncrestHelm];

    /// The shared style id, also accepted by `Presentation::set_headwear`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::WayfarerHood => "wayfarer-hood",
            Self::RangerCap => "ranger-cap",
            Self::IroncrestHelm => "ironcrest-helm",
        }
    }

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::WayfarerHood => "Wayfarer's Hood",
            Self::RangerCap => "Ranger's Cap",
            Self::IroncrestHelm => "Ironcrest Helm",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|hat| hat.id() == id)
    }

    /// The browser's `defaultAppearanceForClass`.
    #[must_use]
    pub const fn for_class(class: PlayerClass) -> Self {
        match class {
            PlayerClass::Warden => Self::IroncrestHelm,
            PlayerClass::Ranger => Self::RangerCap,
            PlayerClass::Arcanist
            | PlayerClass::Necromancer
            | PlayerClass::Rogue
            | PlayerClass::Cleric => Self::WayfarerHood,
        }
    }
}

/// One locally created character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCharacter {
    /// `local-N`, the lowest free number not retired by an online identity.
    pub id: String,
    pub name: String,
    pub class: PlayerClass,
    pub sex: Sex,
    pub headwear: Headwear,
}

/// Trim, collapse inner whitespace and check the browser's name rule: 2–24
/// characters, a letter first, then letters, spaces, apostrophes and hyphens.
pub fn normalize_name(raw: &str) -> Result<String, String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = name.chars().count();
    if !(2..=24).contains(&length) {
        return Err("Character name must be between 2 and 24 characters.".into());
    }
    let mut chars = name.chars();
    let first_is_letter = chars.next().is_some_and(char::is_alphabetic);
    if !first_is_letter || !chars.all(|c| c.is_alphabetic() || matches!(c, ' ' | '\'' | '’' | '-'))
    {
        return Err("Character name may contain letters, spaces, apostrophes, and hyphens.".into());
    }
    Ok(name)
}

fn comparable(name: &str) -> String {
    name.to_lowercase()
}

fn class_id(class: PlayerClass) -> &'static str {
    class.name()
}

fn sex_id(sex: Sex) -> &'static str {
    match sex {
        Sex::Male => "male",
        Sex::Female => "female",
    }
}

fn valid_id(id: &str) -> bool {
    id.strip_prefix("local-").is_some_and(|n| {
        !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    }) && id.len() <= 32
}

/// The roster and where it is kept. Without a path it lives in memory only.
#[derive(Debug, Default)]
pub struct Roster {
    pub characters: Vec<LocalCharacter>,
    path: Option<PathBuf>,
}

/// `$MMORPG_CONFIG_DIR`, otherwise the platform configuration directory
/// (`$XDG_CONFIG_HOME` or `~/.config` on Linux, `~/Library/Application
/// Support` on macOS, `%APPDATA%` on Windows) plus `mmorpg`.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    let var = |name| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = var("MMORPG_CONFIG_DIR") {
        return Some(dir);
    }
    let base = if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Application Support"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|home| home.join(".config")))
    };
    base.map(|base| base.join("mmorpg"))
}

impl Roster {
    /// Loads the roster kept in `dir`; a missing file is an empty roster.
    /// An unreadable or invalid file fails closed and is never overwritten.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join(FILE_NAME);
        let characters = match std::fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("Local character roster is unreadable: {error}")),
            Ok(meta) if meta.len() > MAX_ROSTER_BYTES => {
                return Err("Local character roster is too large.".into());
            }
            Ok(_) => {
                let raw = std::fs::read_to_string(&path)
                    .map_err(|error| format!("Local character roster is unreadable: {error}"))?;
                decode(&raw)?
            }
        };
        Ok(Self {
            characters,
            path: Some(path),
        })
    }

    /// Creates a character with the lowest free, unretired `local-N` ID and saves.
    pub fn create(
        &mut self,
        name: &str,
        class: PlayerClass,
        sex: Sex,
        headwear: Headwear,
    ) -> Result<usize, String> {
        if self.characters.len() >= MAX_CHARACTERS {
            return Err(format!("All {MAX_CHARACTERS} character slots are full."));
        }
        let name = normalize_name(name)?;
        if self
            .characters
            .iter()
            .any(|c| comparable(&c.name) == comparable(&name))
        {
            return Err("A character with that name already exists in this local roster.".into());
        }
        let retired = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(crate::online_identity::load)
            .transpose()?
            .unwrap_or_default();
        let id = (1..)
            .map(|n| format!("local-{n}"))
            .find(|id| !retired.contains_key(id) && self.characters.iter().all(|c| &c.id != id))
            .expect("a free slot exists below the slot limit");
        self.characters.push(LocalCharacter {
            id,
            name,
            class,
            sex,
            headwear,
        });
        if let Err(error) = self.save() {
            self.characters.pop();
            return Err(error);
        }
        Ok(self.characters.len() - 1)
    }

    /// Changes one character's headwear and saves.
    pub fn set_headwear(&mut self, index: usize, headwear: Headwear) -> Result<(), String> {
        let Some(character) = self.characters.get_mut(index) else {
            return Err("No such character.".into());
        };
        let previous = std::mem::replace(&mut character.headwear, headwear);
        self.save().inspect_err(|_| {
            self.characters[index].headwear = previous;
        })
    }

    /// Deletes one character and saves.
    pub fn delete(&mut self, index: usize) -> Result<(), String> {
        if index >= self.characters.len() {
            return Err("No such character.".into());
        }
        let removed = self.characters.remove(index);
        self.save().inspect_err(|_| {
            self.characters.insert(index, removed);
        })
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let raw = encode(&self.characters);
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // Replace atomically so a crash never leaves half a roster.
            let partial = path.with_extension("json.partial");
            std::fs::write(&partial, raw.as_bytes())?;
            std::fs::rename(&partial, path)
        };
        write().map_err(|error| format!("Could not save the local character roster: {error}"))
    }
}

fn encode(characters: &[LocalCharacter]) -> String {
    let characters: Vec<Value> = characters
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "classId": class_id(c.class),
                "sex": sex_id(c.sex),
                "appearance": { "hat": c.headwear.id() },
            })
        })
        .collect();
    json!({ "schemaVersion": SCHEMA_VERSION, "characters": characters }).to_string()
}

fn object<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let map = value
        .as_object()
        .ok_or("Local character roster is invalid.")?;
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return Err("Local character roster has missing or unsupported fields.".into());
    }
    Ok(map)
}

fn decode(raw: &str) -> Result<Vec<LocalCharacter>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|_| "Local character roster is not valid JSON.")?;
    let envelope = object(&value, &["schemaVersion", "characters"])?;
    let (Some(SCHEMA_VERSION), Some(entries)) = (
        envelope["schemaVersion"].as_u64(),
        envelope["characters"].as_array(),
    ) else {
        return Err("Unsupported local character roster schema.".into());
    };
    if entries.len() > MAX_CHARACTERS {
        return Err("Local character roster exceeds the available slots.".into());
    }
    let invalid = || "Local character roster contains an invalid character.".to_owned();
    let mut characters: Vec<LocalCharacter> = Vec::new();
    for entry in entries {
        let fields = object(entry, &["id", "name", "classId", "sex", "appearance"])?;
        let appearance = object(&fields["appearance"], &["hat"])?;
        let id = fields["id"]
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or_else(invalid)?;
        let name = fields["name"].as_str().ok_or_else(invalid)?;
        let name = normalize_name(name).map_err(|_| invalid())?;
        let class = fields["classId"]
            .as_str()
            .and_then(|id| PlayerClass::ALL.into_iter().find(|c| c.name() == id))
            .ok_or_else(invalid)?;
        let sex = match fields["sex"].as_str() {
            Some("male") => Sex::Male,
            Some("female") => Sex::Female,
            _ => return Err(invalid()),
        };
        let headwear = appearance["hat"]
            .as_str()
            .and_then(Headwear::from_id)
            .ok_or_else(invalid)?;
        if characters
            .iter()
            .any(|c| c.id == id || comparable(&c.name) == comparable(&name))
        {
            return Err("Local character roster contains duplicate identity.".into());
        }
        characters.push(LocalCharacter {
            id: id.to_owned(),
            name,
            class,
            sex,
            headwear,
        });
    }
    Ok(characters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roster_survives_a_restart_and_fails_closed_on_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut roster = Roster::load(dir.path()).unwrap();
        assert!(roster.characters.is_empty());
        roster
            .create(
                "  Mira   Vale ",
                PlayerClass::Ranger,
                Sex::Female,
                Headwear::RangerCap,
            )
            .unwrap();
        roster
            .create(
                "Bram",
                PlayerClass::Warden,
                Sex::Male,
                Headwear::IroncrestHelm,
            )
            .unwrap();
        assert!(
            roster
                .create(
                    "mira vale",
                    PlayerClass::Arcanist,
                    Sex::Male,
                    Headwear::WayfarerHood
                )
                .is_err()
        );
        assert!(
            roster
                .create(
                    "1x",
                    PlayerClass::Arcanist,
                    Sex::Male,
                    Headwear::WayfarerHood
                )
                .is_err()
        );
        roster.set_headwear(1, Headwear::WayfarerHood).unwrap();
        roster.delete(0).unwrap();
        roster
            .create(
                "Ilsa O'Neil",
                PlayerClass::Arcanist,
                Sex::Female,
                Headwear::WayfarerHood,
            )
            .unwrap();

        let loaded = Roster::load(dir.path()).unwrap();
        assert_eq!(loaded.characters, roster.characters);
        assert_eq!(loaded.characters[0].name, "Bram");
        assert_eq!(loaded.characters[0].headwear, Headwear::WayfarerHood);
        // The freed lowest ID is reused.
        assert_eq!(loaded.characters[1].id, "local-1");
        assert_eq!(loaded.characters[1].name, "Ilsa O'Neil");

        let file = dir.path().join(FILE_NAME);
        for bad in [
            "not json",
            r#"{"schemaVersion":2,"characters":[]}"#,
            r#"{"schemaVersion":1,"characters":[],"progress":{}}"#,
            r#"{"schemaVersion":1,"characters":[{"id":"local-1","name":"Ab","classId":"bard","sex":"male","appearance":{"hat":"ranger-cap"}}]}"#,
        ] {
            std::fs::write(&file, bad).unwrap();
            assert!(Roster::load(dir.path()).is_err(), "{bad}");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), bad);
        }
    }
}
