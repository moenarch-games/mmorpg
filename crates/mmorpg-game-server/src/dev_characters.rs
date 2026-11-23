//! Development/LAN persistence. In file mode one process owns the shared
//! directory for its lifetime; in handoff mode (#145) the characters live in
//! a store several hosts share, under each zone's control-plane lease epoch.
use crate::ZoneGameServerAdapter;
use crate::handoff_host::HandoffHost;
use game_server::{
    ControlContext, ControlServiceError, LiveMatchHost, MatchControlService, MatchId,
};
use mmorpg_characters::{
    CharacterLease, CharacterStore, EntryBackend, FileEntries, LeaseHolder, StoreError,
    decode_entry,
};
use mmorpg_core::{CharacterId, ClassChoice, ZoneId, ZoneSimulation};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

pub(crate) type Key = (MatchId, u32);

/// Consecutive failed save passes (one a second) after which a live character is
/// logged out (#1145): it stops gaining progress the store cannot keep, while the
/// rest of the host keeps serving. Its save is still retried every pass.
pub const MAX_CONSECUTIVE_SAVE_FAILURES: u32 = 10;

/// A character a save pass could not save. The pass saved every other one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveFailure {
    pub zone: MatchId,
    pub player: u32,
    /// Failed passes in a row, this one included.
    pub consecutive: u32,
    pub error: String,
    /// This pass logs the session out, after [`MAX_CONSECUTIVE_SAVE_FAILURES`].
    pub log_out: bool,
}

/// What one save pass could not do. Everything else it saved.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SaveReport {
    pub failed: Vec<SaveFailure>,
    /// Zones whose characters could not be read this pass; their leases are kept.
    pub unread: Vec<(MatchId, String)>,
}

/// The characters a save pass captured from each hosted zone.
pub(crate) type Captured = Vec<(
    MatchId,
    Result<Vec<crate::character_admission::StoredCharacter>, String>,
)>;

/// Why a character could not be stored.
#[derive(Debug)]
pub(crate) enum PersistError {
    /// A newer epoch of the lease's own zone holds the character: another host
    /// took the zone over, and this one must stop writing.
    Superseded(String),
    /// Only this character: retried on the next pass.
    Character(StoreError),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Superseded(error) => {
                write!(formatter, "a newer host holds this zone: {error}")
            }
            Self::Character(error) => error.fmt(formatter),
        }
    }
}

pub struct DevCharacterStorage<B: EntryBackend = FileEntries> {
    ownership: Option<Arc<File>>,
    /// The host epoch every zone's leases use in file mode.
    epoch: u64,
    /// Per-zone lease epochs (handoff mode: the zones' control-plane grants).
    zone_epochs: BTreeMap<ZoneId, u64>,
    pub(crate) store: CharacterStore<B>,
    pub(crate) leases: BTreeMap<Key, CharacterLease>,
    released: BTreeSet<Key>,
    /// Consecutive failed save passes per character still failing.
    failures: BTreeMap<Key, u32>,
    /// Each session's latest connection epoch, to log it out after failed saves.
    sessions: BTreeMap<Key, u32>,
}

impl DevCharacterStorage<FileEntries> {
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
            ownership: Some(Arc::new(ownership)),
            epoch,
            zone_epochs: BTreeMap::new(),
            store,
            leases: BTreeMap::new(),
            released: BTreeSet::new(),
            failures: BTreeMap::new(),
            sessions: BTreeMap::new(),
        })
    }
}

impl<B: EntryBackend> DevCharacterStorage<B> {
    /// A store shared by several hosts: each zone's leases use that zone's
    /// control-plane epoch. A held lease of an older epoch of the same zone
    /// is superseded on the next acquisition.
    pub fn shared(store: CharacterStore<B>, zone_epochs: BTreeMap<ZoneId, u64>) -> Self {
        Self {
            ownership: None,
            epoch: 0,
            zone_epochs,
            store,
            leases: BTreeMap::new(),
            released: BTreeSet::new(),
            failures: BTreeMap::new(),
            sessions: BTreeMap::new(),
        }
    }

    pub(crate) fn holder(&self, zone_id: ZoneId) -> LeaseHolder {
        LeaseHolder {
            zone_id,
            zone_epoch: self
                .zone_epochs
                .get(&zone_id)
                .copied()
                .unwrap_or(self.epoch),
        }
    }

