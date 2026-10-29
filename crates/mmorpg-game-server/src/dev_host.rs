//! Development character admission composed with the shared graceful-recovery host.
use crate::handoff_host::HandoffHost;
use crate::{
    DevCharacterService, DevCharacterStorage, HandoffChannel, ZoneGameServerAdapter, zone_match_id,
};
use game_server::{
    LiveMatchHost, MatchHostRecoveryConfig, MatchHostStatusConfig, MatchHostWebTransportConfig,
    MatchId, PreparedLiveMatchHost, prepare_live_match_host_for_recovery,
    serve_prepared_live_match_host_with_status_and_control_and_shutdown,
};
use mmorpg_characters::EntryBackend;
use mmorpg_core::ZoneId;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::File,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

type HostResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct DevelopmentCharacterHostConfig {
    pub directory: PathBuf,
    pub recovery_directory: Option<PathBuf>,
    pub zone_ids: Vec<ZoneId>,
    pub reconnect_grace_ticks: u64,
}

/// Live handoff between hosts (#145): each zone's channel to the service,
/// the service's builder and its transfer link listener.
pub(crate) struct HandoffWiring<B: EntryBackend> {
    pub channels: BTreeMap<MatchId, HandoffChannel>,
    pub listener: tokio::net::TcpListener,
    #[allow(clippy::type_complexity)]
    pub build: Box<
        dyn FnOnce(
                LiveMatchHost<ZoneGameServerAdapter>,
                Arc<Mutex<DevCharacterStorage<B>>>,
            ) -> Arc<HandoffHost<B>>
            + Send,
    >,
}

async fn prepare(
    ids: Vec<MatchId>,
    ownership: Option<Arc<File>>,
    channels: BTreeMap<MatchId, HandoffChannel>,
    directory: PathBuf,
    grace: u64,
) -> HostResult<PreparedLiveMatchHost<ZoneGameServerAdapter>> {
    let expected: BTreeSet<_> = ids.iter().cloned().collect();
    if ids.is_empty() || expected.len() != ids.len() {
        return Err("development host needs a nonempty, unique zone set".into());
    }
    let prepared = prepare_live_match_host_for_recovery(
        ids,
        move |id: &MatchId| -> Result<_, String> {
            let zone = crate::dev_characters::zone_of(id)?;
            let mut adapter =
                ZoneGameServerAdapter::with_content(zone, mmorpg_core::content_for_zone(zone))
                    .map_err(|error| error.to_string())?;
            match &ownership {
                Some(ownership) => adapter.require_character_claims(Arc::clone(ownership)),
                None => adapter.require_durable_character_claims(),
            }
            if let Some(channel) = channels.get(id) {
                adapter.attach_handoffs(channel.clone());
            }
            Ok(adapter)
        },
        expected.len(),
        grace,
        MatchHostRecoveryConfig { directory },
    )
    .await?;
    let actual: BTreeSet<_> = prepared
        .host()
        .statuses()
        .await
        .into_iter()
        .map(|status| status.id)
        .collect();
    if actual != expected {
        return Err("recovery zone set differs from configured zones".into());
    }
    Ok(prepared)
}

async fn snapshots(
    host: &LiveMatchHost<ZoneGameServerAdapter>,
) -> HostResult<Vec<(MatchId, Vec<u8>)>> {
    let mut snapshots = Vec::new();
    for status in host.statuses().await {
        let snapshot = host
            .inspect(&status.id, |runtime| runtime.snapshot())
            .await??;
        snapshots.push((status.id, snapshot.payload));
    }
    Ok(snapshots)
}

/// Explicit development/LAN trust mode. Persistent characters use the existing
/// whole-host graceful recovery so final commits read stopped, replayed state.
pub async fn serve_development_character_host(
    config: DevelopmentCharacterHostConfig,
    transport: MatchHostWebTransportConfig,
    status: MatchHostStatusConfig,
    shutdown: mpsc::Receiver<()>,
) -> HostResult<()> {
    let directory = config.directory;
    let recovery = config
        .recovery_directory
        .unwrap_or_else(|| directory.join("zone-recovery"));
    let storage =
        tokio::task::spawn_blocking(move || DevCharacterStorage::open(&directory)).await??;
    let ids: Vec<_> = config
        .zone_ids
        .into_iter()
        .map(zone_match_id)
        .collect::<Result<_, _>>()?;
    serve_characters(
        storage,
        ids,
        recovery,
        config.reconnect_grace_ticks,
        None,
        transport,
        status,
        shutdown,
    )
    .await
}

/// Serves `ids` with character claims against `storage`, and, with
/// `handoffs`, live transfers to and from other hosts.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn serve_characters<B: EntryBackend + Send + 'static>(
    mut storage: DevCharacterStorage<B>,
    ids: Vec<MatchId>,
    recovery: PathBuf,
    grace: u64,
    handoffs: Option<HandoffWiring<B>>,
    transport: MatchHostWebTransportConfig,
    status: MatchHostStatusConfig,
    shutdown: mpsc::Receiver<()>,
) -> HostResult<()> {
    let ownership = storage.ownership();
    let channels = handoffs
        .as_ref()
        .map(|wiring| wiring.channels.clone())
        .unwrap_or_default();
    let prepared = prepare(
        ids.clone(),
        ownership.clone(),
        channels,
        recovery.clone(),
        grace,
    )
    .await?;
    let host = prepared.host();
    let restored = snapshots(&host).await?;
    storage = tokio::task::spawn_blocking(move || {
        for (id, payload) in restored {
            storage.restore_snapshot(&id, &payload)?;
        }
        Ok::<_, String>(storage)
    })
    .await??;
    let storage = Arc::new(Mutex::new(storage));
    let (handoff_host, listener) = match handoffs {
        Some(wiring) => (
            Some((wiring.build)(host.clone(), Arc::clone(&storage))),
            Some(wiring.listener),
        ),
        None => (None, None),
    };
    let service =
        DevCharacterService::with_shared_storage(host.clone(), storage, handoff_host.clone());
    let serving = serve_prepared_live_match_host_with_status_and_control_and_shutdown(
        prepared,
        service.clone(),
        transport,
        status,
        shutdown,
    );
    tokio::pin!(serving);
    let links = async move {
        match (handoff_host, listener) {
            (Some(handoffs), Some(listener)) => handoffs.run(listener).await,
            _ => std::future::pending().await,
        }
    };
    tokio::pin!(links);
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = &mut serving => { result?; break; }
            result = &mut links => {
                return Err(format!("transfer link stopped: {}", result.err().unwrap_or_default()).into());
            }
            _ = interval.tick() => {
                if !host.is_serving() { continue; }
                let service = service.clone();
                // A failed commit exits serving and stops the authoritative host.
                tokio::task::spawn_blocking(move || service.save_all()).await??;
            }
        }
    }
    // Serving has stopped all writers and saved the exact final recovery image.
    // Prepare without serving: replay is shared game-server behavior, and the
    // bundle remains unconsumed for the next actual startup.
    let stopped = prepare(ids, ownership, BTreeMap::new(), recovery, grace).await?;
    let final_snapshots = snapshots(&stopped.host()).await?;
    tokio::task::spawn_blocking(move || service.finish_snapshots(final_snapshots)).await??;
    Ok(())
}
