//! Empty-zone lifecycle outside simulation ticks; recovery always uses durable stores.
use super::hosted::HostedPersistence;
use crate::{
    ZoneGameServerAdapter,
    lifecycle::{EmptyGrace, ResumeInput, SuspensionPolicy, frozen_resume},
};
use game_server::{
    ColdMatchResolver, LiveHostError, LiveMatchHost, MatchId, MatchRuntime, PersistenceError,
    SnapshotCommitFuture,
};
use mmorpg_control_plane::LocalZonePermit;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex as HostMutex},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

fn error(cause: impl std::fmt::Display) -> PersistenceError {
    PersistenceError::new(cause.to_string())
}

enum Phase {
    Resident(EmptyGrace),
    AwaitingCheckpoint {
        frozen_at: Arc<HostMutex<Option<Instant>>>,
    },
    Suspended {
        frozen_at: Instant,
    },
}

pub(super) struct ZoneLifecycle {
    stores: BTreeMap<MatchId, Arc<HostedPersistence>>,
    permits: BTreeMap<MatchId, Arc<LocalZonePermit>>,
    phases: Mutex<BTreeMap<MatchId, Phase>>,
    clock: Instant,
    grace: Duration,
    timeout: Duration,
}
impl ZoneLifecycle {
    pub(super) fn new(
        stores: BTreeMap<MatchId, Arc<HostedPersistence>>,
        permits: BTreeMap<MatchId, Arc<LocalZonePermit>>,
        policy: SuspensionPolicy,
        proposed_grace: Option<Duration>,
        timeout: Duration,
    ) -> Self {
        let phases = stores
            .keys()
            .cloned()
            .map(|id| (id, Phase::Resident(EmptyGrace::default())))
            .collect();
        Self {
            stores,
            permits,
            phases: Mutex::new(phases),
            clock: Instant::now(),
            grace: policy.effective_grace(proposed_grace),
            timeout,
        }
    }

