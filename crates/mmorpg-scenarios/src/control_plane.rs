//! Scripted control-plane sequences with distributed-world invariant checks.
//!
//! Operations go to the in-memory `mmorpg-control-plane` reference model and,
//! for `write`, through `mmorpg-game-server::FencedZoneRuntime`. After every
//! step the runner observes directory and handoff state and checks the
//! AGENTS.md invariants against its own simple formulation of them. The model
//! remains the authority; the runner only observes and compares.

use std::collections::{BTreeMap, BTreeSet};

use game_server::MatchRuntime;
use mmorpg_control_plane::{
    ControlPlaneError, EntityId, HandoffPhase, HandoffRecord, HandoffRegistry, HandoffTicket,
    HostId, HostRegistry, TransferId, ZoneDirectory, ZoneLease,
};
use mmorpg_core::ZoneId;
use mmorpg_game_server::{FencedRuntimeError, FencedZoneRuntime};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::report::{Report, snake_kind, validate_name};

pub const SCHEMA: &str = "mmorpg.control-plane-scenario/v1";
const MAX_STEPS: usize = 10_000;
const RECONNECT_GRACE_TICKS: u64 = mmorpg_core::long_ticks_from_ms(4_000) as u64;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlPlaneScenario {
    pub name: String,
    pub lease_ttl_ticks: u64,
    pub heartbeat_ttl_ticks: u64,
    pub steps: Vec<Step>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Register,
    Heartbeat,
    ExpireHosts,
    Assign,
    Reassign,
    Renew,
    Release,
    ExpireLeases,
    Write,
    Prepare,
    Accept,
    Commit,
}

impl Op {
    fn name(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Heartbeat => "heartbeat",
            Self::ExpireHosts => "expire_hosts",
            Self::Assign => "assign",
            Self::Reassign => "reassign",
            Self::Renew => "renew",
            Self::Release => "release",
            Self::ExpireLeases => "expire_leases",
            Self::Write => "write",
            Self::Prepare => "prepare",
            Self::Accept => "accept",
            Self::Commit => "commit",
        }
    }

    /// Operations that change zone ownership or handoff state and must
    /// therefore present the current epoch.
    fn presents_epoch(self) -> bool {
        matches!(
            self,
            Self::Reassign
                | Self::Renew
                | Self::Release
                | Self::Write
                | Self::Prepare
                | Self::Accept
                | Self::Commit
        )
    }
}

/// One operation at control-plane time `at`. Leases are named by
/// `(zone, host, epoch)`; handoff operations name the transfer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub at: u64,
    pub op: Op,
    pub host: Option<String>,
    pub zone: Option<u32>,
    pub epoch: Option<u64>,
    pub expected_epoch: Option<u64>,
    pub transfer: Option<u64>,
    pub entity: Option<u64>,
    pub source: Option<LeaseRef>,
    pub destination: Option<LeaseRef>,
    /// `ok` (default) or `rejected:<kind>`.
    pub expect: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseRef {
    pub zone: u32,
    pub host: String,
    pub epoch: u64,
}

/// Parses and validates a scenario at the file trust boundary.
pub fn load(text: &str) -> Result<ControlPlaneScenario, String> {
    let scenario: ControlPlaneScenario = toml::from_str(text).map_err(|error| error.to_string())?;
    validate(&scenario)?;
    Ok(scenario)
}

