//! Development/LAN persistence. One process owns the shared directory for its lifetime.
use crate::ZoneGameServerAdapter;
use game_server::{
    ControlContext, ControlServiceError, LiveMatchHost, MatchControlService, MatchId,
};
use mmorpg_characters::{
    CharacterLease, CharacterStore, EntryBackend, FileEntries, LeaseHolder, decode_entry,
};
use mmorpg_core::{CharacterId, ClassChoice, ZoneId, ZoneSimulation};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

type Key = (MatchId, u32);

pub struct DevCharacterStorage {
    ownership: Arc<File>,
    epoch: u64,
    store: CharacterStore<FileEntries>,
    leases: BTreeMap<Key, CharacterLease>,
    released: BTreeSet<Key>,
}

impl DevCharacterStorage {
    /// The OS lock prevents a new epoch while any simulation still owns the directory.
    /// This deliberately permits only one development host per shared directory.
    pub fn open(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(message)?;
        let ownership = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(directory.join("host.lock"))
            .map_err(message)?;
        ownership
            .try_lock()
            .map_err(|error| format!("character directory is already owned: {error}"))?;
        let epoch_path = directory.join("host.epoch");
        let previous = match fs::read_to_string(&epoch_path) {
            Ok(value) => value.trim().parse::<u64>().map_err(message)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(message(error)),
        };
        let epoch = previous
            .checked_add(1)
            .ok_or("character host epoch exhausted")?;
        let temporary = directory.join("host.epoch.tmp");
        let mut file = File::create(&temporary).map_err(message)?;
        writeln!(file, "{epoch}").map_err(message)?;
        file.sync_all().map_err(message)?;
        fs::rename(temporary, epoch_path).map_err(message)?;
        File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(message)?;
        let mut store =
            CharacterStore::new(FileEntries::open(directory.join("characters")).map_err(message)?);
        // The previous process has relinquished its OS lock. Clear stranded leases
        // using a newer epoch of their own zone before any runtime can start.
        for entry in fs::read_dir(directory.join("characters")).map_err(message)? {
            let path = entry.map_err(message)?.path();
            if path
                .extension()
                .is_none_or(|extension| extension != "character")
            {
                continue;
            }
            let id = CharacterId::new(
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .ok_or("invalid character filename")?
                    .to_owned(),
            )
            .map_err(message)?;
            let bytes = store
                .backend()
                .read(&id)
                .map_err(message)?
                .ok_or("missing character entry")?;
            let entry = decode_entry(&id, &bytes).map_err(message)?;
            if let Some(holder) = entry.lease {
                let acquired = store
                    .acquire(
                        &id,
                        LeaseHolder {
                            zone_id: holder.zone_id,
                            zone_epoch: epoch,
                        },
                    )
                    .map_err(message)?;
                store.release(&acquired.lease, None).map_err(message)?;
            }
        }
        Ok(Self {
            ownership: Arc::new(ownership),
            epoch,
            store,
            leases: BTreeMap::new(),
            released: BTreeSet::new(),
        })
    }

    /// Reacquires restored bindings while the prepared host is still stopped.
    pub fn restore_snapshot(&mut self, id: &MatchId, payload: &[u8]) -> Result<(), String> {
        for stored in ZoneGameServerAdapter::stored_characters(payload).map_err(message)? {
            let key = (id.clone(), stored.player);
            self.persist(key, stored)?;
        }
        Ok(())
    }

    fn persist(
        &mut self,
        key: Key,
        stored: crate::character_admission::StoredCharacter,
    ) -> Result<(), String> {
        if self.released.contains(&key) {
            return Ok(());
        }
        let lease = if let Some(lease) = self.leases.get(&key) {
            lease.clone()
        } else {
            let holder = LeaseHolder {
                zone_id: stored.record.location.zone_id,
                zone_epoch: self.epoch,
            };
            let acquired = self
                .store
                .acquire(&stored.record.character_id, holder)
                .map_err(message)?;
            self.leases.insert(key.clone(), acquired.lease.clone());
            acquired.lease
        };
        if stored.parked {
            self.store
                .release(&lease, Some(&stored.record))
                .map_err(message)?;
            self.leases.remove(&key);
            self.released.insert(key);
        } else {
            self.store.commit(&lease, &stored.record).map_err(message)?;
        }
        Ok(())
    }

