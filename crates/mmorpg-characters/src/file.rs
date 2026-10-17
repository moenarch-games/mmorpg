//! A directory of character entries for development and LAN hosts.
//!
//! Each character has `<id>.character` and a `<id>.lock` file. An update
//! holds an exclusive OS file lock on the lock file across read, write and
//! rename, so updates from several processes serialize; the new entry is
//! written to a temporary file, synced and atomically renamed into place,
//! so a crash leaves either the old or the new entry, never a torn one.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use mmorpg_core::CharacterId;

use crate::{EntryBackend, EntryChange, StoreError};

pub struct FileEntries {
    directory: PathBuf,
}

impl FileEntries {
    /// Uses `directory`, creating it if needed.
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let directory = directory.into();
        fs::create_dir_all(&directory).map_err(backend)?;
        Ok(Self { directory })
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn path(&self, id: &CharacterId, extension: &str) -> PathBuf {
        self.directory.join(format!("{id}.{extension}"))
    }
}

impl EntryBackend for FileEntries {
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
        match fs::read(self.path(id, "character")) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(backend(error)),
        }
    }

    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.path(id, "lock"))
            .map_err(backend)?;
        lock.lock().map_err(backend)?;
        let current = self.read(id)?;
        let next = change(current.as_deref())?;
        let temporary = self.path(id, "character.tmp");
        let mut file = File::create(&temporary).map_err(backend)?;
        file.write_all(&next).map_err(backend)?;
        file.sync_all().map_err(backend)?;
        fs::rename(&temporary, self.path(id, "character")).map_err(backend)?;
        File::open(&self.directory)
            .and_then(|directory| directory.sync_all())
            .map_err(backend)?;
        // Dropping `lock` releases it.
        Ok(())
    }
}

fn backend(error: io::Error) -> StoreError {
    StoreError::Backend(error.to_string())
}
