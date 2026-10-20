//! Bounded storage worker behind game-server's publication gate. The worker owns
//! PostgreSQL clients for their entire lifetime, including destruction. It never
//! holds a simulation/runtime lock or computes gameplay outcomes.
use super::{
    CharacterWrite,
    checkpoints::{CheckpointError, CheckpointLimits, CheckpointObjects, S3Config},
    postgres::{JournalLimits, PostgresJournal, StreamFence},
};
use crate::ZoneGameServerAdapter;
use game_server::{
    MatchId, MatchRuntime, PersistenceError, RecoveryImage, SnapshotCommitBatch,
    SnapshotCommitFuture, SnapshotCommitter,
};
use mmorpg_characters::{Acquired, LeaseHolder, StoreError};
use mmorpg_core::{CharacterId, ZoneContent};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc as blocking,
    },
    thread,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

type Reply<T> = oneshot::Sender<Result<T, PersistenceError>>;

pub(crate) enum Durability {
    Development,
    Replicated { standby_url: String },
}
pub(crate) enum Startup {
    Bootstrap(RecoveryImage),
    Recover,
}
pub(crate) struct HostedPersistenceConfig {
    pub primary_url: String,
    pub durability: Durability,
    pub database_timeout: Duration,
    pub fence: StreamFence,
    pub journal_limits: JournalLimits,
    pub s3: S3Config,
    pub checkpoint_limits: CheckpointLimits,
    pub checkpoint_interval_ticks: u64,
    pub content: Arc<ZoneContent>,
    pub match_id: MatchId,
    pub startup: Startup,
    pub authority: Option<Arc<mmorpg_control_plane::LocalZonePermit>>,
}

pub(crate) struct HostedPersistence {
    requests: mpsc::Sender<Request>,
    cancelled: Arc<AtomicBool>,
    cancellation: blocking::SyncSender<Cancel>,
}
pub(crate) struct PersistenceWorker {
    requests: mpsc::Sender<Request>,
    cancelled: Arc<AtomicBool>,
    cancellation: blocking::SyncSender<Cancel>,
    thread: Option<thread::JoinHandle<()>>,
}
enum Cancel {
    Query,
    Stop,
}
enum Request {
    Tick {
        batch: SnapshotCommitBatch,
        reply: Reply<()>,
    },
    Claim {
        id: CharacterId,
        player: game_server::PlayerId,
        reply: Reply<Acquired>,
    },
    Checkpoint {
        reply: Reply<()>,
    },
    Stop,
}