    pub fn ownership(&self) -> Arc<File> {
        Arc::clone(&self.ownership)
    }
}

#[derive(Clone)]
pub struct DevCharacterService {
    host: LiveMatchHost<ZoneGameServerAdapter>,
    storage: Arc<Mutex<DevCharacterStorage>>,
    runtime: tokio::runtime::Handle,
}

impl DevCharacterService {
    pub fn new(host: LiveMatchHost<ZoneGameServerAdapter>, storage: DevCharacterStorage) -> Self {
        Self {
            host,
            storage: Arc::new(Mutex::new(storage)),
            runtime: tokio::runtime::Handle::current(),
        }
    }

    fn snapshot(
        &self,
        id: &MatchId,
    ) -> Result<Vec<crate::character_admission::StoredCharacter>, String> {
        let snapshot = self
            .runtime
            .block_on(self.host.inspect(id, |runtime| runtime.snapshot()))
            .map_err(message)?
            .map_err(message)?;
        ZoneGameServerAdapter::stored_characters(&snapshot.payload).map_err(message)
    }

    /// Called from a blocking worker, never the simulation tick or async executor.
    /// Snapshots and claims serialize through this storage owner; I/O never owns a runtime lock.
    pub fn save_all(&self) -> Result<(), String> {
        let mut storage = self.storage.lock().map_err(message)?;
        let mut live = BTreeSet::new();
        for status in self.runtime.block_on(self.host.statuses()) {
            for stored in self.snapshot(&status.id)? {
                let key = (status.id.clone(), stored.player);
                live.insert(key.clone());
                storage.persist(key, stored)?;
            }
        }
        // Abruptly lost sessions keep their last periodic commit. Stronger reward
        // acknowledgement/crash journaling belongs to #40, not this dev adapter.
        let departed: Vec<_> = storage
            .leases
            .keys()
            .filter(|key| !live.contains(*key))
            .cloned()
            .collect();
        for key in departed {
            let lease = storage
                .leases
                .get(&key)
                .ok_or("missing character lease")?
                .clone();
            storage.store.release(&lease, None).map_err(message)?;
            storage.leases.remove(&key);
        }
        storage.released.retain(|key| live.contains(key));
        Ok(())
    }

    /// Commits the final snapshots captured from graceful recovery after serving
    /// stops, then returns all character leases offline.
    pub fn finish_snapshots(&self, snapshots: Vec<(MatchId, Vec<u8>)>) -> Result<(), String> {
        let mut storage = self.storage.lock().map_err(message)?;
        for (id, payload) in snapshots {
            storage.restore_snapshot(&id, &payload)?;
        }
        let keys: Vec<_> = storage.leases.keys().cloned().collect();
        for key in keys {
            let lease = storage
                .leases
                .get(&key)
                .ok_or("missing final character lease")?
                .clone();
            storage.store.release(&lease, None).map_err(message)?;
            storage.leases.remove(&key);
        }
        Ok(())
    }