fn validate(scenario: &ControlPlaneScenario) -> Result<(), String> {
    validate_name("scenario name", &scenario.name)?;
    if scenario.steps.is_empty() || scenario.steps.len() > MAX_STEPS {
        return Err(format!("steps must contain 1..={MAX_STEPS} entries"));
    }
    let mut previous = 0;
    for (index, step) in scenario.steps.iter().enumerate() {
        let at = format!("steps[{index}]");
        if step.at < previous {
            return Err(format!("{at}: control-plane time must not go backwards"));
        }
        previous = step.at;
        let has = |present: bool, field: &str| {
            present
                .then_some(())
                .ok_or_else(|| format!("{at}: {} requires {field}", step.op.name()))
        };
        let lease = |step: &Step| {
            has(step.zone.is_some(), "zone")?;
            has(step.host.is_some(), "host")?;
            has(step.epoch.is_some(), "epoch")
        };
        match step.op {
            Op::Register | Op::Heartbeat => has(step.host.is_some(), "host")?,
            Op::ExpireHosts | Op::ExpireLeases => {}
            Op::Assign => {
                has(step.zone.is_some(), "zone")?;
                has(step.host.is_some(), "host")?;
            }
            Op::Reassign => {
                has(step.zone.is_some(), "zone")?;
                has(step.host.is_some(), "host")?;
                has(step.expected_epoch.is_some(), "expected_epoch")?;
            }
            Op::Renew | Op::Release | Op::Write => lease(step)?,
            Op::Prepare => {
                has(step.transfer.is_some(), "transfer")?;
                has(step.entity.is_some(), "entity")?;
                has(step.source.is_some(), "source")?;
                has(step.destination.is_some(), "destination")?;
            }
            Op::Accept | Op::Commit => {
                has(step.transfer.is_some(), "transfer")?;
                lease(step)?;
            }
        }
    }
    Ok(())
}

/// Directory and handoff state visible through public read APIs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Observation {
    pub leases: BTreeMap<ZoneId, ZoneLease>,
    pub epoch_floor: BTreeMap<ZoneId, u64>,
    pub records: BTreeMap<TransferId, HandoffRecord>,
}

/// What the runner remembers across steps to check invariants.
#[derive(Debug, Default)]
pub struct History {
    /// Every lease the directory ever granted, in grant order.
    pub grants: Vec<ZoneLease>,
    /// Every phase each transfer has been observed in, in order.
    pub phases: BTreeMap<TransferId, Vec<HandoffPhase>>,
}

/// The step being checked, reduced to what invariants need.
#[derive(Debug)]
pub struct Attempt<'a> {
    pub op: Op,
    pub now: u64,
    /// Leases the operation presented as its authority.
    pub presented: Vec<&'a ZoneLease>,
    /// Reassignment's expected epoch, which fences the current owner.
    pub expected_epoch: Option<(ZoneId, u64)>,
    pub transfer: Option<TransferId>,
    pub succeeded: bool,
}

/// Invariant names, in check order.
pub const INVARIANTS: [&str; 4] = [
    "single_writer",
    "epoch_fenced",
    "handoff_idempotent",
    "retire_after_accept",
];

