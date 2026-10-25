//! Explainable Action Lab diagnostics (#383, Atlas 5.6.6).
//!
//! Every finding is read off a replayed [`Run`]: the region actions, the
//! edges' authoritative windows and each command's explained arbitration
//! that `mmorpg-core` reported while the scenario ran. Nothing here decides
//! or re-simulates an outcome. A finding is a diagnostic within the analyzed
//! scenario, not a claim that some configuration is better: the lab never
//! ranks configurations by speed or damage.

use mmorpg_core::action::{
    ActionGraph, Arbitration, BlockReason, Candidate, EdgeId, EdgeReport, EdgeStatus, Outcome,
    RuleId, Verdict,
};
use serde::Serialize;

use crate::lab_experiment::{CommandResult, Run, TickRecord, decision, edge_trace};

/// An input taking effect this much later than the next tick is lag (ms).
pub const LAG_MS: u32 = 100;
/// This many cancelled casts within [`CANCEL_LOOP_MS`] is a cancel loop.
pub const CANCEL_LOOP_COUNT: usize = 3;
pub const CANCEL_LOOP_MS: u32 = 2_000;
/// Commands awaiting an effect for at most this long are pending (ms).
const PENDING_MS: u32 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// An edge never became eligible within the scenario.
    UnreachableTransition,
    /// A buffered input was lost before its edge opened.
    ExpiredBuffer,
    /// An input took effect later than the next tick.
    InputLag,
    /// Several usable candidates competed for one input.
    ArbitrationConflict,
    /// A rule cancelled casts again and again.
    CancelLoop,
    /// A combo that succeeded under A fails under B.
    ComboRegression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Likely a timing or transition defect worth a look.
    Warning,
    /// Explained behavior worth knowing (a hold, a priority, an unused edge).
    Note,
}

/// What the zone reported at the tick a finding is about.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    /// The completed tick whose state the finding describes.
    pub tick: u64,
    /// Active state per region (movement, offense, defense, reaction).
    pub regions: Vec<String>,
    /// Commands still awaiting their effect at that tick.
    pub pending: Vec<String>,
    /// The edge's status (its condition) and eligible window.
    #[serde(serialize_with = "edge_option")]
    pub edge: Option<EdgeReport>,
    /// The events of the edge's authoritative anchor tick.
    pub source_events: Vec<String>,
    /// The observed arbitration of the command involved.
    #[serde(rename = "decision", serialize_with = "decision_option")]
    pub arbitration: Option<Arbitration>,
    /// The index of the command involved.
    pub command: Option<usize>,
    /// The first tick the compared runs' canonical state differs.
    pub diverges_at: Option<u64>,
}

#[expect(clippy::ref_option, reason = "serde's serialize_with signature")]
fn edge_option<S: serde::Serializer>(
    edge: &Option<EdgeReport>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    edge.as_ref().map(edge_trace).serialize(serializer)
}

#[expect(clippy::ref_option, reason = "serde's serialize_with signature")]
fn decision_option<S: serde::Serializer>(
    arbitration: &Option<Arbitration>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    arbitration.as_ref().map(decision).serialize(serializer)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub kind: Kind,
    pub severity: Severity,
    pub tick: u64,
    pub summary: String,
    pub trace: Trace,
}

fn ms(ticks: u64) -> u64 {
    ticks * 1_000 / u64::from(mmorpg_core::TICK_HZ)
}

fn ticks(ms: u32) -> u64 {
    u64::from(mmorpg_core::ticks_from_ms(ms))
}

/// The record of completed tick `tick`; tick 0 (before the first) has none.
fn record(run: &Run, tick: u64) -> Option<&TickRecord> {
    run.ticks
        .binary_search_by_key(&tick, |record| record.tick)
        .ok()
        .map(|index| &run.ticks[index])
}

fn edge_at(run: &Run, tick: u64, edge: EdgeId) -> Option<EdgeReport> {
    record(run, tick)?
        .edges
        .iter()
        .copied()
        .find(|report| report.edge == edge)
}