/// Dropping any in-flight persistence request permanently fences this worker.
/// Cancellation is sent to a separately owned blocking canceller, never executed
/// on an async executor thread. PostgreSQL may already have committed: recovery,
/// rather than another live tick, resolves that uncertainty.
struct Pending<'a> {
    service: &'a HostedPersistence,
    complete: bool,
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if !self.complete {
            self.service.cancel();
        }
    }
}
impl HostedPersistence {
    pub(crate) async fn start(
        config: HostedPersistenceConfig,
    ) -> Result<(Arc<Self>, RecoveryImage, PersistenceWorker), PersistenceError> {
        if matches!(&config.durability, Durability::Replicated { .. }) && config.authority.is_none()
        {
            return Err(error(
                "replicated zone persistence requires a confirmed local authority permit",
            ));
        }
        if let Some(permit) = &config.authority {
            if !permit.lease().same_authority(&config.fence.lease) {
                return Err(error("storage permit differs from journal authority"));
            }
            permit
                .ensure_current(config.fence.lease.zone_id)
                .map_err(error)?;
        }
        if config.checkpoint_interval_ticks == 0 {
            return Err(error("checkpoint interval must be positive"));
        }
        let (requests, receiver) = mpsc::channel(1);
        let (cancellation, cancellations) = blocking::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let service = Arc::new(Self {
            requests: requests.clone(),
            cancelled: Arc::clone(&cancelled),
            cancellation: cancellation.clone(),
        });
        let worker_cancelled = Arc::clone(&cancelled);
        let (ready, startup) = oneshot::channel();
        let worker_cancellation = cancellation.clone();
        let thread = thread::Builder::new()
            .name("mmorpg-zone-persistence".into())
            .spawn(move || {
                run(
                    config,
                    receiver,
                    worker_cancelled,
                    cancellations,
                    worker_cancellation,
                    ready,
                );
            })
            .map_err(error)?;
        let driver = PersistenceWorker {
            requests,
            cancelled,
            cancellation,
            thread: Some(thread),
        };
        let image = startup.await.map_err(error)??;
        Ok((service, image, driver))
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.cancellation.try_send(Cancel::Query);
        let _ = self.requests.try_send(Request::Stop);
    }
    async fn request<T>(
        &self,
        make: impl FnOnce(Reply<T>) -> Request,
    ) -> Result<T, PersistenceError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(error("zone persistence is fenced"));
        }
        let mut pending = Pending {
            service: self,
            complete: false,
        };
        let (reply, response) = oneshot::channel();
        self.requests.send(make(reply)).await.map_err(error)?;
        let result = response.await.map_err(error)?;
        pending.complete = true;
        result
    }
    pub(crate) async fn claim(
        &self,
        id: CharacterId,
        player: game_server::PlayerId,
    ) -> Result<Acquired, PersistenceError> {
        self.request(|reply| Request::Claim { id, player, reply })
            .await
    }
    /// Lifecycle capture uses the last committed immutable image. The caller
    /// drains/stops the live host before requesting its final checkpoint.
    pub(crate) async fn checkpoint(&self) -> Result<(), PersistenceError> {
        self.request(|reply| Request::Checkpoint { reply }).await
    }
}
impl SnapshotCommitter for HostedPersistence {
    fn commit<'a>(&'a self, batch: &'a SnapshotCommitBatch) -> SnapshotCommitFuture<'a> {
        Box::pin(async move {
            self.request(|reply| Request::Tick {
                batch: SnapshotCommitBatch {
                    match_id: batch.match_id.clone(),
                    snapshot: batch.snapshot.clone(),
                    journal: batch.journal.clone(),
                    checkpoint: batch.checkpoint.clone(),
                },
                reply,
            })
            .await
        })
    }
}
impl PersistenceWorker {
    pub(crate) async fn shutdown(mut self) -> Result<(), PersistenceError> {
        let _ = self.requests.send(Request::Stop).await;
        if let Some(worker) = self.thread.take() {
            tokio::task::spawn_blocking(move || {
                worker
                    .join()
                    .map_err(|_| error("persistence worker panicked"))
            })
            .await
            .map_err(error)??;
        }
        Ok(())
    }
}
impl Drop for PersistenceWorker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.cancellation.try_send(Cancel::Query);
        let _ = self.requests.try_send(Request::Stop);
    }
}