/// Checks the distributed-world invariants for one step. Returns
/// `(invariant, detail)` for each violation. `history` must already include
/// grants issued by this step but not yet this step's phases.
pub fn check_invariants(
    history: &History,
    before: &Observation,
    after: &Observation,
    attempt: &Attempt<'_>,
) -> Vec<(&'static str, String)> {
    let mut violations = Vec::new();

    // single_writer: at most one granted lease per zone is currently valid,
    // and no two grants ever share a zone epoch.
    let mut active: BTreeMap<ZoneId, Vec<&ZoneLease>> = BTreeMap::new();
    let mut epochs = BTreeSet::new();
    for grant in &history.grants {
        if !epochs.insert((grant.zone_id, grant.epoch)) {
            violations.push((
                "single_writer",
                format!(
                    "zone {} epoch {} granted twice",
                    grant.zone_id.get(),
                    grant.epoch
                ),
            ));
        }
        let current = after.leases.get(&grant.zone_id).is_some_and(|lease| {
            lease.same_authority(grant) && attempt.now < lease.expires_at_tick
        });
        if current {
            active.entry(grant.zone_id).or_default().push(grant);
        }
    }
    for (zone, writers) in &active {
        if writers.len() > 1 {
            violations.push((
                "single_writer",
                format!("zone {} has {} active writers", zone.get(), writers.len()),
            ));
        }
    }
    for (zone, lease) in &after.leases {
        if !history
            .grants
            .iter()
            .any(|grant| grant.same_authority(lease))
        {
            violations.push((
                "single_writer",
                format!(
                    "zone {} owner {} was never granted",
                    zone.get(),
                    lease.epoch
                ),
            ));
        }
    }

    // epoch_fenced: a successful fenced operation presented the current,
    // unexpired epoch; new epochs only move forward; a rejected operation
    // leaves ownership and handoff state untouched.
    if attempt.succeeded && attempt.op.presents_epoch() {
        for lease in &attempt.presented {
            let current = before.leases.get(&lease.zone_id).is_some_and(|current| {
                current.same_authority(lease) && attempt.now < current.expires_at_tick
            });
            if !current {
                violations.push((
                    "epoch_fenced",
                    format!(
                        "{} succeeded with non-current zone {} epoch {}",
                        attempt.op.name(),
                        lease.zone_id.get(),
                        lease.epoch
                    ),
                ));
            }
        }
        if let Some((zone, expected)) = attempt.expected_epoch {
            let current = before.leases.get(&zone).map(|lease| lease.epoch);
            if current != Some(expected) {
                violations.push((
                    "epoch_fenced",
                    format!(
                        "reassign expected epoch {expected} but zone {} was at {current:?}",
                        zone.get()
                    ),
                ));
            }
        }
    }
    for (zone, epoch) in &after.epoch_floor {
        let previous = before.epoch_floor.get(zone).copied().unwrap_or(0);
        if *epoch < previous {
            violations.push((
                "epoch_fenced",
                format!(
                    "zone {} epoch floor regressed {previous} -> {epoch}",
                    zone.get()
                ),
            ));
        }
    }
    if !attempt.succeeded && before != after {
        violations.push((
            "epoch_fenced",
            format!("rejected {} changed control-plane state", attempt.op.name()),
        ));
    }

    // handoff_idempotent: an existing transfer keeps its ticket, never
    // regresses, and repeating a completed phase changes nothing; each entity
    // has at most one unfinished transfer.
    if let Some(transfer) = attempt.transfer
        && let (Some(old), Some(new)) =
            (before.records.get(&transfer), after.records.get(&transfer))
    {
        if old.ticket != new.ticket {
            violations.push((
                "handoff_idempotent",
                format!("transfer {} ticket changed", transfer.get()),
            ));
        }
        if phase_rank(new.phase) < phase_rank(old.phase) {
            violations.push((
                "handoff_idempotent",
                format!("transfer {} phase regressed", transfer.get()),
            ));
        }
        let target = match attempt.op {
            Op::Prepare => Some(HandoffPhase::Prepared),
            Op::Accept => Some(HandoffPhase::Accepted),
            Op::Commit => Some(HandoffPhase::Committed),
            _ => None,
        };
        if target.is_some_and(|target| phase_rank(target) <= phase_rank(old.phase)) && old != new {
            violations.push((
                "handoff_idempotent",
                format!(
                    "repeated {} changed transfer {}",
                    attempt.op.name(),
                    transfer.get()
                ),
            ));
        }
    }
    let mut unfinished: BTreeMap<EntityId, usize> = BTreeMap::new();
    for record in after.records.values() {
        if record.phase != HandoffPhase::Committed {
            *unfinished.entry(record.ticket.entity_id).or_default() += 1;
        }
    }
    for (entity, count) in unfinished {
        if count > 1 {
            violations.push((
                "handoff_idempotent",
                format!("entity {} has {count} unfinished transfers", entity.get()),
            ));
        }
    }

    // retire_after_accept: the source is retired (committed) only after the
    // destination was observed to accept.
    for (transfer, record) in &after.records {
        if record.phase == HandoffPhase::Committed {
            let accepted = history
                .phases
                .get(transfer)
                .is_some_and(|phases| phases.contains(&HandoffPhase::Accepted));
            if !accepted {
                violations.push((
                    "retire_after_accept",
                    format!(
                        "transfer {} committed without prior acceptance",
                        transfer.get()
                    ),
                ));
            }
        }
    }
    violations
}