    pub(super) async fn maintain(&self, host: &LiveMatchHost<ZoneGameServerAdapter>) {
        if !host.is_serving() || host.is_draining().await {
            return;
        }
        let statuses = host.statuses().await;
        let mut phases = self.phases.lock().await;
        for status in statuses {
            let Some(phase) = phases.get_mut(&status.id) else {
                continue;
            };
            let frozen_at = match phase {
                Phase::Resident(empty) => {
                    if status.frozen
                        || !empty.due(
                            self.clock.elapsed(),
                            status.occupied_player_slots,
                            self.grace,
                        )
                    {
                        continue;
                    }
                    Arc::new(HostMutex::new(None))
                }
                Phase::AwaitingCheckpoint { frozen_at } => Arc::clone(frozen_at),
                Phase::Suspended { .. } => continue,
            };
            let Some(store) = self.stores.get(&status.id) else {
                continue;
            };
            match store.can_suspend().await {
                Ok(true) => {}
                Ok(false) => continue,
                Err(cause) => {
                    eprintln!("zone_suspension_waiting zone={} error={cause}", status.id);
                    continue;
                }
            }
            *phase = Phase::AwaitingCheckpoint {
                frozen_at: Arc::clone(&frozen_at),
            };
            let store = Arc::clone(store);
            let capture_clock = Arc::clone(&frozen_at);
            match host
                .suspend_empty(&status.id, self.timeout, move |image| {
                    capture_clock
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .get_or_insert_with(Instant::now);
                    async move { store.confirm_suspension(image).await }
                })
                .await
            {
                Ok(image) => {
                    let captured = *frozen_at
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    let Some(frozen_at) = captured else {
                        eprintln!(
                            "zone_suspension_failed zone={} error=missing_freeze_timestamp",
                            status.id
                        );
                        continue;
                    };
                    *phase = Phase::Suspended { frozen_at };
                    eprintln!(
                        "zone_suspended zone={} tick={} grace_ms={}",
                        status.id,
                        image.current_tick,
                        self.grace.as_millis()
                    );
                }
                Err(LiveHostError::MatchOccupied | LiveHostError::Draining) => {
                    *phase = Phase::Resident(EmptyGrace::default());
                }
                Err(cause) => {
                    eprintln!("zone_suspension_waiting zone={} error={cause}", status.id);
                }
            }
        }
    }
}
impl ColdMatchResolver<ZoneGameServerAdapter> for ZoneLifecycle {
    fn resolve<'a>(
        &'a self,
        host: &'a LiveMatchHost<ZoneGameServerAdapter>,
        id: &'a MatchId,
    ) -> SnapshotCommitFuture<'a> {
        Box::pin(async move {
            let store = self
                .stores
                .get(id)
                .ok_or_else(|| error("unknown configured zone"))?;
            let permit = self
                .permits
                .get(id)
                .ok_or_else(|| error("zone has no authority permit"))?;
            let mut phases = self.phases.lock().await;
            let phase = phases
                .get_mut(id)
                .ok_or_else(|| error("zone has no lifecycle owner"))?;
            let Phase::Suspended { frozen_at } = phase else {
                return Err(error("zone has no confirmed suspension boundary"));
            };
            let frozen_at = *frozen_at;
            let restored = async {
                permit
                    .ensure_current(permit.lease().zone_id)
                    .map_err(error)?;
                let input = ResumeInput {
                    elapsed_real_millis: u64::try_from(frozen_at.elapsed().as_millis())
                        .map_err(error)?,
                };
                let image = frozen_resume(store.cold_restore().await?, input);
                let mut adapter = ZoneGameServerAdapter::with_content(
                    permit.lease().zone_id,
                    mmorpg_core::content_for_zone(permit.lease().zone_id),
                )
                .map_err(error)?
                .with_authority(Arc::clone(permit))
                .map_err(error)?;
                adapter.require_durable_character_claims();
                let runtime = MatchRuntime::restore_from_recovery(adapter, image).map_err(error)?;
                permit
                    .ensure_current(permit.lease().zone_id)
                    .map_err(error)?;
                host.place(id.clone(), runtime).await.map_err(error)?;
                eprintln!(
                    "zone_resumed zone={id} elapsed_real_ms={} progression=freeze",
                    input.elapsed_real_millis
                );
                Ok(())
            }
            .await;
            *phase = if restored.is_ok() {
                Phase::Resident(EmptyGrace::default())
            } else {
                Phase::Suspended { frozen_at }
            };
            restored
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        persistence::{
            checkpoints::{CheckpointLimits, S3Config},
            hosted::{Durability, HostedPersistenceConfig, Startup},
            postgres::{JournalLimits, install_writer, migrate},
        },
        zone_match_id,
    };
    use game_server::{
        BrowserRoutePrefix, MatchHost, MatchHostWebTransportConfig, RejectMatchControlService,
        SnapshotPersistence, WELCOME_BYTES, decode_welcome,
    };
    use mmorpg_control_plane::{HostId, ZoneLease};
    use mmorpg_core::ZoneId;
    use tokio::sync::mpsc;
    use wtransport::{ClientConfig, Endpoint, Identity};

    #[tokio::test]
    #[ignore = "requires replicated PostgreSQL and S3; run explicitly with configured test services"]
    async fn durable_empty_zone_unloads_freezes_and_cold_admits_without_prewarming() {
        let primary = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL").unwrap();
        let standby = std::env::var("MMORPG_POSTGRES_STANDBY_URL").unwrap();
        let endpoint = std::env::var("MMORPG_CHECKPOINT_S3_ENDPOINT").unwrap();
        let bucket = std::env::var("MMORPG_CHECKPOINT_S3_BUCKET").unwrap();
        let setup = primary.clone();
        let fence = tokio::task::spawn_blocking(move || {
            let mut client = ::postgres::Client::connect(&setup, ::postgres::NoTls).unwrap();
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
                    host_id: HostId::new("suspension-test").unwrap(),
                    epoch: 1,
                    expires_at_tick: 1000,
                },
                0,
            )
            .unwrap()
        })
        .await
        .unwrap();
        let permit = Arc::new(
            LocalZonePermit::from_confirmed_lease(
                fence.lease.clone(),
                Instant::now(),
                Duration::from_secs(60),
            )
            .unwrap(),
        );
        let id = zone_match_id(fence.lease.zone_id).unwrap();
        let content = mmorpg_core::greyhaven_vale::content();
        let mut adapter =
            ZoneGameServerAdapter::with_content(fence.lease.zone_id, Arc::clone(&content)).unwrap();
        adapter.zone_mut().enable_reward_tracking(256, 64).unwrap();
        let mut genesis = MatchRuntime::new_with_replay_capture(adapter, 1);
        let image = genesis.checkpoint_replay().unwrap();
        let (storage, image, worker) = HostedPersistence::start(HostedPersistenceConfig {
            primary_url: primary,
            durability: Durability::Replicated {
                standby_url: standby,
            },
            database_timeout: Duration::from_secs(5),
            fence: fence.clone(),
            journal_limits: JournalLimits {
                max_batch_bytes: 1024 * 1024,
                max_retained_bytes: 4 * 1024 * 1024,
                max_retained_batches: 100,
                max_replay_ticks: 100,
            },
            s3: S3Config {
                endpoint,
                bucket,
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
            match_id: id.clone(),
            startup: Startup::Bootstrap(image),
            authority: Some(Arc::clone(&permit)),
        })
        .await
        .unwrap();
        let runtime = MatchRuntime::restore_from_recovery(
            ZoneGameServerAdapter::with_content(fence.lease.zone_id, Arc::clone(&content))
                .unwrap()
                .with_authority(Arc::clone(&permit))
                .unwrap(),
            image,
        )
        .unwrap();
        let mut matches = MatchHost::new(1).unwrap();
        matches.insert(id.clone(), runtime).unwrap();
        let lifecycle = Arc::new(ZoneLifecycle::new(
            BTreeMap::from([(id.clone(), Arc::clone(&storage))]),
            BTreeMap::from([(id.clone(), permit)]),
            SuspensionPolicy::new(Duration::ZERO, Duration::ZERO, Duration::ZERO).unwrap(),
            None,
            Duration::from_secs(10),
        ));
        let persistence =
            SnapshotPersistence::new(storage.clone(), Duration::from_secs(10), 1024 * 1024)
                .unwrap();
        let host = LiveMatchHost::with_persistence(matches, persistence)
            .unwrap()
            .with_cold_admission(lifecycle.clone(), Duration::from_secs(10))
            .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "mmorpg-suspension-{}-{}",
            std::process::id(),
            fence.lease.zone_id.get()
        ));
        std::fs::create_dir(&directory).unwrap();
        let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        let hash = identity.certificate_chain().as_slice()[0].hash();
        let certificate = directory.join("cert.pem");
        let key = directory.join("key.pem");
        identity
            .certificate_chain()
            .store_pemfile(&certificate)
            .await
            .unwrap();
        identity
            .private_key()
            .store_secret_pemfile(&key)
            .await
            .unwrap();
        let reservation = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let (shutdown, requests) = mpsc::channel(1);
        let server = tokio::spawn(
            game_server::serve_live_match_host_with_control_and_shutdown(
                host.clone(),
                RejectMatchControlService,
                MatchHostWebTransportConfig {
                    port,
                    certificate_pem: certificate,
                    private_key_pem: key,
                    route_prefix: BrowserRoutePrefix::new("/game").unwrap(),
                    allowed_origins: None,
                    drain_grace: Duration::ZERO,
                },
                requests,
            ),
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while !host.is_serving() || host.checkpoint(&id).await.is_err() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        lifecycle.maintain(&host).await;
        assert!(
            host.statuses().await.is_empty(),
            "the runtime must actually unload"
        );
        let frozen = storage.cold_restore().await.unwrap();
        let snapshot = MatchRuntime::restore_from_recovery(
            ZoneGameServerAdapter::with_content(fence.lease.zone_id, Arc::clone(&content)).unwrap(),
            frozen.clone(),
        )
        .unwrap()
        .snapshot()
        .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            storage.cold_restore().await.unwrap(),
            frozen,
            "unloaded simulation must remain frozen"
        );
        assert_eq!(
            MatchRuntime::restore_from_recovery(
                ZoneGameServerAdapter::with_content(fence.lease.zone_id, content).unwrap(),
                frozen
            )
            .unwrap()
            .snapshot()
            .unwrap(),
            snapshot
        );
        let client = Endpoint::client(
            ClientConfig::builder()
                .with_bind_default()
                .with_server_certificate_hashes([hash])
                .build(),
        )
        .unwrap();
        let connection = client
            .connect(format!("https://127.0.0.1:{port}/game/matches/{id}"))
            .await
            .unwrap();
        let mut bytes = [0; WELCOME_BYTES];
        connection
            .accept_uni()
            .await
            .unwrap()
            .read_exact(&mut bytes)
            .await
            .unwrap();
        assert_eq!(decode_welcome(&bytes).unwrap().player_id, 1);
        assert_eq!(host.statuses().await[0].occupied_player_slots, 1);
        assert!(matches!(
            lifecycle.phases.lock().await.get(&id),
            Some(Phase::Resident(_))
        ));
        shutdown.send(()).await.unwrap();
        server.await.unwrap().unwrap();
        worker.shutdown().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