struct Binding {
    player: game_server::PlayerId,
    acquired: Acquired,
    seen: bool,
}
struct State {
    journal: PostgresJournal,
    objects: CheckpointObjects,
    runtime: tokio::runtime::Runtime,
    fence: StreamFence,
    content: Arc<ZoneContent>,
    match_id: MatchId,
    position: u64,
    last_image: RecoveryImage,
    checkpoint_interval: u64,
    last_attempt_tick: u64,
    bindings: BTreeMap<CharacterId, Binding>,
    cancelled: Arc<AtomicBool>,
    authority: Option<Arc<mmorpg_control_plane::LocalZonePermit>>,
}
impl State {
    fn ensure_authority(&self) -> Result<(), PersistenceError> {
        if let Some(permit) = &self.authority {
            permit
                .ensure_current(self.fence.lease.zone_id)
                .map_err(error)?;
        }
        Ok(())
    }
    fn claim(
        &mut self,
        id: CharacterId,
        player: game_server::PlayerId,
    ) -> Result<Acquired, StoreError> {
        self.ensure_authority()
            .map_err(|cause| StoreError::Backend(cause.to_string()))?;
        let holder = LeaseHolder {
            zone_id: self.fence.lease.zone_id,
            zone_epoch: self.fence.lease.epoch,
        };
        if self.bindings.contains_key(&id) {
            return Err(StoreError::Leased(holder));
        }
        let acquired = self
            .journal
            .characters(|store| store.acquire(&id, holder))?;
        self.ensure_authority()
            .map_err(|cause| StoreError::Backend(cause.to_string()))?;
        self.bindings.insert(
            id,
            Binding {
                player,
                acquired: acquired.clone(),
                seen: false,
            },
        );
        Ok(acquired)
    }
    fn tick(&mut self, batch: SnapshotCommitBatch) -> Result<(), PersistenceError> {
        self.ensure_authority()?;
        if batch.match_id != self.match_id || batch.snapshot.tick != batch.checkpoint.current_tick {
            return Err(error("commit batch differs from hosted zone/tick"));
        }
        let (zone, stored) = ZoneGameServerAdapter::decode_stored_zone(
            &batch.snapshot.payload,
            Arc::clone(&self.content),
        )
        .map_err(error)?;
        if zone.zone_id() != self.fence.lease.zone_id || zone.current_tick() != batch.snapshot.tick
        {
            return Err(error("canonical commit differs from fenced zone/tick"));
        }
        // Old parked sessions can share a durable identity with a newer active
        // admission. The active body's core facts take precedence.
        let mut records = BTreeMap::new();
        for stored in stored {
            let id = stored.record.character_id.clone();
            if records.get(&id).is_none_or(
                |previous: &crate::character_admission::StoredCharacter| {
                    previous.parked || !stored.parked
                },
            ) {
                records.insert(id, stored);
            }
        }
        let mut writes = Vec::new();
        for (id, stored) in &records {
            if !stored.parked
                && self
                    .bindings
                    .get(id)
                    .is_none_or(|binding| binding.player != stored.player)
            {
                return Err(error("active character has no storage lease"));
            }
        }
        for (id, binding) in &mut self.bindings {
            let stored = records
                .get(id)
                .filter(|stored| stored.player == binding.player);
            let release =
                stored.is_some_and(|stored| stored.parked) || (binding.seen && stored.is_none());
            let record = stored
                .map(|stored| &stored.record)
                .or(binding.acquired.record.as_ref());
            if let Some(record) = record {
                if release || binding.acquired.record.as_ref() != Some(record) {
                    writes.push(CharacterWrite {
                        lease: binding.acquired.lease.clone(),
                        expected_revision: binding.acquired.revision,
                        record: record.clone(),
                        grants: BTreeSet::new(),
                        release,
                    });
                }
            }
            binding.seen |= stored.is_some();
        }
        self.position = self
            .journal
            .commit_tick(self.position, &batch, &writes)
            .map_err(error)?;
        self.ensure_authority()?;
        for write in writes {
            if write.release {
                self.bindings.remove(&write.lease.character_id);
            } else if let Some(binding) = self.bindings.get_mut(&write.lease.character_id) {
                binding.acquired.revision = write
                    .expected_revision
                    .checked_add(1)
                    .ok_or_else(|| error("character revision exhausted"))?;
                binding.acquired.record = Some(write.record);
            }
        }
        self.last_image = batch.checkpoint;
        if self
            .last_image
            .current_tick
            .saturating_sub(self.last_attempt_tick)
            >= self.checkpoint_interval
        {
            match self.checkpoint() {
                Ok(()) => {}
                Err(CheckpointFailure::Unavailable) => {} // retained durable journal remains bounded by PostgresJournal
                Err(CheckpointFailure::Fatal(error)) => return Err(error),
            }
        }
        self.ensure_authority()
    }
    fn checkpoint(&mut self) -> Result<(), CheckpointFailure> {
        self.ensure_authority().map_err(CheckpointFailure::Fatal)?;
        self.last_attempt_tick = self.last_image.current_tick;
        let prepared = self
            .objects
            .prepare(
                &self.fence,
                self.position,
                self.content.fingerprint(),
                u16::from(mmorpg_protocol::SNAPSHOT_WIRE_VERSION),
                &self.last_image,
            )
            .map_err(checkpoint_error)?;
        let verified = object_io(
            &self.runtime,
            &self.cancelled,
            self.objects.upload(&prepared),
        )
        .map_err(checkpoint_error)?;
        self.ensure_authority().map_err(CheckpointFailure::Fatal)?;
        self.journal
            .publish_checkpoint(&verified)
            .map_err(|cause| CheckpointFailure::Fatal(error(cause)))?;
        self.journal
            .prune()
            .map_err(|cause| CheckpointFailure::Fatal(error(cause)))?;
        self.ensure_authority().map_err(CheckpointFailure::Fatal)
    }
}
enum CheckpointFailure {
    Unavailable,
    Fatal(PersistenceError),
}
fn checkpoint_error(cause: CheckpointError) -> CheckpointFailure {
    match cause {
        CheckpointError::Unavailable(_) => CheckpointFailure::Unavailable,
        CheckpointError::Invalid(_) => CheckpointFailure::Fatal(error(cause)),
    }
}
fn error(cause: impl std::fmt::Display) -> PersistenceError {
    PersistenceError::new(cause.to_string())
}