    /// Reacquires restored bindings while the prepared host is still stopped.
    pub fn restore_snapshot(&mut self, id: &MatchId, payload: &[u8]) -> Result<(), String> {
        for stored in stored_characters(id, payload)? {
            let key = (id.clone(), stored.player);
            self.persist(key, stored).map_err(message)?;
        }
        Ok(())
    }

    fn persist(
        &mut self,
        key: Key,
        stored: crate::character_admission::StoredCharacter,
    ) -> Result<(), PersistError> {
        if self.released.contains(&key) {
            return Ok(());
        }
        let lease = if let Some(lease) = self.leases.get(&key) {
            lease.clone()
        } else {
            let holder = self.holder(stored.record.location.zone_id);
            let acquired = self
                .store
                .acquire(&stored.record.character_id, holder)
                .map_err(|error| self.classify(&stored.record.character_id, holder, error))?;
            self.leases.insert(key.clone(), acquired.lease.clone());
            acquired.lease
        };
        let written = if stored.parked {
            self.store.release(&lease, Some(&stored.record))
        } else {
            self.store.commit(&lease, &stored.record)
        };
        written.map_err(|error| self.classify(&lease.character_id, lease.holder, error))?;
        if stored.parked {
            self.leases.remove(&key);
            self.released.insert(key);
        }
        Ok(())
    }

    /// A fenced or stale lease whose character a newer epoch of the same zone
    /// now holds means this host lost the zone; any other failure is the
    /// character's own.
    fn classify(&self, id: &CharacterId, ours: LeaseHolder, error: StoreError) -> PersistError {
        let newer = |holder: LeaseHolder| {
            holder.zone_id == ours.zone_id && holder.zone_epoch > ours.zone_epoch
        };
        let superseded = match &error {
            StoreError::FencedEpoch(fence) => newer(*fence),
            StoreError::StaleLease => self
                .store
                .committed(id)
                .ok()
                .flatten()
                .and_then(|committed| committed.leased_by)
                .is_some_and(newer),
            _ => false,
        };
        if superseded {
            PersistError::Superseded(error.to_string())
        } else {
            PersistError::Character(error)
        }
    }

    /// Remembers the connection a session's control requests came from.
    pub(crate) fn track_session(&mut self, zone: &MatchId, context: ControlContext) {
        self.sessions
            .insert((zone.clone(), context.player_id), context.connection_epoch);
    }

    /// One save pass over what every zone holds now (#1145). Saves every
    /// character it can, keeps retrying the rest on later passes and marks
    /// for logout a live character that failed
    /// [`MAX_CONSECUTIVE_SAVE_FAILURES`] passes in a row. Leases of characters
    /// no longer live are returned to the store. Errs only when a newer host
    /// holds one of these zones: the host must stop writing and drain.
    pub(crate) fn save_pass(&mut self, captured: Captured) -> Result<SaveReport, String> {
        let mut report = SaveReport::default();
        let mut live = BTreeSet::new();
        let mut active = BTreeSet::new();
        let mut unread = BTreeSet::new();
        let mut errors = BTreeMap::new();
        for (zone, characters) in captured {
            let characters = match characters {
                Ok(characters) => characters,
                Err(error) => {
                    unread.insert(zone.clone());
                    report.unread.push((zone, error));
                    continue;
                }
            };
            for stored in characters {
                let key = (zone.clone(), stored.player);
                live.insert(key.clone());
                if !stored.parked {
                    active.insert(key.clone());
                }
                match self.persist(key.clone(), stored) {
                    Ok(()) => {}
                    Err(PersistError::Superseded(error)) => return Err(error),
                    Err(PersistError::Character(error)) => {
                        errors.insert(key, error.to_string());
                    }
                }
            }
        }
        // Abruptly lost sessions keep their last periodic commit. Stronger reward
        // acknowledgement/crash journaling belongs to #40, not this dev adapter.
        // A character leaving through a transfer is never live here, and its
        // handoff takes its lease out of this set before this storage owner is
        // free again (also while it offers the transfer to another host, #1098).
        let departed: Vec<_> = self
            .leases
            .iter()
            .filter(|(key, _)| !live.contains(*key) && !unread.contains(&key.0))
            .map(|(key, lease)| (key.clone(), lease.clone()))
            .collect();
        for (key, lease) in departed {
            match self.store.release(&lease, None) {
                Ok(_) => {}
                Err(error) => match self.classify(&lease.character_id, lease.holder, error) {
                    PersistError::Superseded(error) => return Err(error),
                    // The lease is no longer the character's: another zone holds it
                    // now (a handoff completed) or it was returned. Nothing is left
                    // to release.
                    PersistError::Character(StoreError::StaleLease) => {}
                    PersistError::Character(error) => {
                        errors.insert(key, error.to_string());
                        continue;
                    }
                },
            }
            self.leases.remove(&key);
        }
        self.released.retain(|key| live.contains(key));
        self.sessions.retain(|key, _| live.contains(key));
        let previous = std::mem::take(&mut self.failures);
        for (key, error) in errors {
            let consecutive = previous.get(&key).copied().unwrap_or(0).saturating_add(1);
            self.failures.insert(key.clone(), consecutive);
            report.failed.push(SaveFailure {
                log_out: active.contains(&key) && consecutive >= MAX_CONSECUTIVE_SAVE_FAILURES,
                zone: key.0,
                player: key.1,
                consecutive,
                error,
            });
        }
        Ok(report)
    }