fn trace(run: &Run, tick: u64, edge: Option<EdgeReport>, command: Option<&CommandResult>) -> Trace {
    let horizon = ticks(PENDING_MS);
    let pending = run
        .commands
        .iter()
        .filter(|other| other.arbitration.is_some() && u64::from(other.tick) <= tick)
        .filter(|other| match other.effect_tick {
            Some(effect) => effect > tick,
            None => other.refused.is_empty() && tick - u64::from(other.tick) <= horizon,
        })
        .map(|other| format!("#{} {} (tick {})", other.index, other.label, other.tick))
        .collect();
    let source_events = edge
        .and_then(|edge| edge.anchor_tick)
        .and_then(|anchor| record(run, anchor))
        .map(|anchor| anchor.events.clone())
        .unwrap_or_default();
    Trace {
        tick,
        regions: record(run, tick)
            .map(|record| record.regions.clone())
            .unwrap_or_default(),
        pending,
        edge,
        source_events,
        arbitration: command.and_then(|command| command.arbitration.clone()),
        command: command.map(|command| command.index),
        diverges_at: None,
    }
}

const fn usable(status: EdgeStatus) -> bool {
    matches!(
        status,
        EdgeStatus::Open | EdgeStatus::Buffering | EdgeStatus::Suspended(_)
    )
}

fn reason(status: EdgeStatus) -> String {
    match status {
        EdgeStatus::Blocked(BlockReason::Condition(condition)) => {
            format!("condition {condition:?} did not hold")
        }
        EdgeStatus::Blocked(BlockReason::Expired) => "its window expired".to_owned(),
        EdgeStatus::Blocked(BlockReason::Cancelled(reaction)) => {
            format!("{reaction:?} cancelled the opening")
        }
        EdgeStatus::Blocked(BlockReason::TooEarly) => "it was too early".to_owned(),
        EdgeStatus::Blocked(BlockReason::SourceInactive) => "its source ended".to_owned(),
        other => format!("{other:?}"),
    }
}

fn window_text(report: &EdgeReport) -> String {
    match (report.buffer_from, report.open_from, report.open_until) {
        (Some(buffer), Some(open), Some(until)) => {
            format!("buffer from tick {buffer}, open ticks {open}–{until}")
        }
        _ => "no window".to_owned(),
    }
}

/// Edges that never became eligible in the scenario.
fn unreachable(run: &Run, graph: &ActionGraph, findings: &mut Vec<Finding>) {
    for def in graph.edges {
        let reports: Vec<(u64, EdgeReport)> = run
            .ticks
            .iter()
            .filter_map(|record| {
                let report = record.edges.iter().find(|report| report.edge == def.id)?;
                Some((record.tick, *report))
            })
            .collect();
        if reports.iter().any(|(_, report)| usable(report.status)) {
            continue;
        }
        let Some(&(tick, report)) = reports.first() else {
            findings.push(Finding {
                kind: Kind::UnreachableTransition,
                severity: Severity::Note,
                tick: 0,
                summary: format!(
                    "{:?} is not exercised: its source {:?} never ran in this scenario.",
                    def.id, def.from
                ),
                trace: trace(run, 0, None, None),
            });
            continue;
        };
        findings.push(Finding {
            kind: Kind::UnreachableTransition,
            severity: Severity::Warning,
            tick,
            summary: format!(
                "{:?} never became eligible although {:?} ran: at tick {tick} {}.",
                def.id,
                def.from,
                reason(report.status)
            ),
            trace: trace(run, tick, Some(report), None),
        });
    }
}

/// Where a buffered input for `edge`, issued at `from`, was lost; `None`
/// when the edge opened for it.
fn lost(run: &Run, from: u64, edge: EdgeId) -> Option<(u64, Option<EdgeReport>)> {
    let mut last = None;
    for record in run.ticks.iter().filter(|record| record.tick >= from) {
        let report = record
            .edges
            .iter()
            .copied()
            .find(|report| report.edge == edge);
        match report.map(|report| report.status) {
            Some(EdgeStatus::Open) => return None,
            Some(
                EdgeStatus::Buffering
                | EdgeStatus::Suspended(_)
                | EdgeStatus::Blocked(BlockReason::TooEarly),
            ) => last = report,
            Some(_) => return Some((record.tick, report)),
            None => return Some((record.tick, last)),
        }
    }
    None
}