fn phase_rank(phase: HandoffPhase) -> u8 {
    match phase {
        HandoffPhase::Prepared => 0,
        HandoffPhase::Accepted => 1,
        HandoffPhase::Committed => 2,
    }
}

fn phase_name(phase: HandoffPhase) -> &'static str {
    match phase {
        HandoffPhase::Prepared => "prepared",
        HandoffPhase::Accepted => "accepted",
        HandoffPhase::Committed => "committed",
    }
}

type LeaseKey = (ZoneId, HostId, u64);

struct Runner<'a> {
    scenario: &'a ControlPlaneScenario,
    hosts: HostRegistry,
    directory: ZoneDirectory,
    handoffs: HandoffRegistry,
    writers: BTreeMap<LeaseKey, FencedZoneRuntime>,
    history: History,
    zones: BTreeSet<ZoneId>,
    transfers: BTreeSet<TransferId>,
    report: Report,
    rejected: usize,
}

/// Runs a scenario. `Err` means the reference model could not be built;
/// scenario failures are recorded in the returned report instead.
pub fn run(scenario: &ControlPlaneScenario) -> Result<Report, String> {
    let hosts =
        HostRegistry::new(scenario.heartbeat_ttl_ticks).map_err(|error| error.to_string())?;
    let directory =
        ZoneDirectory::new(scenario.lease_ttl_ticks).map_err(|error| error.to_string())?;
    let mut zones = BTreeSet::new();
    let mut transfers = BTreeSet::new();
    for step in &scenario.steps {
        zones.extend(step.zone.map(ZoneId::new));
        for lease in [&step.source, &step.destination].into_iter().flatten() {
            zones.insert(ZoneId::new(lease.zone));
        }
        transfers.extend(step.transfer.map(TransferId::new));
    }
    let mut runner = Runner {
        scenario,
        hosts,
        directory,
        handoffs: HandoffRegistry::default(),
        writers: BTreeMap::new(),
        history: History::default(),
        zones,
        transfers,
        report: Report::new(SCHEMA),
        rejected: 0,
    };
    runner.run();
    Ok(runner.report)
}

/// A failed operation: its rejection kind plus the model's message.
struct Rejection {
    kind: String,
    message: String,
}

impl From<ControlPlaneError> for Rejection {
    fn from(error: ControlPlaneError) -> Self {
        Self {
            kind: snake_kind(&format!("{error:?}")),
            message: error.to_string(),
        }
    }
}

impl From<FencedRuntimeError> for Rejection {
    fn from(error: FencedRuntimeError) -> Self {
        match error {
            FencedRuntimeError::Authority(error) => error.into(),
            other => Self {
                kind: snake_kind(&format!("{other:?}")),
                message: other.to_string(),
            },
        }
    }
}