    /// The connection epoch to log `key`'s session out with.
    pub(crate) fn session(&self, key: &Key) -> Option<u32> {
        self.sessions.get(key).copied()
    }

    /// Commits the final snapshots and returns every lease still held. Saves
    /// and releases everything it can, and reports the rest.
    pub(crate) fn finish(&mut self, snapshots: Vec<(MatchId, Vec<u8>)>) -> Result<(), String> {
        let mut errors = Vec::new();
        for (id, payload) in snapshots {
            let characters = match stored_characters(&id, &payload) {
                Ok(characters) => characters,
                Err(error) => {
                    errors.push(format!("{id}: {error}"));
                    continue;
                }
            };
            for stored in characters {
                let player = stored.player;
                if let Err(error) = self.persist((id.clone(), player), stored) {
                    errors.push(format!("{id} player {player}: {error}"));
                }
            }
        }
        let leases: Vec<_> = self
            .leases
            .iter()
            .map(|(key, lease)| (key.clone(), lease.clone()))
            .collect();
        for ((id, player), lease) in leases {
            match self.store.release(&lease, None) {
                Ok(_) => {
                    self.leases.remove(&(id, player));
                }
                Err(error) => errors.push(format!("{id} player {player}: {error}")),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "final character saves failed: {}",
                errors.join("; ")
            ))
        }
    }

    /// The directory lock a file-mode host's simulations hold.
    pub fn ownership(&self) -> Option<Arc<File>> {
        self.ownership.clone()
    }
}

pub(crate) fn zone_of(id: &MatchId) -> Result<ZoneId, String> {
    id.as_str()
        .strip_prefix("zone-")
        .ok_or("not a zone match")?
        .parse::<u32>()
        .map(ZoneId::new)
        .map_err(message)
}

fn stored_characters(
    id: &MatchId,
    payload: &[u8],
) -> Result<Vec<crate::character_admission::StoredCharacter>, String> {
    let content = mmorpg_core::content_for_zone(zone_of(id)?);
    ZoneGameServerAdapter::stored_characters(payload, content).map_err(message)
}

pub struct DevCharacterService<B: EntryBackend = FileEntries> {
    host: LiveMatchHost<ZoneGameServerAdapter>,
    storage: Arc<Mutex<DevCharacterStorage<B>>>,
    runtime: tokio::runtime::Handle,
    handoffs: Option<Arc<HandoffHost<B>>>,
}

impl<B: EntryBackend> Clone for DevCharacterService<B> {
    fn clone(&self) -> Self {
        Self {
            host: self.host.clone(),
            storage: Arc::clone(&self.storage),
            runtime: self.runtime.clone(),
            handoffs: self.handoffs.clone(),
        }
    }
}

impl<B: EntryBackend> DevCharacterService<B> {
    pub fn new(
        host: LiveMatchHost<ZoneGameServerAdapter>,
        storage: DevCharacterStorage<B>,
    ) -> Self {
        Self::with_shared_storage(host, Arc::new(Mutex::new(storage)), None)
    }

