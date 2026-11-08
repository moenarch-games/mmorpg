//! Private instance runs on a live host (#404 part 2, ADR 0009): the zone side of the run
//! directory (`mmorpg_control_plane::instance`).
//!
//! A public zone reports the players standing in its threshold strip; the service resolves each
//! against the directory and either queues a fenced departure toward the run's depths zone or
//! refuses with a notice. Depths zones report whether a character is in them and their run's
//! progress. Nothing here touches gameplay state directly: everything that changes a zone is a
//! trusted host input, so replay sees it in order with commands. Level-triggered reconciliation
//! means a restarted host simply rebinds its runs from the directory.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use game_server::{ControlContext, LiveMatchHost, MatchId, SimulationError};
use mmorpg_control_plane::ZoneLease;
use mmorpg_control_plane::instance::{EntryCandidate, InstanceAuthority};
use mmorpg_core::redbrand_depths::is_depths_zone;
use mmorpg_core::{
    BossReport, CharacterId, CharacterLocation, DepthsRunStart, ErrorCode, PlayerId,
    ThresholdRequest, ZoneId,
};
use mmorpg_protocol::{
    decode_boss_outcome, decode_depths_run_start, encode_boss_outcome, encode_depths_run_start,
    error_code_from_wire, error_code_to_wire,
};

use crate::ZoneGameServerAdapter;

/// How often the service reconciles.
const POLL: Duration = Duration::from_millis(100);

const BIND_RUN: u8 = 10;
const ENTER_DEPTHS: u8 = 11;
const REFUSE: u8 = 12;
const CONFIRM_BOSS: u8 = 13;

/// A threshold request with its characters: the player, then its gathered party.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdEntry {
    pub player: PlayerId,
    pub candidates: Vec<(CharacterId, bool)>,
}

/// A run's reported progress: run ID, cleared chambers, checkpoint.
pub type RunProgress = (u64, u8, bool);

#[derive(Default)]
struct Exchange {
    /// Requests since the service last took them, kept per player: a request made between
    /// service polls is not lost when a later tick no longer sees it.
    requests: BTreeMap<PlayerId, ThresholdEntry>,
    occupied: bool,
    progress: Option<RunProgress>,
    boss: Option<mmorpg_core::BossReport>,
}

/// The exchange between one live zone and its host's instance service.
#[derive(Clone, Default)]
pub struct InstanceChannel(Arc<Mutex<Exchange>>);

