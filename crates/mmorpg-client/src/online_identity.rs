//! Stable, client-owned aliases from local roster slots to global development claims.
use mmorpg_core::CharacterId;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};
const FILE: &str = "online-character-ids.json";

pub(crate) fn load(directory: &Path) -> Result<BTreeMap<String, String>, String> {
    let bytes = match fs::read(directory.join(FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.to_string()),
    };
    if bytes.len() > 131_072 {
        return Err("online character identity file is too large".into());
    }
    let ids: BTreeMap<String, String> =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if ids.len() > 1024 || ids.values().any(|id| CharacterId::new(id.clone()).is_err()) {
        return Err("invalid online character identities".into());
    }
    Ok(ids)
}

pub fn for_character(directory: &Path, local_id: &str) -> Result<CharacterId, String> {
    CharacterId::new(local_id.to_owned()).map_err(|error| error.to_string())?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join("online-character-ids.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    let mut ids = load(directory)?;
    if let Some(id) = ids.get(local_id) {
        return CharacterId::new(id.clone()).map_err(|error| error.to_string());
    }
    if ids.len() >= 1024 {
        return Err("online character identity limit reached".into());
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| error.to_string())?;
    let id = format!(
        "online-{}",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    ids.insert(local_id.to_owned(), id.clone());
    let bytes = serde_json::to_vec(&ids).map_err(|error| error.to_string())?;
    let temporary = directory.join("online-character-ids.tmp");
    let mut file = File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    fs::rename(temporary, directory.join(FILE)).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|error| error.to_string())?;
    CharacterId::new(id).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_slot_keeps_its_identity_across_reads_and_other_profiles_get_distinct_ids() {
        let first = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let id = for_character(first.path(), "local-1").unwrap();
        assert_eq!(for_character(first.path(), "local-1").unwrap(), id);
        assert_ne!(for_character(other.path(), "local-1").unwrap(), id);
        assert_ne!(for_character(first.path(), "local-2").unwrap(), id);
    }
    #[test]
    fn deleted_roster_slots_with_online_characters_are_not_reused() {
        use crate::roster::{Headwear, Roster};
        use mmorpg_core::{PlayerClass, Sex};
        let directory = tempfile::tempdir().unwrap();
        let mut roster = Roster::load(directory.path()).unwrap();
        roster
            .create(
                "First Ranger",
                PlayerClass::Ranger,
                Sex::Female,
                Headwear::RangerCap,
            )
            .unwrap();
        let old = roster.characters[0].id.clone();
        for_character(directory.path(), &old).unwrap();
        roster.delete(0).unwrap();
        roster
            .create(
                "Second Ranger",
                PlayerClass::Ranger,
                Sex::Female,
                Headwear::RangerCap,
            )
            .unwrap();
        assert_ne!(roster.characters[0].id, old);
    }
}