    fn claim(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        let (&appearance, raw_id) = payload.split_first().ok_or("missing character claim")?;
        let class =
            ClassChoice::from_appearance(u16::from(appearance)).ok_or("invalid character class")?;
        let character = CharacterId::new(std::str::from_utf8(raw_id).map_err(message)?.to_owned())
            .map_err(message)?;
        let zone = id
            .as_str()
            .strip_prefix("zone-")
            .ok_or("not a zone match")?
            .parse::<u32>()
            .map(ZoneId::new)
            .map_err(message)?;
        let mut storage = self.storage.lock().map_err(message)?;
        let key = (id.clone(), context.player_id);
        if let Some(lease) = storage.leases.get(&key) {
            return if lease.character_id == character {
                Ok(vec![1, 1])
            } else {
                Err("session already has a character".into())
            };
        }
        let holder = LeaseHolder {
            zone_id: zone,
            zone_epoch: storage.epoch,
        };
        let acquired = storage.store.acquire(&character, holder).map_err(message)?;
        let admission = (|| {
            let record = match acquired.record {
                Some(record) => {
                    if record.class != class {
                        return Err("stored character does not match the selected class/sex".into());
                    }
                    record
                }
                None => {
                    let mut zone =
                        ZoneSimulation::with_content(zone, mmorpg_core::content_for_zone(zone))
                            .map_err(message)?;
                    zone.create_character(1, class).map_err(message)?;
                    let record = zone.capture_character(1, character).map_err(message)?;
                    storage
                        .store
                        .commit(&acquired.lease, &record)
                        .map_err(message)?;
                    record
                }
            };
            let input =
                ZoneGameServerAdapter::character_admission_input(&record).map_err(message)?;
            self.runtime
                .block_on(self.host.submit_host_input(
                    id,
                    context.player_id,
                    context.connection_epoch,
                    &input,
                ))
                .map_err(message)
        })();
        if let Err(error) = admission {
            if let Err(release) = storage.store.release(&acquired.lease, None) {
                return Err(format!("{error}; lease cleanup failed: {release}"));
            }
            return Err(error);
        }
        storage.leases.insert(key, acquired.lease);
        Ok(vec![1, 1])
    }
}

impl MatchControlService for DevCharacterService {
    fn handle(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, ControlServiceError> {
        let result = match payload {
            [1, 0] => Ok(vec![1, 1]), // development character capability
            [1, 1, claim @ ..] => self.claim(id, context, claim),
            [1, 2] => self
                .runtime
                .block_on(self.host.submit_host_input(
                    id,
                    context.player_id,
                    context.connection_epoch,
                    &[1, 2],
                ))
                .map_err(message)
                .and_then(|()| self.save_all())
                .map(|()| vec![1, 2]),
            _ => Err("unsupported character control request".into()),
        };
        result.map_err(ControlServiceError::new)
    }
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "mmorpg-character-host-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn simulation_ownership_blocks_a_new_host_epoch_until_the_last_owner_drops() {
        let directory = Directory::new();
        let storage = DevCharacterStorage::open(&directory.0).unwrap();
        let ownership = storage.ownership();
        assert_eq!(storage.epoch, 1);
        drop(storage);
        assert!(DevCharacterStorage::open(&directory.0).is_err());
        drop(ownership);
        let replacement = DevCharacterStorage::open(&directory.0).unwrap();
        assert_eq!(replacement.epoch, 2);
    }

    #[test]
    fn restart_releases_an_uncommitted_claim_from_the_stopped_host() {
        let directory = Directory::new();
        let mut storage = DevCharacterStorage::open(&directory.0).unwrap();
        let id = CharacterId::new("interrupted-claim").unwrap();
        let previous = storage
            .store
            .acquire(
                &id,
                LeaseHolder {
                    zone_id: ZoneId::new(1),
                    zone_epoch: storage.epoch,
                },
            )
            .unwrap();
        assert!(previous.record.is_none());
        drop(storage);
        let mut replacement = DevCharacterStorage::open(&directory.0).unwrap();
        let acquired = replacement
            .store
            .acquire(
                &id,
                LeaseHolder {
                    zone_id: ZoneId::new(2),
                    zone_epoch: replacement.epoch,
                },
            )
            .unwrap();
        assert!(acquired.record.is_none());
        assert!(replacement.store.release(&previous.lease, None).is_err());
    }
}