    pub(crate) fn with_shared_storage(
        host: LiveMatchHost<ZoneGameServerAdapter>,
        storage: Arc<Mutex<DevCharacterStorage<B>>>,
        handoffs: Option<Arc<HandoffHost<B>>>,
    ) -> Self {
        Self {
            host,
            storage,
            runtime: tokio::runtime::Handle::current(),
            handoffs,
        }
    }

    fn snapshot(
        &self,
        id: &MatchId,
    ) -> Result<Vec<crate::character_admission::StoredCharacter>, String> {
        // Captured straight from the live zone under the runtime lock (#1004);
        // the caller holds the storage owner, so logout and handoff stay ordered.
        self.runtime
            .block_on(self.host.inspect(id, |runtime| {
                runtime.simulation().capture_stored_characters()
            }))
            .map_err(message)?
            .map_err(message)
    }

    /// Called from a blocking worker, never the simulation tick or async executor.
    /// Snapshots and claims serialize through this storage owner; I/O never owns a runtime lock.
    ///
    /// Saves every character it can and reports the rest (#1145); a character
    /// that keeps failing is logged out, alone. Errs only when the host must
    /// stop: a newer host holds one of its zones, or the storage owner is
    /// poisoned.
    pub fn save_all(&self) -> Result<SaveReport, String> {
        let mut storage = self.storage.lock().map_err(message)?;
        let captured = self
            .runtime
            .block_on(self.host.statuses())
            .into_iter()
            .map(|status| {
                let characters = self.snapshot(&status.id);
                (status.id, characters)
            })
            .collect();
        let mut report = storage.save_pass(captured)?;
        for failure in report.failed.iter_mut().filter(|failure| failure.log_out) {
            let key = (failure.zone.clone(), failure.player);
            let logged_out = match storage.session(&key) {
                Some(epoch) => self
                    .runtime
                    .block_on(self.host.submit_host_input(&key.0, key.1, epoch, &[1, 2]))
                    .map_err(message),
                None => Err("no known connection".into()),
            };
            if let Err(error) = logged_out {
                failure.log_out = false;
                failure.error = format!("{}; logout failed: {error}", failure.error);
            }
        }
        Ok(report)
    }

    /// Commits the final snapshots captured from graceful recovery after serving
    /// stops, then returns all character leases offline. Saves and releases
    /// everything it can before reporting what failed.
    pub fn finish_snapshots(&self, snapshots: Vec<(MatchId, Vec<u8>)>) -> Result<(), String> {
        self.storage.lock().map_err(message)?.finish(snapshots)
    }

