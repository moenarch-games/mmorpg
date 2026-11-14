//! Development character admission composed with the shared graceful-recovery host.
use crate::handoff_host::HandoffHost;
use crate::instance::InstanceChannel;
use crate::party::{PartyChannel, PartyControl, PartyService, PartyZone};
use crate::{
    DevCharacterService, DevCharacterStorage, HandoffChannel, ZoneGameServerAdapter, zone_match_id,
};
use game_server::{
    LiveMatchHost, MatchHostRecoveryConfig, MatchHostStatusConfig, MatchHostWebTransportConfig,
    MatchId, PreparedLiveMatchHost, RecoveryImage, ReplayRecord,
    prepare_live_match_host_for_recovery,
    serve_prepared_live_match_host_with_status_and_control_and_shutdown,
};
use mmorpg_characters::EntryBackend;
use mmorpg_control_plane::party::PartyAuthority;
use mmorpg_core::ZoneId;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::File,
    path::{Path, PathBuf},
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
/// the service's builder and its transfer link listener, plus the party store its peer
/// hosts share and the zone leases that fence it.
pub(crate) struct HandoffWiring<B: EntryBackend> {
    pub channels: BTreeMap<MatchId, HandoffChannel>,
    /// Zones the instance service resolves (#404): public thresholds and depths slots.
    pub instance_channels: BTreeMap<MatchId, InstanceChannel>,
    pub parties: Box<dyn mmorpg_control_plane::party::PartyAuthority + Send>,
    pub leases: BTreeMap<MatchId, mmorpg_control_plane::ZoneLease>,
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
    party_channels: BTreeMap<MatchId, PartyChannel>,
    instance_channels: BTreeMap<MatchId, InstanceChannel>,
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
            if let Some(channel) = party_channels.get(id) {
                adapter.attach_parties(channel.clone());
            }
            if let Some(channel) = instance_channels.get(id) {
                adapter.attach_instances(channel.clone());
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
/// whole-host graceful recovery so final commits read the exact stopped checkpoint.
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

/// Managed native hosting retains the existing persistence and party composition.
pub async fn serve_managed_character_host(
    config: DevelopmentCharacterHostConfig,
    transport: MatchHostWebTransportConfig,
    status: MatchHostStatusConfig,
    shutdown: mpsc::Receiver<()>,
    managed: crate::hosted_access::ManagedWorld,
) -> HostResult<()> {
    let directory = config.directory;
    let recovery = config
        .recovery_directory
        .unwrap_or_else(|| directory.join("zone-recovery"));
    let storage =
        tokio::task::spawn_blocking(move || DevCharacterStorage::open(&directory)).await??;
    let ids = config
        .zone_ids
        .into_iter()
        .map(zone_match_id)
        .collect::<Result<Vec<_>, _>>()?;
    serve_characters_managed(
        storage,
        ids,
        recovery,
        config.reconnect_grace_ticks,
        None,
        transport,
        status,
        shutdown,
        Some(managed),
    )
    .await
}

/// Serves `ids` with character claims against `storage`, and, with
/// `handoffs`, live transfers to and from other hosts.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn serve_characters<B: EntryBackend + Send + 'static>(
    storage: DevCharacterStorage<B>,
    ids: Vec<MatchId>,
    recovery: PathBuf,
    grace: u64,
    handoffs: Option<HandoffWiring<B>>,
    transport: MatchHostWebTransportConfig,
    status: MatchHostStatusConfig,
    shutdown: mpsc::Receiver<()>,
) -> HostResult<()> {
    serve_characters_managed(
        storage, ids, recovery, grace, handoffs, transport, status, shutdown, None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn serve_characters_managed<B: EntryBackend + Send + 'static>(
    mut storage: DevCharacterStorage<B>,
    ids: Vec<MatchId>,
    recovery: PathBuf,
    grace: u64,
    mut handoffs: Option<HandoffWiring<B>>,
    transport: MatchHostWebTransportConfig,
    status: MatchHostStatusConfig,
    shutdown: mpsc::Receiver<()>,
    managed: Option<crate::hosted_access::ManagedWorld>,
) -> HostResult<()> {
    let ownership = storage.ownership();
    let channels = handoffs
        .as_ref()
        .map(|wiring| wiring.channels.clone())
        .unwrap_or_default();
    let instance_channels = handoffs
        .as_ref()
        .map(|wiring| wiring.instance_channels.clone())
        .unwrap_or_default();
    let party_channels: BTreeMap<MatchId, PartyChannel> = ids
        .iter()
        .map(|id| (id.clone(), PartyChannel::default()))
        .collect();
    let prepared = prepare(
        ids.clone(),
        ownership.clone(),
        channels,
        party_channels.clone(),
        instance_channels,
        recovery.clone(),
        grace,
    )
    .await?;
    let host = prepared.host();
    let (party_authority, party_leases): (Box<dyn PartyAuthority + Send>, BTreeMap<_, _>) =
        match &mut handoffs {
            Some(wiring) => (
                std::mem::replace(
                    &mut wiring.parties,
                    Box::new(mmorpg_control_plane::party::LocalParties::new()),
                ),
                wiring.leases.clone(),
            ),
            None => (
                Box::new(mmorpg_control_plane::party::LocalParties::new()),
                BTreeMap::new(),
            ),
        };
    let mut party_zones = BTreeMap::new();
    for (id, channel) in party_channels {
        let zone = crate::dev_characters::zone_of(&id)?;
        party_zones.insert(
            id.clone(),
            PartyZone {
                zone,
                lease: party_leases.get(&id).cloned(),
                channel,
            },
        );
    }
    let parties = PartyService::new(host.clone(), party_authority, party_zones);
    tokio::spawn(Arc::clone(&parties).run());
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
    let control = Arc::new(crate::hosted_access::ScopedControl::new(
        PartyControl {
            inner: service.clone(),
            parties,
        },
        managed.clone(),
    ));
    let sharing = async {
        match managed {
            Some(managed) => managed.run(host.clone(), control.clone()).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(sharing);
    let serving = serve_prepared_live_match_host_with_status_and_control_and_shutdown(
        prepared,
        (*control).clone(),
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
            _ = &mut sharing => return Err("Hosted sharing worker ended".into()),
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
    // The provider has frozen every writer and atomically saved the exact final image.
    // Its typed final checkpoint is already the canonical state we need. Re-preparing
    // here replays the entire session a second time and can keep Stop World locked
    // for minutes. Leave the bundle untouched: normal next-start verification/replay
    // still owns restoration, while final character commits use this same frozen state.
    let final_snapshots =
        tokio::task::spawn_blocking(move || stopped_snapshots(&recovery, &ids)).await??;
    tokio::task::spawn_blocking(move || service.finish_snapshots(final_snapshots)).await??;
    Ok(())
}

/// Read only our just-written, stopped recovery bundle through its owner's typed codec.
fn stopped_snapshots(directory: &Path, ids: &[MatchId]) -> HostResult<Vec<(MatchId, Vec<u8>)>> {
    ids.iter()
        .map(|id| {
            let image =
                RecoveryImage::read_file(&directory.join(format!("{}.recovery", id.as_str())))?;
            let Some(ReplayRecord::Checkpoint { snapshot }) = image.replay.records().last() else {
                return Err(
                    format!("Stopped recovery for {id} has no final canonical checkpoint").into(),
                );
            };
            if snapshot.tick != image.current_tick
                || snapshot.state_hash
                    != game_server::snapshot_hash(snapshot.tick, &snapshot.payload)
            {
                return Err(format!(
                    "Stopped recovery for {id} has an inconsistent final checkpoint"
                )
                .into());
            }
            let expected = crate::dev_characters::zone_of(id)?;
            let (zone, _) = ZoneGameServerAdapter::decode_stored_zone(
                &snapshot.payload,
                mmorpg_core::content_for_zone(expected),
            )?;
            if zone.zone_id() != expected || zone.current_tick() != image.current_tick {
                return Err(format!(
                    "Stopped recovery checkpoint does not match {id} and its final tick"
                )
                .into());
            }
            Ok((id.clone(), snapshot.payload.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_character_snapshot_is_the_exact_last_stopped_checkpoint() {
        let directory = tempfile::tempdir().unwrap();
        let id = zone_match_id(ZoneId::new(1)).unwrap();
        let mut runtime = game_server::MatchRuntime::new_with_replay_capture(
            ZoneGameServerAdapter::with_content(
                ZoneId::new(1),
                mmorpg_core::content_for_zone(ZoneId::new(1)),
            )
            .unwrap(),
            120,
        );
        for _ in 0..4 {
            runtime.advance_tick().unwrap();
        }
        runtime.freeze_for_recovery();
        let final_snapshot = runtime.snapshot().unwrap();
        runtime
            .recovery_image()
            .unwrap()
            .write_atomic(&directory.path().join("zone-1.recovery"))
            .unwrap();
        assert_eq!(
            stopped_snapshots(directory.path(), &[id.clone()]).unwrap(),
            vec![(id, final_snapshot.payload)]
        );
        // The codec/file association must not permit another match's final commit.
        std::fs::copy(
            directory.path().join("zone-1.recovery"),
            directory.path().join("zone-2.recovery"),
        )
        .unwrap();
        assert!(
            stopped_snapshots(directory.path(), &[zone_match_id(ZoneId::new(2)).unwrap()]).is_err()
        );
        // Reading final character state never consumes next startup's recovery bundle.
        assert!(RecoveryImage::read_file(&directory.path().join("zone-1.recovery")).is_ok());
    }
}