fn run(
    config: HostedPersistenceConfig,
    mut requests: mpsc::Receiver<Request>,
    cancelled: Arc<AtomicBool>,
    cancellations: blocking::Receiver<Cancel>,
    cancellation: blocking::SyncSender<Cancel>,
    ready: Reply<RecoveryImage>,
) {
    let journal = match &config.durability {
        Durability::Development => PostgresJournal::connect_development(
            &config.primary_url,
            config.fence.clone(),
            config.database_timeout,
            config.journal_limits,
        ),
        Durability::Replicated { standby_url } => PostgresJournal::connect_replicated(
            &config.primary_url,
            standby_url,
            config.fence.clone(),
            config.database_timeout,
            config.journal_limits,
        ),
    };
    let mut journal = match journal {
        Ok(journal) => journal,
        Err(cause) => {
            let _ = ready.send(Err(error(cause)));
            return;
        }
    };
    journal.set_cancellation(Arc::clone(&cancelled));
    let tokens = journal.cancel_tokens();
    let canceller = match thread::Builder::new()
        .name("mmorpg-persistence-cancel".into())
        .spawn(move || {
            while let Ok(command) = cancellations.recv() {
                match command {
                    Cancel::Query => {
                        for token in &tokens {
                            let _ = token.cancel_query(::postgres::NoTls);
                        }
                    }
                    Cancel::Stop => break,
                }
            }
        }) {
        Ok(thread) => thread,
        Err(cause) => {
            let _ = ready.send(Err(error(cause)));
            return;
        }
    };
    let result = initialize(config, journal, Arc::clone(&cancelled));
    match result {
        Err(cause) => {
            let _ = ready.send(Err(cause));
        }
        Ok(mut state) => {
            if ready.send(Ok(state.last_image.clone())).is_ok() {
                while !cancelled.load(Ordering::Acquire) {
                    let Some(request) = requests.blocking_recv() else {
                        break;
                    };
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let fatal = match request {
                        Request::Stop => break,
                        Request::Tick { batch, reply } => {
                            let result = state.tick(batch);
                            let failed = result.is_err();
                            let _ = reply.send(result);
                            failed
                        }
                        Request::Claim { id, player, reply } => {
                            let result = state.claim(id, player);
                            let failed = matches!(
                                &result,
                                Err(StoreError::Backend(_) | StoreError::Corrupt(_))
                            );
                            let _ = reply.send(result.map_err(error));
                            failed
                        }
                        Request::Checkpoint { reply } => {
                            let result = state.checkpoint();
                            let failed = matches!(&result, Err(CheckpointFailure::Fatal(_)));
                            let _ = reply.send(result.map_err(|cause| match cause {
                                CheckpointFailure::Unavailable => {
                                    error("checkpoint object store unavailable; journal retained")
                                }
                                CheckpointFailure::Fatal(cause) => cause,
                            }));
                            failed
                        }
                    };
                    if fatal {
                        break;
                    }
                }
            }
            // State (including synchronous PostgreSQL runtimes) dies here on
            // this storage thread, never on an async executor or request future.
        }
    }
    cancelled.store(true, Ordering::Release);
    let _ = cancellation.send(Cancel::Stop);
    let _ = canceller.join();
}