    fn claim(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        let (&appearance, raw_id) = payload.split_first().ok_or("missing character claim")?;
        let class =
            ClassChoice::from_appearance(u32::from(appearance)).ok_or("invalid character class")?;
        let character = CharacterId::new(std::str::from_utf8(raw_id).map_err(message)?.to_owned())
            .map_err(message)?;
        let zone = zone_of(id)?;
        let mut storage = self.storage.lock().map_err(message)?;
        let key = (id.clone(), context.player_id);
        if let Some(lease) = storage.leases.get(&key) {
            return if lease.character_id == character {
                Ok(vec![1, 1])
            } else {
                Err("session already has a character".into())
            };
        }
        if let Some(handoffs) = &self.handoffs
            && handoffs.claim_arrival(&mut storage, id, context, &character, class)?
        {
            return Ok(vec![1, 1]);
        }
        if mmorpg_core::redbrand_depths::is_depths_zone(zone) {
            return Err("the depths admit characters only through their threshold".into());
        }
        let holder = storage.holder(zone);
        let acquired = match (storage.store.acquire(&character, holder), &self.handoffs) {
            (Err(StoreError::HandoffPending(_)), Some(handoffs)) => {
                handoffs.resolve_pending(&mut storage, zone, &character)?
            }
            (acquired, _) => acquired.map_err(message)?,
        };
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

impl<B: EntryBackend + Send + 'static> MatchControlService for DevCharacterService<B> {
    fn handle(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, ControlServiceError> {
        if let Some(handoffs) = &self.handoffs {
            handoffs.track_session(id, context);
        }
        if let Ok(mut storage) = self.storage.lock() {
            storage.track_session(id, context);
        }
        let result = match payload {
            [1, 0] => Ok(vec![1, 1]), // development character capability
            [1, 1, claim @ ..] => self.claim(id, context, claim),
            [1, 2] => {
                self.runtime
                    .block_on(self.host.submit_host_input(
                        id,
                        context.player_id,
                        context.connection_epoch,
                        &[1, 2],
                    ))
                    .map_err(message)
                    .and_then(|()| self.save_all())
                    .and_then(|report| {
                        // Only this character's own save decides its logout's answer;
                        // a failed one is retried on the next pass.
                        match report.failed.into_iter().find(|failure| {
                            &failure.zone == id && failure.player == context.player_id
                        }) {
                            Some(failure) => Err(failure.error),
                            None => Ok(vec![1, 2]),
                        }
                    })
            }
            _ => Err("unsupported character control request".into()),
        };
        result.map_err(ControlServiceError::new)
    }
}

pub(crate) fn message(error: impl std::fmt::Display) -> String {
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

    /// Shared in-memory entries whose writes fail for the characters in `failing`,
    /// as a statement over its timeout would.
    #[derive(Clone, Default)]
    struct MemoryEntries {
        entries: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
        failing: Arc<Mutex<BTreeSet<String>>>,
    }

    impl EntryBackend for MemoryEntries {
        fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
            Ok(self.entries.lock().unwrap().get(id.as_str()).cloned())
        }

        fn update(
            &mut self,
            id: &CharacterId,
            change: &mut mmorpg_characters::EntryChange<'_>,
        ) -> Result<(), StoreError> {
            if self.failing.lock().unwrap().contains(id.as_str()) {
                return Err(StoreError::Backend("statement timeout".into()));
            }
            let mut entries = self.entries.lock().unwrap();
            let next = change(entries.get(id.as_str()).map(Vec::as_slice))?;
            entries.insert(id.as_str().to_owned(), next);
            Ok(())
        }
    }

    const ZONE: ZoneId = ZoneId::new(1);
    const CLASS: ClassChoice = ClassChoice {
        class: mmorpg_core::PlayerClass::Warden,
        sex: mmorpg_core::Sex::Female,
    };

    fn zone() -> MatchId {
        crate::zone_match_id(ZONE).unwrap()
    }

    fn storage(entries: &MemoryEntries) -> DevCharacterStorage<MemoryEntries> {
        DevCharacterStorage::shared(
            CharacterStore::new(entries.clone()),
            BTreeMap::from([(ZONE, 1)]),
        )
    }

    /// Players 1, 2 and 3 as characters "player-1" … in zone 1.
    fn characters(parked: &[u32]) -> Captured {
        let mut simulation =
            ZoneSimulation::with_content(ZONE, mmorpg_core::content_for_zone(ZONE)).unwrap();
        let stored = (1..=3)
            .map(|player| {
                simulation.create_character(player, CLASS).unwrap();
                let id = CharacterId::new(format!("player-{player}")).unwrap();
                crate::character_admission::StoredCharacter {
                    player,
                    parked: parked.contains(&player),
                    record: simulation.capture_character(player, id).unwrap(),
                }
            })
            .collect();
        vec![(zone(), Ok(stored))]
    }

    fn revision(entries: &MemoryEntries, player: u32) -> Option<u64> {
        CharacterStore::new(entries.clone())
            .committed(&CharacterId::new(format!("player-{player}")).unwrap())
            .unwrap()
            .map(|committed| committed.revision)
    }

    #[test]
    fn one_failing_character_does_not_stop_the_others_saving() {
        let entries = MemoryEntries::default();
        let mut storage = storage(&entries);
        storage.save_pass(characters(&[])).unwrap();
        let before = [revision(&entries, 1), revision(&entries, 3)];
        entries.failing.lock().unwrap().insert("player-2".into());

        let report = storage.save_pass(characters(&[])).unwrap();
        assert_eq!(report.failed.len(), 1);
        assert_eq!(
            (report.failed[0].player, report.failed[0].consecutive),
            (2, 1)
        );
        assert!(!report.failed[0].log_out);
        // Player 3 comes after the failing character and still saved.
        let after = [revision(&entries, 1), revision(&entries, 3)];
        assert_eq!(
            after,
            before.map(|revision| revision.map(|value| value + 1))
        );

        // Retried on the next pass, and saved once the store recovers.
        entries.failing.lock().unwrap().clear();
        let healed = revision(&entries, 2);
        assert_eq!(
            storage.save_pass(characters(&[])).unwrap(),
            SaveReport::default()
        );
        assert_eq!(revision(&entries, 2), healed.map(|value| value + 1));
    }

    #[test]
    fn a_character_that_keeps_failing_logs_out_only_its_session() {
        let entries = MemoryEntries::default();
        let mut storage = storage(&entries);
        storage.save_pass(characters(&[])).unwrap();
        entries.failing.lock().unwrap().insert("player-2".into());
        for pass in 1..=MAX_CONSECUTIVE_SAVE_FAILURES {
            let report = storage.save_pass(characters(&[])).unwrap();
            let [failure] = report.failed.as_slice() else {
                panic!("only player 2 fails: {report:?}");
            };
            assert_eq!((failure.player, failure.consecutive), (2, pass));
            assert_eq!(failure.log_out, pass == MAX_CONSECUTIVE_SAVE_FAILURES);
        }
        // Logged out (parked), its save keeps being retried without another logout.
        let report = storage.save_pass(characters(&[2])).unwrap();
        assert_eq!(report.failed.len(), 1);
        assert!(!report.failed[0].log_out);
        assert!(storage.leases.contains_key(&(zone(), 2)));
        // Once the store recovers, the parked character is saved and returned.
        entries.failing.lock().unwrap().clear();
        assert_eq!(
            storage.save_pass(characters(&[2])).unwrap(),
            SaveReport::default()
        );
        assert!(!storage.leases.contains_key(&(zone(), 2)));
        assert_eq!(storage.leases.len(), 2);
    }

    #[test]
    fn a_departed_character_keeps_its_lease_until_its_release_succeeds() {
        let entries = MemoryEntries::default();
        let mut storage = storage(&entries);
        storage.save_pass(characters(&[])).unwrap();
        entries.failing.lock().unwrap().insert("player-2".into());
        let (id, stored) = characters(&[]).pop().unwrap();
        let mut stored = stored.unwrap();
        stored.retain(|stored| stored.player != 2);
        let report = storage.save_pass(vec![(id.clone(), Ok(stored))]).unwrap();
        assert_eq!(report.failed.len(), 1);
        assert!(!report.failed[0].log_out, "no session is left to log out");
        assert!(storage.leases.contains_key(&(id.clone(), 2)));
        entries.failing.lock().unwrap().clear();
        storage
            .save_pass(vec![(id.clone(), Ok(Vec::new()))])
            .unwrap();
        assert!(storage.leases.is_empty());
        // A zone that could not be read keeps its characters' leases.
        storage.save_pass(characters(&[])).unwrap();
        let report = storage
            .save_pass(vec![(id, Err("zone unavailable".into()))])
            .unwrap();
        assert_eq!(report.unread.len(), 1);
        assert_eq!(storage.leases.len(), 3);
    }

    #[test]
    fn a_newer_host_holding_the_zone_stops_the_save_pass() {
        let entries = MemoryEntries::default();
        let mut storage = storage(&entries);
        storage.save_pass(characters(&[])).unwrap();
        // A newer epoch of zone 1 takes player 2 over: this host lost the zone.
        let mut successor = CharacterStore::new(entries.clone());
        successor
            .acquire(
                &CharacterId::new("player-2").unwrap(),
                LeaseHolder {
                    zone_id: ZONE,
                    zone_epoch: 2,
                },
            )
            .unwrap();
        assert!(storage.save_pass(characters(&[])).is_err());
        // The final drain still saves and releases what this host still holds,
        // and the store keeps refusing the fenced character.
        let final_error = storage.finish(Vec::new()).unwrap_err();
        assert!(final_error.contains("player 2"), "{final_error}");
        assert_eq!(storage.leases.len(), 1);
        assert_eq!(
            successor
                .committed(&CharacterId::new("player-1").unwrap())
                .unwrap()
                .unwrap()
                .leased_by,
            None
        );
    }
}