fn expired_buffers(run: &Run, findings: &mut Vec<Finding>) {
    for command in &run.commands {
        let Some(Outcome::WaitForEdge(edge)) = command.arbitration.as_ref().map(|a| a.outcome)
        else {
            continue;
        };
        let Some((tick, report)) = lost(run, u64::from(command.tick), edge) else {
            continue;
        };
        let why = report.map_or_else(
            || "its source ended".to_owned(),
            |report| {
                if report.status == EdgeStatus::Buffering {
                    "its source ended".to_owned()
                } else {
                    reason(report.status)
                }
            },
        );
        let window = edge_at(run, u64::from(command.tick), edge)
            .map(|report| window_text(&report))
            .unwrap_or_default();
        findings.push(Finding {
            kind: Kind::ExpiredBuffer,
            severity: Severity::Warning,
            tick,
            summary: format!(
                "#{} {} was buffered at tick {} for {edge:?} ({window}) but lost at tick {tick}: {why}.",
                command.index, command.label, command.tick
            ),
            trace: trace(run, tick, report, Some(command)),
        });
    }
}

fn input_lag(run: &Run, findings: &mut Vec<Finding>) {
    let threshold = 1 + ticks(LAG_MS);
    for command in &run.commands {
        let (Some(arbitration), Some(delay)) = (&command.arbitration, command.delay_ticks) else {
            continue;
        };
        if delay <= threshold {
            continue;
        }
        let at = u64::from(command.tick);
        let lag = format!("{delay} ticks ({} ms)", ms(delay));
        let (severity, summary, edge) = match arbitration.outcome {
            // A refused input (#413) never takes effect; refusals report it.
            Outcome::Refuse(_) => continue,
            Outcome::WaitForRule(rule) => (
                Severity::Note,
                format!(
                    "#{} {} took effect after {lag}: {rule:?} held it.",
                    command.index, command.label
                ),
                None,
            ),
            Outcome::WaitForEdge(edge) => (
                Severity::Note,
                format!(
                    "#{} {} took effect after {lag}: it waited for {edge:?} to open.",
                    command.index, command.label
                ),
                edge_at(run, at, edge),
            ),
            Outcome::Proceed(_) | Outcome::Cancel(_) => (
                Severity::Warning,
                format!(
                    "#{} {} took effect after {lag} although the graph let it proceed at once; \
                     ordinary validation (cooldown, range, swing timer) delayed it.",
                    command.index, command.label
                ),
                None,
            ),
        };
        findings.push(Finding {
            kind: Kind::InputLag,
            severity,
            tick: at,
            summary,
            trace: trace(run, at, edge, Some(command)),
        });
    }
}

fn conflicts(run: &Run, findings: &mut Vec<Finding>) {
    for command in &run.commands {
        let Some(arbitration) = &command.arbitration else {
            continue;
        };
        let outranked: Vec<String> = arbitration
            .candidates
            .iter()
            .filter(|(_, verdict)| matches!(verdict, Verdict::Outranked(_)))
            .map(|(candidate, _)| format!("{candidate:?}"))
            .collect();
        if outranked.is_empty() {
            continue;
        }
        let winner = arbitration
            .candidates
            .iter()
            .find(|(_, verdict)| matches!(verdict, Verdict::Won | Verdict::Holding))
            .map_or_else(
                || "none".to_owned(),
                |(candidate, _)| format!("{candidate:?}"),
            );
        let edge = arbitration
            .candidates
            .iter()
            .find_map(|(candidate, _)| match candidate {
                Candidate::Edge(edge) => edge_at(run, u64::from(command.tick), *edge),
                Candidate::Direct => None,
            });
        findings.push(Finding {
            kind: Kind::ArbitrationConflict,
            severity: Severity::Note,
            tick: u64::from(command.tick),
            summary: format!(
                "#{} {}: {winner} won by priority over usable {} ({:?}).",
                command.index,
                command.label,
                outranked.join(", "),
                arbitration.outcome
            ),
            trace: trace(run, u64::from(command.tick), edge, Some(command)),
        });
    }
}