fn initialize(
    config: HostedPersistenceConfig,
    mut journal: PostgresJournal,
    cancelled: Arc<AtomicBool>,
) -> Result<State, PersistenceError> {
    let objects = CheckpointObjects::s3(config.s3, config.checkpoint_limits).map_err(error)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(error)?;
    let (position, image, bootstrap) = match config.startup {
        Startup::Bootstrap(image) => (0, image, true),
        Startup::Recover => {
            let reference = journal.checkpoint_reference().map_err(error)?;
            let image = object_io(
                &runtime,
                &cancelled,
                objects.load(
                    &config.fence,
                    &reference,
                    config.content.fingerprint(),
                    u16::from(mmorpg_protocol::SNAPSHOT_WIRE_VERSION),
                ),
            )
            .map_err(error)?;
            let (position, image) = journal.recover_suffix(&reference, image).map_err(error)?;
            (position, image, false)
        }
    };
    if cancelled.load(Ordering::Acquire) {
        return Err(error("persistence startup cancelled"));
    }
    // The shared replay interpreter, not storage, validates the authoritative
    // continuation before this image can be handed to a serving host.
    let restored = MatchRuntime::restore_from_recovery(
        ZoneGameServerAdapter::with_content(
            config.fence.lease.zone_id,
            Arc::clone(&config.content),
        )
        .map_err(error)?,
        image.clone(),
    )
    .map_err(error)?;
    let snapshot = restored.snapshot().map_err(error)?;
    let (_, records) =
        ZoneGameServerAdapter::decode_stored_zone(&snapshot.payload, Arc::clone(&config.content))
            .map_err(error)?;
    let tick = image.current_tick;
    let mut state = State {
        journal,
        objects,
        runtime,
        fence: config.fence,
        content: config.content,
        match_id: config.match_id,
        position,
        last_image: image,
        checkpoint_interval: config.checkpoint_interval_ticks,
        last_attempt_tick: tick,
        bindings: BTreeMap::new(),
        authority: config.authority,
        cancelled,
    };
    if bootstrap && !records.is_empty() {
        return Err(error(
            "bootstrap requires a world without durable character bindings",
        ));
    }
    for stored in records.into_iter().filter(|stored| !stored.parked) {
        let acquired = state
            .claim(stored.record.character_id.clone(), stored.player)
            .map_err(error)?;
        if acquired.record.as_ref() != Some(&stored.record) {
            return Err(error(
                "recovered character differs from global committed progression",
            ));
        }
        if let Some(binding) = state.bindings.get_mut(&stored.record.character_id) {
            binding.seen = true;
        }
    }
    if bootstrap {
        state.checkpoint().map_err(|cause| match cause {
            CheckpointFailure::Unavailable => {
                error("cannot bootstrap without a verified durable checkpoint")
            }
            CheckpointFailure::Fatal(cause) => cause,
        })?;
    }
    state.ensure_authority()?;
    Ok(state)
}

