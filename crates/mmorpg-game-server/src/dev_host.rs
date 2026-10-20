//! Development character admission composed with the shared graceful-recovery host.
use crate::{DevCharacterService, DevCharacterStorage, ZoneGameServerAdapter, zone_match_id};
use game_server::{
    LiveMatchHost, MatchHostRecoveryConfig, MatchHostStatusConfig, MatchHostWebTransportConfig,
    MatchId, PreparedLiveMatchHost, prepare_live_match_host_for_recovery,
    serve_prepared_live_match_host_with_status_and_control_and_shutdown,
};
use mmorpg_core::{ZoneId, greyhaven_vale};
use std::{
    collections::BTreeSet, error::Error, fs::File, path::PathBuf, sync::Arc, time::Duration,
};
use tokio::sync::mpsc;

type HostResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct DevelopmentCharacterHostConfig {
    pub directory: PathBuf,
    pub recovery_directory: Option<PathBuf>,
    pub zone_ids: Vec<ZoneId>,
    pub reconnect_grace_ticks: u64,
}

async fn prepare(
    ids: Vec<MatchId>,
    ownership: Arc<File>,
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
            let zone = id
                .as_str()
                .strip_prefix("zone-")
                .ok_or("invalid recovered zone route")?
                .parse::<u32>()
                .map(ZoneId::new)
                .map_err(|error| error.to_string())?;
            let mut adapter = ZoneGameServerAdapter::with_content(zone, greyhaven_vale::content())
                .map_err(|error| error.to_string())?;
            adapter.require_character_claims(Arc::clone(&ownership));
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
    let mut storage =
        tokio::task::spawn_blocking(move || DevCharacterStorage::open(&directory)).await??;
    let ownership = storage.ownership();
    let ids: Vec<_> = config
        .zone_ids
        .into_iter()
        .map(zone_match_id)
        .collect::<Result<_, _>>()?;
    let prepared = prepare(
        ids.clone(),
        Arc::clone(&ownership),
        recovery.clone(),
        config.reconnect_grace_ticks,
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
    let service = DevCharacterService::new(host.clone(), storage);
    let serving = serve_prepared_live_match_host_with_status_and_control_and_shutdown(
        prepared,
        service.clone(),
        transport,
        status,
        shutdown,
    );
    tokio::pin!(serving);
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = &mut serving => { result?; break; }
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
    let stopped = prepare(ids, ownership, recovery, config.reconnect_grace_ticks).await?;
    let final_snapshots = snapshots(&stopped.host()).await?;
    tokio::task::spawn_blocking(move || service.finish_snapshots(final_snapshots)).await??;
    Ok(())
}