fn cancel_loops(run: &Run, findings: &mut Vec<Finding>) {
    let cancels: Vec<(&CommandResult, RuleId)> = run
        .commands
        .iter()
        .filter_map(|command| match command.arbitration.as_ref()?.outcome {
            Outcome::Cancel(rule) => Some((command, rule)),
            _ => None,
        })
        .collect();
    let span = ticks(CANCEL_LOOP_MS);
    let mut start = 0;
    while start < cancels.len() {
        let (first, rule) = cancels[start];
        let end = cancels[start..]
            .iter()
            .take_while(|(command, other)| {
                *other == rule && u64::from(command.tick - first.tick) <= span
            })
            .count();
        if end >= CANCEL_LOOP_COUNT {
            let (last, _) = cancels[start + end - 1];
            findings.push(Finding {
                kind: Kind::CancelLoop,
                severity: Severity::Warning,
                tick: u64::from(first.tick),
                summary: format!(
                    "{rule:?} cancelled {end} casts between ticks {} and {}.",
                    first.tick, last.tick
                ),
                trace: trace(run, u64::from(last.tick), None, Some(last)),
            });
            start += end;
        } else {
            start += 1;
        }
    }
}

/// The edge a command's combo succeeded through, if it did.
fn combo(run: &Run, command: &CommandResult) -> Option<EdgeId> {
    if !command.refused.is_empty() {
        return None;
    }
    match command.arbitration.as_ref()?.outcome {
        Outcome::Proceed(Candidate::Edge(edge)) => Some(edge),
        Outcome::WaitForEdge(edge)
            if command.effect_tick.is_some()
                && lost(run, u64::from(command.tick), edge).is_none() =>
        {
            Some(edge)
        }
        _ => None,
    }
}

/// The findings of one run under `graph`, in tick order.
#[must_use]
pub fn diagnose(run: &Run, graph: &ActionGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    unreachable(run, graph, &mut findings);
    expired_buffers(run, &mut findings);
    input_lag(run, &mut findings);
    conflicts(run, &mut findings);
    cancel_loops(run, &mut findings);
    findings.sort_by_key(|finding| finding.tick);
    findings
}