fn object_io<T>(
    runtime: &tokio::runtime::Runtime,
    cancelled: &AtomicBool,
    operation: impl std::future::Future<Output = Result<T, CheckpointError>>,
) -> Result<T, CheckpointError> {
    runtime.block_on(async {
        tokio::select! {
            biased;
            _ = async {
                while !cancelled.load(Ordering::Acquire) { tokio::time::sleep(Duration::from_millis(10)).await; }
            } => Err(CheckpointError::Invalid("checkpoint I/O cancelled; outcome unresolved".into())),
            result = operation => result,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::postgres::{install_writer, migrate, record_retirement};
    use super::*;
    use mmorpg_control_plane::{HostId, ZoneLease};
    use mmorpg_core::{ClassChoice, PlayerClass, Sex, ZoneId, ZoneSimulation, greyhaven_vale};

    #[tokio::test]
    async fn hosted_worker_commits_characters_and_recovers_from_s3_plus_journal() {
        let (Ok(primary), Ok(standby), Ok(endpoint), Ok(bucket)) = (
            std::env::var("MMORPG_CHARACTERS_POSTGRES_URL"),
            std::env::var("MMORPG_POSTGRES_STANDBY_URL"),
            std::env::var("MMORPG_CHECKPOINT_S3_ENDPOINT"),
            std::env::var("MMORPG_CHECKPOINT_S3_BUCKET"),
        ) else {
            return;
        };
        let setup_url = primary.clone();
        let fence = tokio::task::spawn_blocking(move || {
            let mut client = ::postgres::Client::connect(&setup_url, ::postgres::NoTls).unwrap();
            migrate(&mut client).unwrap();
            client
                .execute("SELECT pg_advisory_lock($1)", &[&0x4d4d_5a4a_5445_5354_i64])
                .unwrap();
            let zone: i64 = client
                .query_one(
                    "SELECT coalesce(max(zone_id),1000)+1 FROM mmorpg_persistence.streams",
                    &[],
                )
                .unwrap()
                .get(0);
            install_writer(
                &mut client,
                ZoneLease {
                    zone_id: ZoneId::new(zone.try_into().unwrap()),
                    host_id: HostId::new("hosted-test").unwrap(),
                    epoch: 1,
                    expires_at_tick: 1000,
                },
                0,
            )
            .unwrap()
        })
        .await
        .unwrap();
        let content = greyhaven_vale::content();
        let adapter = || {
            ZoneGameServerAdapter::with_content(fence.lease.zone_id, Arc::clone(&content)).unwrap()
        };
        let mut genesis = MatchRuntime::new_with_replay_capture(adapter(), 30);
        let initial = genesis.checkpoint_replay().unwrap();
        let make_config = |fence: StreamFence, startup| {
            let authority = Arc::new(
                mmorpg_control_plane::LocalZonePermit::from_confirmed_lease(
                    fence.lease.clone(),
                    std::time::Instant::now(),
                    Duration::from_secs(60),
                )
                .unwrap(),
            );
            HostedPersistenceConfig {
                primary_url: primary.clone(),
                durability: Durability::Replicated {
                    standby_url: standby.clone(),
                },
                database_timeout: Duration::from_secs(5),
                fence,
                journal_limits: JournalLimits {
                    max_batch_bytes: 1024 * 1024,
                    max_retained_bytes: 4 * 1024 * 1024,
                    max_retained_batches: 100,
                    max_replay_ticks: 100,
                },
                s3: S3Config {
                    endpoint: endpoint.clone(),
                    bucket: bucket.clone(),
                    region: "us-east-1".into(),
                    allow_http: true,
                },
                checkpoint_limits: CheckpointLimits {
                    max_compressed_bytes: 1024 * 1024,
                    max_decoded_bytes: 4 * 1024 * 1024,
                    timeout: Duration::from_secs(5),
                },
                checkpoint_interval_ticks: 50,
                content: Arc::clone(&content),
                match_id: MatchId::new("hosted-test").unwrap(),
                startup,
                authority: Some(authority),
            }
        };
        let (storage, ready, worker) = HostedPersistence::start(make_config(
            fence.clone(),
            Startup::Bootstrap(initial.clone()),
        ))
        .await
        .unwrap();
        assert_eq!(ready, initial);
        let character =
            CharacterId::new(format!("hosted-worker-{}", fence.lease.zone_id.get())).unwrap();
        let acquired = storage.claim(character.clone(), 1).await.unwrap();
        assert!(acquired.record.is_none());
        // A rejected duplicate claim is a routine refusal, not a storage failure.
        assert!(storage.claim(character.clone(), 1).await.is_err());
        let mut character_zone =
            ZoneSimulation::with_content(fence.lease.zone_id, Arc::clone(&content)).unwrap();
        character_zone
            .create_character(
                1,
                ClassChoice {
                    class: PlayerClass::Warden,
                    sex: Sex::Male,
                },
            )
            .unwrap();
        let mut record = character_zone
            .capture_character(1, character.clone())
            .unwrap();
        record.copper = 37;
        let mut live = MatchRuntime::new_with_replay_capture(adapter(), 30);
        let session = live.admit(game_server::ReconnectToken([71; 16])).unwrap();
        live.submit_host_input(
            session.player_id,
            &ZoneGameServerAdapter::character_admission_input(&record).unwrap(),
        )
        .unwrap();
        let snapshot = live.advance_tick().unwrap();
        let batch = SnapshotCommitBatch {
            match_id: MatchId::new("hosted-test").unwrap(),
            snapshot: snapshot.clone(),
            journal: live.replay_log().unwrap().clone(),
            checkpoint: live.checkpoint_replay().unwrap(),
        };
        storage.commit(&batch).await.unwrap();
        worker.shutdown().await.unwrap();
        drop(storage);
        // Discard the entire first worker/connection state. A separately fenced
        // successor reads the object + short journal and reacquires character
        // leases only after matching their global committed records.
        let setup_url = primary.clone();
        let old = fence.clone();
        let successor = tokio::task::spawn_blocking(move || {
            let mut client = ::postgres::Client::connect(&setup_url, ::postgres::NoTls).unwrap();
            record_retirement(&mut client, &old).unwrap();
            install_writer(
                &mut client,
                ZoneLease {
                    epoch: 2,
                    ..old.lease
                },
                1,
            )
            .unwrap()
        })
        .await
        .unwrap();
        let recovery_config = make_config(successor, Startup::Recover);
        let recovered_authority = Arc::clone(recovery_config.authority.as_ref().unwrap());
        let (storage, image, worker) = HostedPersistence::start(recovery_config).await.unwrap();
        let mut restored = MatchRuntime::restore_from_recovery(adapter(), image).unwrap();
        assert_eq!(restored.snapshot().unwrap(), snapshot);
        assert_eq!(
            restored.advance_tick().unwrap(),
            live.advance_tick().unwrap()
        );
        storage.checkpoint().await.unwrap();
        recovered_authority.revoke().unwrap();
        assert!(storage.checkpoint().await.is_err());
        worker.shutdown().await.unwrap();
    }
}