impl InstanceChannel {
    fn with<R>(&self, body: impl FnOnce(&mut Exchange) -> R) -> R {
        body(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The threshold requests the zone published at its last tick, for the service to resolve.
    #[must_use]
    pub fn take_requests(&self) -> Vec<ThresholdEntry> {
        self.with(|exchange| {
            std::mem::take(&mut exchange.requests)
                .into_values()
                .collect()
        })
    }

    /// Whether a character is in the zone, and its run's progress, as of its last tick.
    #[must_use]
    pub fn presence(&self) -> (bool, Option<RunProgress>) {
        self.with(|exchange| (exchange.occupied, exchange.progress))
    }

    /// The boss outcome the zone reports, if its boss fell.
    #[must_use]
    pub fn boss_report(&self) -> Option<BossReport> {
        self.with(|exchange| exchange.boss.clone())
    }
}

/// The host input binding a run to a depths zone (`[1, 10] · run start`).
pub fn bind_run_input(start: &DepthsRunStart) -> Result<Vec<u8>, SimulationError> {
    let mut payload = vec![1, BIND_RUN];
    payload.extend_from_slice(&encode_depths_run_start(start).map_err(crate::map_protocol_error)?);
    Ok(payload)
}

/// The host input sending the session's player on to `destination`
/// (`[1, 11] · zone u32 · position · facing`).
#[must_use]
pub fn enter_depths_input(destination: CharacterLocation) -> Vec<u8> {
    let mut payload = vec![1, ENTER_DEPTHS];
    payload.extend_from_slice(&destination.zone_id.get().to_be_bytes());
    for axis in destination.position {
        payload.extend_from_slice(&axis.to_be_bytes());
    }
    payload.extend_from_slice(&destination.facing.to_be_bytes());
    payload
}

/// The host input confirming the run directory recorded a run's boss outcome up to `version`
/// (`[1, 13] · run u64 · version u32`).
#[must_use]
pub fn confirm_boss_input(run_id: u64, version: u32) -> Vec<u8> {
    let mut payload = vec![1, CONFIRM_BOSS];
    payload.extend_from_slice(&run_id.to_be_bytes());
    payload.extend_from_slice(&version.to_be_bytes());
    payload
}

/// The host input refusing the session's player at the threshold (`[1, 12] · wire code`).
#[must_use]
pub fn refuse_input(code: ErrorCode) -> Vec<u8> {
    let mut payload = vec![1, REFUSE];
    payload.extend_from_slice(&error_code_to_wire(code).to_be_bytes());
    payload
}

fn parse_destination(body: &[u8]) -> Result<CharacterLocation, SimulationError> {
    let bad = || SimulationError::new("invalid depths destination input");
    let number = |range: std::ops::Range<usize>| body.get(range).ok_or_else(bad);
    if body.len() != 4 + 12 + 2 {
        return Err(bad());
    }
    let zone = u32::from_be_bytes(number(0..4)?.try_into().map_err(|_| bad())?);
    let axis = |offset: usize| -> Result<i32, SimulationError> {
        Ok(i32::from_be_bytes(
            number(offset..offset + 4)?.try_into().map_err(|_| bad())?,
        ))
    };
    Ok(CharacterLocation {
        zone_id: ZoneId::new(zone),
        position: [axis(4)?, axis(8)?, axis(12)?],
        facing: u16::from_be_bytes(number(16..18)?.try_into().map_err(|_| bad())?),
    })
}

impl ZoneGameServerAdapter {
    /// Attaches the host's instance service; a public zone starts reporting threshold requests.
    pub fn attach_instances(&mut self, channel: InstanceChannel) {
        self.zone.enable_depths_entry();
        self.instances = Some(channel);
    }

    /// Applies an instance host input; `false` when `payload` is not one.
    pub(crate) fn apply_instance_input(
        &mut self,
        player: PlayerId,
        payload: &[u8],
    ) -> Result<bool, SimulationError> {
        match payload {
            [1, BIND_RUN, body @ ..] => {
                let start = decode_depths_run_start(body).map_err(crate::map_protocol_error)?;
                self.zone
                    .begin_depths_run(start)
                    .map_err(crate::map_zone_error)?;
            }
            [1, CONFIRM_BOSS, body @ ..] => {
                let bad = || SimulationError::new("invalid boss confirmation input");
                let (run_id, version) = body.split_at_checked(8).ok_or_else(bad)?;
                self.zone.confirm_boss_outcome(
                    u64::from_be_bytes(run_id.try_into().map_err(|_| bad())?),
                    u32::from_be_bytes(version.try_into().map_err(|_| bad())?),
                );
            }
            [1, ENTER_DEPTHS, body @ ..] => {
                let destination = parse_destination(body)?;
                self.zone
                    .queue_departure(player, destination)
                    .map_err(crate::map_zone_error)?;
            }
            [1, REFUSE, hi, lo] => {
                let code = error_code_from_wire(u16::from_be_bytes([*hi, *lo]))
                    .map_err(crate::map_protocol_error)?;
                self.threshold_held.insert(player);
                self.zone
                    .queue_notice(player, code)
                    .map_err(crate::map_zone_error)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Tick step: publishes threshold requests and run presence.
    pub(crate) fn publish_instances(&mut self) {
        let Some(channel) = self.instances.clone() else {
            return;
        };
        let requests = self.zone.threshold_requests();
        let standing: BTreeSet<_> = requests.iter().map(|request| request.player).collect();
        self.threshold_held
            .retain(|player| standing.contains(player));
        let entries: Vec<_> = requests
            .into_iter()
            .filter(|request| !self.threshold_held.contains(&request.player))
            .filter_map(|request| self.resolve_entry(request))
            .collect();
        let occupied = self
            .characters
            .keys()
            .any(|player| !self.parked_characters.contains_key(player));
        let progress = self
            .zone
            .depths_run()
            .map(|run| (run.run_id, run.cleared, run.checkpoint));
        let boss = self.zone.boss_report();
        channel.with(|exchange| {
            for entry in entries {
                exchange.requests.insert(entry.player, entry);
            }
            exchange.occupied = occupied;
            exchange.progress = progress;
            exchange.boss = boss;
        });
    }

    fn resolve_entry(&self, request: ThresholdRequest) -> Option<ThresholdEntry> {
        let candidates = request
            .candidates
            .iter()
            .map(|(player, qualified)| Some((self.characters.get(player)?.clone(), *qualified)))
            .collect::<Option<Vec<_>>>()?;
        Some(ThresholdEntry {
            player: request.player,
            candidates,
        })
    }
}

/// One zone the service reconciles.
#[derive(Clone)]
pub struct InstanceZone {
    pub zone: ZoneId,
    /// Fences this zone's directory writes when the host has a shared directory.
    pub lease: Option<ZoneLease>,
    pub channel: InstanceChannel,
}

/// The host's instance service; see the module documentation.
pub struct InstanceService {
    host: LiveMatchHost<ZoneGameServerAdapter>,
    runtime: tokio::runtime::Handle,
    authority: Mutex<Box<dyn InstanceAuthority + Send>>,
    zones: BTreeMap<MatchId, InstanceZone>,
    /// The instance slots of `zones`, registered with the directory every pass.
    slots: Vec<ZoneId>,
    sessions: Mutex<BTreeMap<(MatchId, PlayerId), u32>>,
    grace_ms: u64,
    /// The newest boss outcome each zone confirmed, to send each confirmation once.
    confirmed: Mutex<BTreeMap<MatchId, (u64, u32)>>,
}

impl InstanceService {
    /// A service for `zones`; instance slots register with the directory on the first pass.
    #[must_use]
    pub fn new(
        host: LiveMatchHost<ZoneGameServerAdapter>,
        authority: Box<dyn InstanceAuthority + Send>,
        zones: BTreeMap<MatchId, InstanceZone>,
        grace: Duration,
    ) -> Arc<Self> {
        let slots = zones
            .values()
            .map(|zone| zone.zone)
            .filter(|zone| is_depths_zone(*zone))
            .collect();
        Arc::new(Self {
            host,
            runtime: tokio::runtime::Handle::current(),
            authority: Mutex::new(authority),
            zones,
            slots,
            sessions: Mutex::default(),
            grace_ms: u64::try_from(grace.as_millis()).unwrap_or(u64::MAX),
            confirmed: Mutex::default(),
        })
    }

    /// Remembers the session's connection epoch for its host inputs.
    pub fn track(&self, id: &MatchId, context: ControlContext) {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((id.clone(), context.player_id), context.connection_epoch);
    }

    fn epoch(&self, id: &MatchId, player: PlayerId) -> Option<u32> {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(id.clone(), player))
            .copied()
    }

    fn submit(
        &self,
        id: &MatchId,
        player: PlayerId,
        epoch: u32,
        input: &[u8],
    ) -> Result<(), String> {
        self.runtime
            .block_on(self.host.submit_host_input(id, player, epoch, input))
            .map_err(|error| error.to_string())
    }

    fn transact(
        &self,
        zone: &InstanceZone,
        body: &mut dyn FnMut(&mut mmorpg_control_plane::instance::InstanceDirectory, u64),
    ) -> Result<(), String> {
        self.authority
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .transact(zone.lease.as_ref(), body)
            .map_err(|error| error.to_string())
    }

    /// The directory's run for `character`, when it is on this slot; `Ok(None)` when not.
    fn bound_run(
        &self,
        zone: &InstanceZone,
        character: &CharacterId,
    ) -> Result<Option<DepthsRunStart>, String> {
        let mut run = None;
        self.transact(zone, &mut |directory, _| {
            run = directory.run_of(character).cloned();
        })?;
        run.filter(|run| run.slot == zone.zone)
            .map(|run| {
                let outcome = run
                    .outcome
                    .as_ref()
                    .map(|recorded| decode_boss_outcome(&recorded.bytes))
                    .transpose()
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>(DepthsRunStart {
                    run_id: run.id,
                    roster: run.roster,
                    cleared: run.cleared,
                    checkpoint: run.checkpoint,
                    outcome,
                })
            })
            .transpose()
    }

    /// Before an arrival activates: checks the character is on the run this slot holds and binds
    /// the run. Refuses a character the directory does not place on this zone.
    pub(crate) fn bind_for_arrival(
        &self,
        id: &MatchId,
        context: ControlContext,
        character: &CharacterId,
    ) -> Result<(), String> {
        let Some(zone) = self.zones.get(id).filter(|zone| is_depths_zone(zone.zone)) else {
            return Ok(());
        };
        let start = self
            .bound_run(zone, character)?
            .ok_or("this character holds no run in these depths")?;
        let input = bind_run_input(&start).map_err(|error| error.to_string())?;
        self.submit(id, context.player_id, context.connection_epoch, &input)
    }

    /// One reconciliation pass.
    pub fn reconcile(&self) {
        if let Some(zone) = self.zones.values().next() {
            let slots = self.slots.clone();
            if let Err(error) = self.transact(zone, &mut |directory, _| {
                directory.bind_slots(slots.iter().copied())
            }) {
                eprintln!("instance slot registration failed: {error}");
                return;
            }
        }
        for (id, zone) in &self.zones {
            let result = if is_depths_zone(zone.zone) {
                self.reconcile_slot(id, zone)
            } else {
                self.resolve_requests(id, zone)
            };
            if let Err(error) = result {
                eprintln!("instance reconcile of {} failed: {error}", id.as_str());
            }
        }
        if let Some(zone) = self.zones.values().next() {
            let grace = self.grace_ms;
            let swept = self.transact(zone, &mut |directory, now| {
                directory.sweep(now, grace);
            });
            if let Err(error) = swept {
                eprintln!("instance sweep failed: {error}");
            }
        }
    }

    fn reconcile_slot(&self, id: &MatchId, zone: &InstanceZone) -> Result<(), String> {
        let (occupied, progress) = zone.channel.presence();
        let report = zone.channel.boss_report();
        let encoded = report
            .as_ref()
            .map(|report| encode_boss_outcome(&report.outcome))
            .transpose()
            .map_err(|error| error.to_string())?;
        let slot = zone.zone;
        self.transact(zone, &mut |directory, now| {
            directory.sync_slot(slot, occupied, now);
            if let Some((run, cleared, checkpoint)) = progress {
                directory.report(slot, run, cleared, checkpoint);
            }
            if let (Some(report), Some(bytes)) = (&report, &encoded) {
                directory.report_outcome(
                    slot,
                    report.run_id,
                    report.outcome.version,
                    bytes.clone(),
                    report.outstanding.clone(),
                );
            }
        })?;
        // Only after the directory committed: the zone delivers rewards up to this version.
        let Some(report) = report else {
            return Ok(());
        };
        let current = (report.run_id, report.outcome.version);
        let mut confirmed = self
            .confirmed
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if confirmed.get(id) == Some(&current) {
            return Ok(());
        }
        let Some((player, epoch)) = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|((session, _), _)| session == id)
            .map(|((_, player), epoch)| (*player, *epoch))
        else {
            return Ok(());
        };
        self.submit(id, player, epoch, &confirm_boss_input(current.0, current.1))?;
        confirmed.insert(id.clone(), current);
        Ok(())
    }

    fn resolve_requests(&self, id: &MatchId, zone: &InstanceZone) -> Result<(), String> {
        for request in zone.channel.take_requests() {
            let Some(epoch) = self.epoch(id, request.player) else {
                continue; // The session has not spoken over control yet.
            };
            let candidates: Vec<_> = request
                .candidates
                .iter()
                .map(|(character, qualified)| EntryCandidate {
                    character: character.clone(),
                    qualified: *qualified,
                })
                .collect();
            let mut outcome = None;
            self.transact(zone, &mut |directory, now| {
                outcome = Some(directory.enter(&candidates, now));
            })?;
            let input = match outcome.ok_or("the directory gave no answer")? {
                Ok(entry) => enter_depths_input(mmorpg_core::depths_arrival(entry.run.slot)),
                Err(code) => refuse_input(code),
            };
            self.submit(id, request.player, epoch, &input)?;
        }
        Ok(())
    }

    /// Reconciles until the process ends.
    pub async fn run(self: Arc<Self>) {
        let mut interval = tokio::time::interval(POLL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if self.host.is_serving() {
                let service = Arc::clone(&self);
                if let Err(error) = tokio::task::spawn_blocking(move || service.reconcile()).await {
                    eprintln!("instance reconcile stopped: {error}");
                }
            }
        }
    }
}