impl Runner<'_> {
    fn observe(&self) -> Observation {
        Observation {
            leases: self
                .zones
                .iter()
                .filter_map(|zone| {
                    self.directory
                        .lease(*zone)
                        .map(|lease| (*zone, lease.clone()))
                })
                .collect(),
            epoch_floor: self.directory.epoch_floor_snapshot(),
            records: self
                .transfers
                .iter()
                .filter_map(|id| {
                    self.handoffs
                        .record(*id)
                        .map(|record| (*id, record.clone()))
                })
                .collect(),
        }
    }

    /// Builds the lease a caller presents. Only identity matters for fencing;
    /// the deadline is taken from the directory when the identity matches.
    fn lease(&self, zone: u32, host: &str, epoch: u64) -> Result<ZoneLease, Rejection> {
        let zone_id = ZoneId::new(zone);
        let host_id = HostId::new(host)?;
        let expires_at_tick = self
            .directory
            .lease(zone_id)
            .filter(|lease| lease.host_id == host_id && lease.epoch == epoch)
            .map_or(0, |lease| lease.expires_at_tick);
        Ok(ZoneLease {
            zone_id,
            host_id,
            epoch,
            expires_at_tick,
        })
    }

    fn step_lease(&self, step: &Step) -> Result<ZoneLease, Rejection> {
        self.lease(
            step.zone.unwrap_or_default(),
            step.host.as_deref().unwrap_or_default(),
            step.epoch.unwrap_or_default(),
        )
    }

    fn run(&mut self) {
        let scenario = self.scenario;
        self.report.line(
            "scenario",
            format!(
                "scenario {} lease_ttl={} heartbeat_ttl={} steps={}",
                scenario.name,
                scenario.lease_ttl_ticks,
                scenario.heartbeat_ttl_ticks,
                scenario.steps.len()
            ),
            json!({
                "scenario": scenario.name,
                "lease_ttl_ticks": scenario.lease_ttl_ticks,
                "heartbeat_ttl_ticks": scenario.heartbeat_ttl_ticks,
                "steps": scenario.steps.len(),
            }),
        );
        for (index, step) in scenario.steps.iter().enumerate() {
            self.step(index, step);
        }
        let failures = self.report.failures();
        let verdict = if failures == 0 { "PASS" } else { "FAIL" };
        let checks = scenario.steps.len() * INVARIANTS.len();
        self.report.line(
            "result",
            format!(
                "{verdict} {} steps={} rejected={} invariant_checks={checks} failures={failures}",
                scenario.name,
                scenario.steps.len(),
                self.rejected
            ),
            json!({
                "scenario": scenario.name,
                "passed": failures == 0,
                "steps": scenario.steps.len(),
                "rejected": self.rejected,
                "invariant_checks": checks,
                "failures": failures,
            }),
        );
    }

    fn step(&mut self, index: usize, step: &Step) {
        let before = self.observe();
        let presented = self.presented(step);
        let result = self.apply(step);
        let after = self.observe();

        let transfer = step.transfer.map(TransferId::new);
        let attempt = Attempt {
            op: step.op,
            now: step.at,
            presented: presented.iter().collect(),
            expected_epoch: step
                .expected_epoch
                .zip(step.zone)
                .map(|(epoch, zone)| (ZoneId::new(zone), epoch)),
            transfer,
            succeeded: result.is_ok(),
        };
        let violations = check_invariants(&self.history, &before, &after, &attempt);
        if let Some(record) = transfer.and_then(|id| after.records.get(&id)) {
            let phases = self
                .history
                .phases
                .entry(record.ticket.transfer_id)
                .or_default();
            if phases.last() != Some(&record.phase) {
                phases.push(record.phase);
            }
        }

        let (outcome, detail) = match &result {
            Ok(detail) => ("ok".to_string(), detail.clone()),
            Err(rejection) => {
                self.rejected += 1;
                (
                    format!("rejected:{}", rejection.kind),
                    rejection.message.clone(),
                )
            }
        };
        let expected = step.expect.as_deref().unwrap_or("ok");
        let ok = outcome == expected;
        if !ok {
            self.report.fail();
        }
        for _ in &violations {
            self.report.fail();
        }

        let arguments = describe(step);
        let owners = self.owners(&after);
        let mut text = format!("#{index} t={} {}", step.at, step.op.name());
        if !arguments.is_empty() {
            text.push(' ');
            text.push_str(&arguments);
        }
        text.push_str(&format!(" -> {outcome}"));
        if result.is_ok() && !detail.is_empty() {
            text.push(' ');
            text.push_str(&detail);
        }
        if !ok {
            text.push_str(&format!(" FAIL expected {expected} ({detail})"));
        }
        text.push_str(&format!(" | {owners} | "));
        if violations.is_empty() {
            text.push_str("invariants ok");
        } else {
            let names = violations
                .iter()
                .map(|(name, detail)| format!("{name}: {detail}"))
                .collect::<Vec<_>>()
                .join("; ");
            text.push_str(&format!("INVARIANT FAIL {names}"));
        }
        self.report.line(
            "step",
            text,
            json!({
                "index": index,
                "at": step.at,
                "op": step.op.name(),
                "arguments": arguments,
                "outcome": outcome,
                "detail": detail,
                "expected": expected,
                "ok": ok,
                "owners": owners,
                "invariants": INVARIANTS,
                "violations": violations
                    .iter()
                    .map(|(name, detail)| json!({ "invariant": name, "detail": detail }))
                    .collect::<Vec<Value>>(),
            }),
        );
    }

    fn presented(&self, step: &Step) -> Vec<ZoneLease> {
        let mut leases = Vec::new();
        if matches!(
            step.op,
            Op::Renew | Op::Release | Op::Write | Op::Accept | Op::Commit
        ) {
            leases.extend(self.step_lease(step).ok());
        }
        if step.op == Op::Prepare {
            for lease in [&step.source, &step.destination].into_iter().flatten() {
                leases.extend(self.lease(lease.zone, &lease.host, lease.epoch).ok());
            }
        }
        leases
    }

    fn apply(&mut self, step: &Step) -> Result<String, Rejection> {
        let now = step.at;
        let host = || HostId::new(step.host.as_deref().unwrap_or_default());
        let zone = ZoneId::new(step.zone.unwrap_or_default());
        match step.op {
            Op::Register => {
                let registration = self.hosts.register(host()?, now)?;
                Ok(format!("live_until={}", registration.expires_at_tick))
            }
            Op::Heartbeat => {
                let registration = self.hosts.heartbeat(&host()?, now)?;
                Ok(format!("live_until={}", registration.expires_at_tick))
            }
            Op::ExpireHosts => {
                let expired = self.hosts.expire(now);
                let names = expired
                    .iter()
                    .map(|registration| registration.host_id.as_str())
                    .collect::<Vec<_>>();
                Ok(format!("expired=[{}]", names.join(",")))
            }
            Op::Assign => {
                let lease = self.directory.assign(zone, host()?, &self.hosts, now)?;
                Ok(self.grant(lease, now))
            }
            Op::Reassign => {
                let expected = step.expected_epoch.unwrap_or_default();
                let lease = self
                    .directory
                    .reassign(zone, expected, host()?, &self.hosts, now)?;
                Ok(self.grant(lease, now))
            }
            Op::Renew => {
                let lease = self.directory.renew(&self.step_lease(step)?, now)?;
                Ok(format!(
                    "epoch={} expires={}",
                    lease.epoch, lease.expires_at_tick
                ))
            }
            Op::Release => {
                self.directory.release(&self.step_lease(step)?, now)?;
                Ok(String::new())
            }
            Op::ExpireLeases => {
                let expired = self.directory.expire(now);
                let names = expired
                    .iter()
                    .map(|lease| format!("{}@{}", lease.zone_id.get(), lease.epoch))
                    .collect::<Vec<_>>();
                Ok(format!("expired=[{}]", names.join(",")))
            }
            Op::Write => self.write(step, now),
            Op::Prepare => {
                let source = step.source.as_ref().ok_or_else(missing)?;
                let destination = step.destination.as_ref().ok_or_else(missing)?;
                let ticket = HandoffTicket {
                    transfer_id: TransferId::new(step.transfer.unwrap_or_default()),
                    entity_id: EntityId::new(step.entity.unwrap_or_default()),
                    source: self.lease(source.zone, &source.host, source.epoch)?,
                    destination: self.lease(
                        destination.zone,
                        &destination.host,
                        destination.epoch,
                    )?,
                };
                let phase = self.handoffs.prepare(ticket, &self.directory, now)?;
                Ok(format!("phase={}", phase_name(phase)))
            }
            Op::Accept => {
                let lease = self.step_lease(step)?;
                let transfer = TransferId::new(step.transfer.unwrap_or_default());
                let phase = self
                    .handoffs
                    .accept(transfer, &lease, &self.directory, now)?;
                Ok(format!("phase={}", phase_name(phase)))
            }
            Op::Commit => {
                let lease = self.step_lease(step)?;
                let transfer = TransferId::new(step.transfer.unwrap_or_default());
                let phase = self
                    .handoffs
                    .commit(transfer, &lease, &self.directory, now)?;
                Ok(format!("phase={}", phase_name(phase)))
            }
        }
    }

    /// Records a granted lease and starts the fenced runtime its host will use.
    fn grant(&mut self, lease: ZoneLease, now: u64) -> String {
        let detail = format!("epoch={} expires={}", lease.epoch, lease.expires_at_tick);
        if let Ok(runtime) =
            FencedZoneRuntime::new(lease.clone(), &self.directory, now, RECONNECT_GRACE_TICKS)
        {
            self.writers
                .insert((lease.zone_id, lease.host_id.clone(), lease.epoch), runtime);
        }
        self.history.grants.push(lease);
        detail
    }

    /// Advances the writer's zone one tick through `FencedZoneRuntime`, the
    /// same check that guards admission, commands and publication.
    fn write(&mut self, step: &Step, now: u64) -> Result<String, Rejection> {
        let lease = self.step_lease(step)?;
        let key = (lease.zone_id, lease.host_id.clone(), lease.epoch);
        let runtime = match self.writers.remove(&key) {
            Some(runtime) => runtime,
            None => FencedZoneRuntime::new(lease, &self.directory, now, RECONNECT_GRACE_TICKS)?,
        };
        let mut runtime = runtime;
        let result = runtime.execute(&self.directory, now, MatchRuntime::advance_tick);
        self.writers.insert(key, runtime);
        let snapshot = result?;
        Ok(format!("zone_tick={}", snapshot.tick))
    }

    fn owners(&self, observation: &Observation) -> String {
        if observation.leases.is_empty() {
            return "no owners".into();
        }
        observation
            .leases
            .values()
            .map(|lease| {
                format!(
                    "zone{}={}@{}<{}",
                    lease.zone_id.get(),
                    lease.host_id.as_str(),
                    lease.epoch,
                    lease.expires_at_tick
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn missing() -> Rejection {
    Rejection {
        kind: "invalid_step".into(),
        message: "validated step is missing a lease".into(),
    }
}

fn describe(step: &Step) -> String {
    let mut parts = Vec::new();
    if let Some(transfer) = step.transfer {
        parts.push(format!("transfer={transfer}"));
    }
    if let Some(entity) = step.entity {
        parts.push(format!("entity={entity}"));
    }
    if let Some(zone) = step.zone {
        parts.push(format!("zone={zone}"));
    }
    if let Some(host) = &step.host {
        parts.push(format!("host={host}"));
    }
    if let Some(epoch) = step.epoch {
        parts.push(format!("epoch={epoch}"));
    }
    if let Some(epoch) = step.expected_epoch {
        parts.push(format!("expected_epoch={epoch}"));
    }
    for (label, lease) in [("source", &step.source), ("destination", &step.destination)] {
        if let Some(lease) = lease {
            parts.push(format!(
                "{label}={}:{}@{}",
                lease.zone, lease.host, lease.epoch
            ));
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(zone: u32, host: &str, epoch: u64, expires_at_tick: u64) -> ZoneLease {
        ZoneLease {
            zone_id: ZoneId::new(zone),
            host_id: HostId::new(host).unwrap(),
            epoch,
            expires_at_tick,
        }
    }

    fn attempt(op: Op, succeeded: bool) -> Attempt<'static> {
        Attempt {
            op,
            now: 10,
            presented: Vec::new(),
            expected_epoch: None,
            transfer: None,
            succeeded,
        }
    }

    fn names(violations: &[(&'static str, String)]) -> Vec<&'static str> {
        violations.iter().map(|(name, _)| *name).collect()
    }

    fn observation(leases: &[ZoneLease]) -> Observation {
        Observation {
            leases: leases
                .iter()
                .map(|lease| (lease.zone_id, lease.clone()))
                .collect(),
            ..Observation::default()
        }
    }

    #[test]
    fn a_reused_zone_epoch_is_a_double_writer() {
        let history = History {
            grants: vec![lease(1, "host-a", 1, 100), lease(1, "host-b", 1, 100)],
            ..History::default()
        };
        let state = observation(&[lease(1, "host-b", 1, 100)]);
        let violations = check_invariants(&history, &state, &state, &attempt(Op::Assign, true));
        assert_eq!(names(&violations), ["single_writer"]);
    }

    #[test]
    fn success_with_a_stale_epoch_is_unfenced() {
        let current = lease(1, "host-b", 2, 100);
        let stale = lease(1, "host-a", 1, 100);
        let history = History {
            grants: vec![stale.clone(), current.clone()],
            ..History::default()
        };
        let state = observation(&[current]);
        let mut renew = attempt(Op::Renew, true);
        renew.presented = vec![&stale];
        let violations = check_invariants(&history, &state, &state, &renew);
        assert_eq!(names(&violations), ["epoch_fenced"]);
    }

    #[test]
    fn a_rejected_operation_must_not_change_state() {
        let granted = lease(1, "host-a", 1, 100);
        let history = History {
            grants: vec![granted.clone()],
            ..History::default()
        };
        let before = observation(&[granted]);
        let after = Observation::default();
        let violations = check_invariants(&history, &before, &after, &attempt(Op::Release, false));
        assert_eq!(names(&violations), ["epoch_fenced"]);
    }

    fn record(transfer: u64, entity: u64, phase: HandoffPhase) -> (TransferId, HandoffRecord) {
        (
            TransferId::new(transfer),
            HandoffRecord {
                ticket: HandoffTicket {
                    transfer_id: TransferId::new(transfer),
                    entity_id: EntityId::new(entity),
                    source: lease(1, "host-a", 1, 100),
                    destination: lease(2, "host-b", 1, 100),
                },
                phase,
            },
        )
    }

    #[test]
    fn commit_without_acceptance_retires_the_source_too_early() {
        let after = Observation {
            records: [record(7, 42, HandoffPhase::Committed)].into(),
            ..Observation::default()
        };
        let history = History {
            phases: [(TransferId::new(7), vec![HandoffPhase::Prepared])].into(),
            ..History::default()
        };
        let violations = check_invariants(
            &history,
            &Observation::default(),
            &after,
            &attempt(Op::Commit, true),
        );
        assert_eq!(names(&violations), ["retire_after_accept"]);
    }

    #[test]
    fn repeating_a_phase_must_not_change_the_transfer() {
        let before = Observation {
            records: [record(7, 42, HandoffPhase::Accepted)].into(),
            ..Observation::default()
        };
        let after = Observation {
            records: [record(7, 43, HandoffPhase::Accepted)].into(),
            ..Observation::default()
        };
        let mut accept = attempt(Op::Accept, true);
        accept.transfer = Some(TransferId::new(7));
        let violations = check_invariants(&History::default(), &before, &after, &accept);
        assert_eq!(
            names(&violations),
            ["handoff_idempotent", "handoff_idempotent"]
        );
    }

    #[test]
    fn two_unfinished_transfers_for_one_entity_are_duplicates() {
        let after = Observation {
            records: [
                record(7, 42, HandoffPhase::Prepared),
                record(8, 42, HandoffPhase::Accepted),
            ]
            .into(),
            ..Observation::default()
        };
        let violations = check_invariants(
            &History::default(),
            &Observation::default(),
            &after,
            &attempt(Op::Prepare, true),
        );
        assert_eq!(names(&violations), ["handoff_idempotent"]);
    }
}