/// Combos that succeeded under `a` and fail under `b`, traced in `b`.
#[must_use]
pub fn regressions(a: &Run, b: &Run, diverges_at: Option<u64>) -> Vec<Finding> {
    a.commands
        .iter()
        .filter_map(|before| {
            let edge = combo(a, before)?;
            let after = b
                .commands
                .iter()
                .find(|command| command.index == before.index)?;
            if combo(b, after) == Some(edge) {
                return None;
            }
            let tick = u64::from(after.tick);
            let what = if after.refused.is_empty() {
                after
                    .arbitration
                    .as_ref()
                    .map_or_else(|| "no decision".to_owned(), |a| format!("{:?}", a.outcome))
            } else {
                format!("refused {}", after.refused.join(", "))
            };
            let mut trace = trace(b, tick, edge_at(b, tick, edge), Some(after));
            trace.diverges_at = diverges_at;
            Some(Finding {
                kind: Kind::ComboRegression,
                severity: Severity::Warning,
                tick,
                summary: format!(
                    "#{} {} followed {edge:?} under A but not under B: {what}.",
                    after.index, after.label
                ),
                trace,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::action::{ActionInput, BlockReason, Origin};

    fn tick(tick: u64, edges: Vec<EdgeReport>) -> TickRecord {
        TickRecord {
            tick,
            regions: vec![String::new(); 4],
            events: Vec::new(),
            health: 100,
            resource: None,
            position: [0, 0, 0],
            speed: 0,
            digest: String::new(),
            edges,
        }
    }

    fn bash_edge(status: EdgeStatus) -> EdgeReport {
        EdgeReport {
            edge: EdgeId::SlashToShieldBash,
            priority: 1,
            status,
            anchor_tick: Some(10),
            buffer_from: Some(10),
            open_from: Some(22),
            open_until: Some(58),
            precision: None,
            origin: Origin::Baseline,
        }
    }

    fn bash(tick: u32, outcome: Outcome, refused: Vec<String>) -> CommandResult {
        CommandResult {
            index: 0,
            tick,
            label: "UseAbility".to_owned(),
            refused,
            effect_tick: None,
            delay_ticks: None,
            arbitration: Some(Arbitration {
                input: ActionInput::ShieldBash,
                outcome,
                candidates: vec![
                    (Candidate::Edge(EdgeId::SlashToShieldBash), Verdict::Holding),
                    (
                        Candidate::Direct,
                        Verdict::Outranked(Candidate::Edge(EdgeId::SlashToShieldBash)),
                    ),
                ],
                precise: false,
            }),
        }
    }

    fn run(ticks: Vec<TickRecord>, commands: Vec<CommandResult>) -> Run {
        Run {
            config: "exp".to_owned(),
            config_name: "test".to_owned(),
            baseline: true,
            content_revision: String::new(),
            content_changed: false,
            reproduces_recording: None,
            ticks,
            commands,
            summary: crate::lab_experiment::Summary::default(),
            findings: Vec::new(),
        }
    }

    #[test]
    fn a_buffered_input_lost_to_a_cancel_is_explained() {
        let ticks = vec![
            tick(12, vec![bash_edge(EdgeStatus::Buffering)]),
            tick(13, vec![bash_edge(EdgeStatus::Buffering)]),
            tick(
                14,
                vec![bash_edge(EdgeStatus::Blocked(BlockReason::Cancelled(
                    mmorpg_core::action::ActionId::Death,
                )))],
            ),
        ];
        let commands = vec![bash(
            12,
            Outcome::WaitForEdge(EdgeId::SlashToShieldBash),
            Vec::new(),
        )];
        let findings = diagnose(&run(ticks, commands), &mmorpg_core::action::BASELINE);
        let lost = findings
            .iter()
            .find(|finding| finding.kind == Kind::ExpiredBuffer)
            .unwrap();
        assert_eq!(lost.tick, 14);
        assert!(lost.summary.contains("Death cancelled the opening"));
        assert!(lost.summary.contains("open ticks 22–58"));
        assert_eq!(lost.trace.pending.len(), 1);
        assert!(lost.trace.arbitration.is_some());
        // The edge was eligible, so it is reachable, and the hold outranked
        // the direct use.
        assert!(
            !findings
                .iter()
                .any(|finding| finding.kind == Kind::UnreachableTransition)
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == Kind::ArbitrationConflict)
        );
    }

    #[test]
    fn a_combo_that_now_fails_is_a_regression() {
        let before = run(
            vec![tick(30, vec![bash_edge(EdgeStatus::Open)])],
            vec![bash(
                30,
                Outcome::Proceed(Candidate::Edge(EdgeId::SlashToShieldBash)),
                Vec::new(),
            )],
        );
        let after = run(
            vec![tick(
                30,
                vec![bash_edge(EdgeStatus::Blocked(BlockReason::Expired))],
            )],
            vec![bash(
                30,
                Outcome::Proceed(Candidate::Direct),
                vec!["NotReady".to_owned()],
            )],
        );
        let found = regressions(&before, &after, Some(25));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::ComboRegression);
        assert_eq!(found[0].trace.diverges_at, Some(25));
        assert!(found[0].summary.contains("refused NotReady"));
        assert!(regressions(&before, &before, None).is_empty());
    }

    #[test]
    fn an_edge_whose_source_never_ran_is_not_exercised() {
        let findings = diagnose(
            &run(vec![tick(1, Vec::new())], Vec::new()),
            &mmorpg_core::action::BASELINE,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, Kind::UnreachableTransition);
        assert_eq!(findings[0].severity, Severity::Note);
    }
}
