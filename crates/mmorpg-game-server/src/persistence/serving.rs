//! Opt-in trusted-LAN host composition over the production durability adapters.
//! Public authentication/routing remains outside this local application boundary.
use super::{
    backend::{BackendConfig, GlobalBackend},
    checkpoints::{CheckpointLimits, CheckpointObjects, S3Config},
    global_worker::BackendWorker,
    hosted::{Durability, HostedPersistence, HostedPersistenceConfig, PersistenceWorker, Startup},
    postgres::JournalLimits,
    zone_lifecycle::ZoneLifecycle,
};
use crate::{ZoneGameServerAdapter, zone_match_id};
use game_server::{
    ControlContext, ControlServiceError, LiveMatchHost, MatchControlService, MatchHost,
    MatchHostStatusConfig, MatchHostWebTransportConfig, MatchId, MatchRuntime, SnapshotCommitBatch,
    SnapshotCommitFuture, SnapshotCommitter, SnapshotPersistence,
};
use mmorpg_core::{
    CharacterId, ClassChoice, ZoneId, ZoneSimulation, greyhaven_vale, long_ticks_from_ms,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
fn message(cause: impl std::fmt::Display) -> String {
    cause.to_string()
}
fn required(name: &str) -> Result<String> {
    let value = env::var(name).map_err(|_| format!("{name} must be configured"))?;
    if value.is_empty() {
        return Err(format!("{name} must not be empty").into());
    }
    Ok(value)
}
fn number<T: FromStr>(name: &str, default: T) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|cause| format!("{name}: {cause}").into()),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(cause) => Err(cause.into()),
    }
}
fn millis(name: &str, default: u64) -> Result<Duration> {
    let value = number(name, default)?;
    if value == 0 {
        return Err(format!("{name} must be positive").into());
    }
    Ok(Duration::from_millis(value))
}
fn backend_config() -> Result<BackendConfig> {
    Ok(BackendConfig {
        coordinator_primary_url: required("MMORPG_COORDINATOR_DATABASE_URL")?,
        coordinator_standby_url: required("MMORPG_COORDINATOR_STANDBY_URL")?,
        character_standby_url: required("MMORPG_CHARACTER_STANDBY_URL")?,
        timeout: millis("MMORPG_PERSISTENCE_TIMEOUT_MS", 5000)?,
    })
}
fn s3_config() -> Result<S3Config> {
    Ok(S3Config {
        endpoint: required("MMORPG_CHECKPOINT_S3_ENDPOINT")?,
        bucket: required("MMORPG_CHECKPOINT_S3_BUCKET")?,
        region: env::var("MMORPG_CHECKPOINT_S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
        allow_http: env::var("MMORPG_CHECKPOINT_ALLOW_HTTP").ok().as_deref() == Some("1"),
    })
}
fn checkpoint_limits() -> Result<CheckpointLimits> {
    Ok(CheckpointLimits {
        max_compressed_bytes: number("MMORPG_CHECKPOINT_MAX_COMPRESSED_BYTES", 16 * 1024 * 1024)?,
        max_decoded_bytes: number("MMORPG_CHECKPOINT_MAX_DECODED_BYTES", 64 * 1024 * 1024)?,
        timeout: millis("MMORPG_CHECKPOINT_TIMEOUT_MS", 5000)?,
    })
}
fn genesis(
    zone: ZoneId,
    grace: u64,
    known: u16,
    grants: u16,
) -> Result<game_server::RecoveryImage> {
    let mut adapter = ZoneGameServerAdapter::with_content(zone, greyhaven_vale::content())?;
    adapter.zone_mut().enable_reward_tracking(known, grants)?;
    Ok(MatchRuntime::new_with_replay_capture(adapter, grace).checkpoint_replay()?)
}

struct Stores(BTreeMap<MatchId, Arc<HostedPersistence>>);
impl SnapshotCommitter for Stores {
    fn commit<'a>(&'a self, batch: &'a SnapshotCommitBatch) -> SnapshotCommitFuture<'a> {
        Box::pin(async move {
            self.0
                .get(&batch.match_id)
                .ok_or_else(|| game_server::PersistenceError::new("zone has no persistence owner"))?
                .commit(batch)
                .await
        })
    }
}
#[derive(Clone)]
struct Characters {
    host: LiveMatchHost<ZoneGameServerAdapter>,
    stores: Arc<Stores>,
    serial: Arc<BTreeMap<MatchId, Mutex<()>>>,
    runtime: tokio::runtime::Handle,
    timeout: Duration,
}
impl Characters {
    async fn current(
        &self,
        id: &MatchId,
        player: u32,
    ) -> std::result::Result<Option<crate::character_admission::StoredCharacter>, String> {
        let snapshot = self
            .host
            .inspect(id, |runtime| runtime.snapshot())
            .await
            .map_err(message)?
            .map_err(message)?;
        Ok(ZoneGameServerAdapter::stored_characters(&snapshot.payload)
            .map_err(message)?
            .into_iter()
            .find(|stored| stored.player == player))
    }
    async fn wait(
        &self,
        store: &HostedPersistence,
        player: u32,
        character: Option<CharacterId>,
    ) -> std::result::Result<(), String> {
        tokio::time::timeout(self.timeout, async {
            loop {
                let done = match &character {
                    Some(id) => store.admitted(player, id.clone()).await,
                    None => store.released(player).await,
                }
                .map_err(message)?;
                if done {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| "character durable commit timed out".to_string())?
    }
    async fn clear_pending(
        &self,
        id: &MatchId,
        context: ControlContext,
        store: &HostedPersistence,
    ) -> std::result::Result<(), String> {
        if store
            .pending_character(context.player_id)
            .await
            .map_err(message)?
            .is_some()
        {
            self.host
                .submit_host_input(id, context.player_id, context.connection_epoch, &[1, 4])
                .await
                .map_err(message)?;
            store
                .cancel_pending(context.player_id)
                .await
                .map_err(message)?;
        }
        Ok(())
    }
    async fn claim(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> std::result::Result<Vec<u8>, String> {
        let (&appearance, raw) = payload.split_first().ok_or("missing character claim")?;
        let class =
            ClassChoice::from_appearance(u16::from(appearance)).ok_or("invalid character class")?;
        let character = CharacterId::new(std::str::from_utf8(raw).map_err(message)?.to_owned())
            .map_err(message)?;
        let store = self.stores.0.get(id).ok_or("unknown zone persistence")?;
        if let Some(current) = self.current(id, context.player_id).await?
            && !current.parked
        {
            if current.record.character_id != character || current.record.class != class {
                return Err("session already has a different character".into());
            }
            self.wait(store, context.player_id, Some(character)).await?;
            return Ok(vec![1, 1]);
        }
        if store
            .pending_character(context.player_id)
            .await
            .map_err(message)?
            .is_some_and(|pending| pending != character)
        {
            self.clear_pending(id, context, store).await?;
        }
        let acquired = store
            .claim(character.clone(), context.player_id)
            .await
            .map_err(message)?;
        let admission = async {
            let record = if let Some(record) = acquired.acquired.record {
                if record.class != class {
                    return Err("stored character does not match selected class/sex".into());
                }
                record
            } else {
                let zone = zone_id(id)?;
                let mut creation = ZoneSimulation::with_content(zone, greyhaven_vale::content())
                    .map_err(message)?;
                creation.create_character(1, class).map_err(message)?;
                creation
                    .capture_character(1, character.clone())
                    .map_err(message)?
            };
            for key in acquired.receipts {
                let input = ZoneGameServerAdapter::reward_receipt_input(key).map_err(message)?;
                self.host
                    .submit_host_input(id, context.player_id, context.connection_epoch, &input)
                    .await
                    .map_err(message)?;
            }
            let input =
                ZoneGameServerAdapter::character_admission_input(&record).map_err(message)?;
            self.host
                .submit_host_input(id, context.player_id, context.connection_epoch, &input)
                .await
                .map_err(message)?;
            self.wait(store, context.player_id, Some(character)).await?;
            Ok(vec![1, 1])
        }
        .await;
        if admission.is_err() {
            let _ = self.clear_pending(id, context, store).await;
        }
        admission
    }
}
impl MatchControlService for Characters {
    fn handle(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> std::result::Result<Vec<u8>, ControlServiceError> {
        let result = (|| -> std::result::Result<Vec<u8>, String> {
            if payload == [1, 0] {
                return Ok(vec![1, 1]);
            }
            let _serial = self
                .serial
                .get(id)
                .ok_or("unknown zone")?
                .lock()
                .map_err(message)?;
            self.runtime.block_on(async {
                match payload {
                    [1, 1, claim @ ..] => self.claim(id, context, claim).await,
                    [1, 2] => {
                        let store = self.stores.0.get(id).ok_or("unknown zone persistence")?;
                        if self
                            .current(id, context.player_id)
                            .await?
                            .is_some_and(|stored| !stored.parked)
                        {
                            self.host
                                .submit_host_input(
                                    id,
                                    context.player_id,
                                    context.connection_epoch,
                                    &[1, 2],
                                )
                                .await
                                .map_err(message)?;
                            self.wait(store, context.player_id, None).await?;
                        } else {
                            self.clear_pending(id, context, store).await?;
                        }
                        Ok(vec![1, 2])
                    }
                    _ => Err("unsupported character control request".into()),
                }
            })
        })();
        result.map_err(ControlServiceError::new)
    }
}
fn zone_id(id: &MatchId) -> std::result::Result<ZoneId, String> {
    id.as_str()
        .strip_prefix("zone-")
        .ok_or("invalid zone route")?
        .parse::<u32>()
        .map(ZoneId::new)
        .map_err(message)
}

/// Explicit opt-in application composition. The storage roles/standby and S3
/// destination come from environment configuration; there is no local fallback.
pub async fn serve_postgres_character_host(
    zones: Vec<ZoneId>,
    grace: u64,
    transport: MatchHostWebTransportConfig,
    status: MatchHostStatusConfig,
    mut shutdown: mpsc::Receiver<()>,
) -> Result<()> {
    let mode = required("MMORPG_PERSISTENCE_MODE")?;
    if mode == "migrate" {
        let url = required("MMORPG_MIGRATION_DATABASE_URL")?;
        tokio::task::spawn_blocking(move || -> std::result::Result<(), String> {
            let mut client =
                ::postgres::Client::connect(&url, ::postgres::NoTls).map_err(message)?;
            super::postgres::migrate(&mut client).map_err(message)
        })
        .await??;
        eprintln!("PostgreSQL persistence migrations applied.");
        return Ok(());
    }
    let unique: BTreeSet<_> = zones.iter().copied().collect();
    if zones.is_empty() || unique.len() != zones.len() {
        return Err("persistence host requires unique configured zones".into());
    }
    let backend = backend_config()?;
    if mode == "profile" {
        let id = CharacterId::new(required("MMORPG_PROFILE_CHARACTER")?)?;
        let profile = tokio::task::spawn_blocking(move || -> std::result::Result<_, String> {
            let mut backend = GlobalBackend::connect(backend).map_err(message)?;
            backend.committed_character(&id).map_err(message)
        })
        .await??;
        let value=profile.map(|profile|serde_json::json!({"character":profile.record.character_id.as_str(),"revision":profile.revision,"level":profile.record.level,"experience":profile.record.experience,"copper":profile.record.copper}));
        println!("{}", serde_json::to_string(&value)?);
        return Ok(());
    }
    if mode == "retire" {
        if zones.len() != 1
            || env::var("MMORPG_CONFIRM_SOURCE_STOPPED").ok().as_deref() != Some("1")
        {
            return Err("retire requires one zone and MMORPG_CONFIRM_SOURCE_STOPPED=1 after stopping its old host".into());
        }
        let epoch = required("MMORPG_EXPECTED_ZONE_EPOCH")?.parse::<u64>()?;
        let zone = zones[0];
        tokio::task::spawn_blocking(move || -> std::result::Result<(), String> {
            GlobalBackend::connect(backend)
                .map_err(message)?
                .confirm_stopped_assignment(zone, epoch)
                .map_err(message)
        })
        .await??;
        eprintln!("Stopped zone assignment retired.");
        return Ok(());
    }
    if !matches!(mode.as_str(), "bootstrap" | "recover" | "reset") {
        return Err(
            "MMORPG_PERSISTENCE_MODE must be bootstrap, recover, reset, migrate, profile or retire"
                .into(),
        );
    }
    let known = number("MMORPG_MAX_REWARD_RECEIPTS", 1024_u16)?;
    let tick_grants = number("MMORPG_MAX_TICK_GRANTS", 128_u16)?;
    let object_limits = checkpoint_limits()?;
    if mode == "reset" {
        if env::var("MMORPG_CONFIRM_SOURCE_STOPPED").ok().as_deref() != Some("1") {
            return Err(
                "reset requires MMORPG_CONFIRM_SOURCE_STOPPED=1 and previously retired assignments"
                    .into(),
            );
        }
        let mut baselines = Vec::new();
        for zone in &zones {
            baselines.push((*zone, genesis(*zone, grace, known, tick_grants)?));
        }
        let backend = backend.clone();
        let s3 = s3_config()?;
        tokio::task::spawn_blocking(move || -> std::result::Result<(), String> {
            let mut backend = GlobalBackend::connect(backend).map_err(message)?;
            let objects = CheckpointObjects::s3(s3, object_limits).map_err(message)?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(message)?;
            for (zone, image) in baselines {
                let previous = backend.retired_fence(zone, None).map_err(message)?;
                let prepared = objects
                    .prepare_world_reset(&previous, greyhaven_vale::content(), &image)
                    .map_err(message)?;
                let verified = runtime
                    .block_on(objects.upload_world_reset(&prepared))
                    .map_err(message)?;
                backend
                    .reset_retired(&previous, &verified)
                    .map_err(message)?;
            }
            Ok(())
        })
        .await??;
    }
    let zone_primary = required("MMORPG_ZONE_DATABASE_URL")?;
    let zone_standby = required("MMORPG_ZONE_STANDBY_URL")?;
    let database_timeout = millis("MMORPG_PERSISTENCE_TIMEOUT_MS", 5000)?;
    let commit_timeout = millis("MMORPG_COMMIT_TIMEOUT_MS", 25000)?;
    let policy = crate::lifecycle::SuspensionPolicy::new(
        Duration::from_secs(1),
        Duration::from_secs(30),
        Duration::from_secs(300),
    )
    .ok_or("invalid zone suspension policy")?;
    let proposed_grace = match env::var("MMORPG_EMPTY_ZONE_GRACE_MS") {
        Ok(value) => Some(Duration::from_millis(value.parse().map_err(message)?)),
        Err(env::VarError::NotPresent) => None,
        Err(cause) => return Err(cause.into()),
    };
    let ttl = millis("MMORPG_GRANT_TTL_MS", 30000)?;
    let renew = millis("MMORPG_GRANT_RENEW_MS", 10000)?;
    let interval = number(
        "MMORPG_CHECKPOINT_INTERVAL_TICKS",
        u64::from(long_ticks_from_ms(10_000)),
    )?;
    let limits = JournalLimits {
        max_batch_bytes: number("MMORPG_JOURNAL_MAX_BATCH_BYTES", 16 * 1024 * 1024)?,
        max_retained_bytes: number("MMORPG_JOURNAL_MAX_RETAINED_BYTES", 64 * 1024 * 1024)?,
        max_retained_batches: number("MMORPG_JOURNAL_MAX_BATCHES", 900)?,
        max_replay_ticks: number(
            "MMORPG_JOURNAL_MAX_REPLAY_TICKS",
            u64::from(long_ticks_from_ms(30_000)),
        )?,
    };
    let host_id = mmorpg_control_plane::HostId::new(required("MMORPG_HOST_ID")?)?;
    // Validate the S3 destination before granting any ownership.
    let _ = CheckpointObjects::s3(s3_config()?, object_limits)?;
    let (mut backend, grants) = BackendWorker::start(backend, zones, host_id, ttl, renew).await?;
    let mut workers = Vec::<PersistenceWorker>::new();
    let mut stores = BTreeMap::new();
    let mut matches = MatchHost::new(grants.len())?;
    let mut permits = Vec::new();
    let prepared = async {
        for grant in grants {
            let zone = grant.fence.lease.zone_id;
            let id = zone_match_id(zone)?;
            let startup = if mode == "bootstrap" {
                Startup::Bootstrap(genesis(zone, grace, known, tick_grants)?)
            } else {
                Startup::Recover
            };
            let (store, image, worker) = HostedPersistence::start(HostedPersistenceConfig {
                primary_url: zone_primary.clone(),
                durability: Durability::Production {
                    standby_url: zone_standby.clone(),
                },
                database_timeout,
                fence: grant.fence,
                journal_limits: limits,
                s3: s3_config()?,
                checkpoint_limits: object_limits,
                checkpoint_interval_ticks: interval,
                content: greyhaven_vale::content(),
                match_id: id.clone(),
                startup,
                authority: Some(Arc::clone(&grant.permit)),
            })
            .await?;
            workers.push(worker);
            stores.insert(id.clone(), store);
            let mut adapter = ZoneGameServerAdapter::with_content(zone, greyhaven_vale::content())?
                .with_authority(Arc::clone(&grant.permit))?;
            adapter.require_durable_character_claims();
            matches.insert(id, MatchRuntime::restore_from_recovery(adapter, image)?)?;
            permits.push(grant.permit);
        }
        Ok::<(), Box<dyn Error + Send + Sync>>(())
    }
    .await;
    if let Err(cause) = prepared {
        for worker in workers {
            let _ = worker.shutdown().await;
        }
        let _ = backend.shutdown().await;
        return Err(cause);
    }
    let stores = Arc::new(Stores(stores));
    let persistence = SnapshotPersistence::new(
        stores.clone(),
        commit_timeout,
        usize::try_from(limits.max_batch_bytes)?,
    )?;
    let lifecycle = Arc::new(ZoneLifecycle::new(
        stores.0.clone(),
        permits
            .iter()
            .map(|permit| zone_match_id(permit.lease().zone_id).map(|id| (id, Arc::clone(permit))))
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?,
        policy,
        proposed_grace,
        commit_timeout,
    ));
    let host = LiveMatchHost::with_persistence(matches, persistence)?
        .with_cold_admission(lifecycle.clone(), commit_timeout)?;
    let upkeep_host = host.clone();
    let upkeep = async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            lifecycle.maintain(&upkeep_host).await;
        }
    };
    tokio::pin!(upkeep);
    let serial = Arc::new(
        stores
            .0
            .keys()
            .cloned()
            .map(|id| (id, Mutex::new(())))
            .collect(),
    );
    let controls = Characters {
        host: host.clone(),
        stores: Arc::clone(&stores),
        serial,
        runtime: tokio::runtime::Handle::current(),
        timeout: commit_timeout,
    };
    let (stop, requests) = mpsc::channel(1);
    let serving = game_server::serve_live_match_host_with_status_and_control_and_shutdown(
        host, controls, transport, status, requests,
    );
    tokio::pin!(serving);
    let mut health = tokio::time::interval(Duration::from_millis(100));
    health.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut stopping = false;
    let mut failure = None;
    loop {
        tokio::select! {
            ()=&mut upkeep=>{failure=Some("zone lifecycle worker stopped".into());stopping=true;let _=stop.try_send(());},
            result=&mut serving=>{if let Err(cause)=result{failure.get_or_insert_with(||cause.to_string());}break;},
            _=shutdown.recv(),if !stopping=>{stopping=true;let _=stop.try_send(());},
            result=backend.faults.changed(),if !stopping=>{failure=Some(if result.is_err(){"global backend worker stopped".into()}else{backend.faults.borrow().clone().unwrap_or_else(||"global backend failed".into())});stopping=true;let _=stop.try_send(());},
            _=health.tick(),if !stopping=>{
                if stores.0.values().any(|store|store.is_fenced())||permits.iter().any(|permit|permit.ensure_current(permit.lease().zone_id).is_err()){
                    failure=Some("zone persistence or ownership stopped; recovering the last committed boundary is required".into());stopping=true;let _=stop.try_send(());
                }
            }
        }
    }
    // Transport/tick writers have stopped. Final capture uses only the worker's
    // last committed image; uncertain private state is never saved over it.
    for store in stores.0.values() {
        if !store.is_fenced()
            && let Err(cause) = store.checkpoint().await
        {
            failure.get_or_insert_with(|| cause.to_string());
        }
    }
    for worker in workers {
        if let Err(cause) = worker.shutdown().await {
            failure.get_or_insert_with(|| cause.to_string());
        }
    }
    if let Err(cause) = backend.shutdown().await {
        failure.get_or_insert(cause);
    }
    if let Some(cause) = failure {
        return Err(cause.into());
    }
    Ok(())
}
