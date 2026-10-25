//! Promoting an approved Action Lab configuration (#383, Atlas 5.6.7).
//!
//! The lab never changes production rules. A developer explicitly picks a
//! saved configuration version in the browser and exports a
//! [`Request`]. The `cargo run -p mmorpg-wasm --example action-lab -- promote` tool then, in a
//! checkout, re-validates it, replays its scenario under the production rules
//! and the candidate, rewrites the production graph's source
//! (`crates/mmorpg-core/src/action/baseline.rs`) and writes a promotion
//! [`Record`] plus a pull request description. The change becomes gameplay
//! only when that pull request is reviewed and merged; the record keeps the
//! previous configuration for comparison and rollback.

use std::fmt::{Debug, Write as _};

use mmorpg_core::action::{
    ActionGraph, Effect, Interrupted, Movement, Precision, Scope, Setting, Window,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::lab_diagnostics::{Finding, Severity};
use crate::lab_experiment::{self, Config, Run, ScenarioFile};

/// The `format` of a promotion request the lab exports.
pub const REQUEST_FORMAT: &str = "mmorpg-action-lab-promotion";
/// The `format` of a checked-in promotion record.
pub const RECORD_FORMAT: &str = "mmorpg-action-lab-promotion-record";
/// The production graph's source, relative to the repository root.
pub const BASELINE_SOURCE: &str = "crates/mmorpg-core/src/action/baseline.rs";
/// Where promotion records live, relative to the repository root.
pub const RECORDS: &str = "crates/mmorpg-wasm/promotions";
/// The line after which [`BASELINE_SOURCE`] is generated.
pub const MARKER: &str =
    "// @generated below by `cargo run -p mmorpg-wasm --example action-lab -- promote` (#383).";

/// A saved configuration version a developer chose to promote, with the
/// scenario that is its evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub format: String,
    /// The saved version's number under its configuration name.
    pub version: u32,
    /// The identity of the production rules the lab compared against.
    pub production: String,
    pub config: Config,
    pub scenario: ScenarioFile,
    /// Why this should become gameplay, for the pull request.
    pub note: String,
}

/// A checked-in promotion: what production was, what it became and the
/// scenario that shows the difference. The lab imports it as A/B.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub format: String,
    pub name: String,
    pub version: u32,
    pub identity: String,
    pub note: String,
    pub previous: Config,
    pub promoted: Config,
    pub scenario: ScenarioFile,
    pub diverges_at: Option<u64>,
}

/// The files a promotion writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Promotion {
    /// A branch-name-safe name: the configuration's name and version.
    pub slug: String,
    /// The new [`BASELINE_SOURCE`].
    pub source: String,
    /// The record, as pretty JSON, for `{RECORDS}/{slug}.json`.
    pub record: String,
    pub title: String,
    /// The pull request description.
    pub body: String,
}

impl Request {
    pub fn parse(json: &str) -> Result<Self, String> {
        let request: Self =
            serde_json::from_str(json).map_err(|error| format!("promotion: {error}"))?;
        if request.format != REQUEST_FORMAT {
            return Err(format!(
                "promotion format {:?} is not {REQUEST_FORMAT:?}",
                request.format
            ));
        }
        Ok(request)
    }
}

/// `text` as lowercase words joined by `-`.
#[must_use]
pub fn slug(text: &str) -> String {
    let mut slug = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_owned()
}

fn changes_into(path: &str, from: &Value, to: &Value, out: &mut Vec<String>) {
    match (from, to) {
        (Value::Object(left), Value::Object(right)) => {
            let at = |key: &str| {
                if path.is_empty() {
                    key.to_owned()
                } else {
                    format!("{path}.{key}")
                }
            };
            for (key, value) in left {
                match right.get(key) {
                    Some(other) => changes_into(&at(key), value, other, out),
                    None => out.push(format!("{}: {value} → removed", at(key))),
                }
            }
            for (key, value) in right.iter().filter(|(key, _)| !left.contains_key(*key)) {
                out.push(format!("{}: added {value}", at(key)));
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            for index in 0..left.len().max(right.len()) {
                let at = format!("{path}[{index}]");
                match (left.get(index), right.get(index)) {
                    (Some(value), Some(other)) => changes_into(&at, value, other, out),
                    (Some(value), None) => out.push(format!("{at}: {value} → removed")),
                    (None, Some(other)) => out.push(format!("{at}: added {other}")),
                    (None, None) => {}
                }
            }
        }
        _ if from != to => out.push(format!("{path}: {from} → {to}")),
        _ => {}
    }
}

/// The rule values that differ from `from` to `to` (names ignored).
#[must_use]
pub fn changes(from: &Config, to: &Config) -> Vec<String> {
    let value = |config: &Config| {
        let mut value = serde_json::to_value(config).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.remove("name");
        }
        value
    };
    let mut out = Vec::new();
    changes_into("", &value(from), &value(to), &mut out);
    out
}

fn path<T: Debug>(prefix: &str, value: T) -> String {
    format!("{prefix}::{value:?}")
}

fn list<T: Copy + Debug>(prefix: &str, values: &[T]) -> String {
    let items: Vec<String> = values.iter().map(|value| path(prefix, *value)).collect();
    format!("&[{}]", items.join(", "))
}

fn window(window: Window, indent: &str) -> String {
    format!(
        "Window {{\n{indent}    open_after_ms: {},\n{indent}    actionable_ms: {},\n{indent}    buffer_ms: {},\n{indent}}}",
        window.open_after_ms, window.actionable_ms, window.buffer_ms
    )
}

fn effects(values: &[Effect]) -> String {
    list("Effect", values)
}

fn precision(value: Option<Precision>) -> String {
    value.map_or_else(
        || "None".to_owned(),
        |precision| {
            format!(
                "Some(Precision {{\n            after_open_ms: {},\n            length_ms: {},\n            benefits: {},\n        }})",
                precision.after_open_ms,
                precision.length_ms,
                effects(precision.benefits)
            )
        },
    )
}

fn interrupted(value: Interrupted) -> String {
    match value {
        Interrupted::Action(id) => format!("Interrupted::Action({})", path("ActionId", id)),
        Interrupted::Edge(id) => format!("Interrupted::Edge({})", path("EdgeId", id)),
        Interrupted::Held(id) => format!("Interrupted::Held({})", path("RuleId", id)),
    }
}

fn scope(value: Scope) -> String {
    match value {
        Scope::Class(class) => format!("Scope::Class({})", path("PlayerClass", class)),
        Scope::Weapon(item) => format!("Scope::Weapon(ItemId::new({}))", item.get()),
        Scope::Ability(id) => format!("Scope::Ability(AbilityId::new({}))", id.get()),
    }
}

fn setting(value: Setting) -> String {
    match value {
        Setting::Recovery { action, ms } => format!(
            "Setting::Recovery {{\n            action: {},\n            ms: {ms},\n        }}",
            path("ActionId", action)
        ),
        Setting::Window { edge, window: at } => format!(
            "Setting::Window {{\n            edge: {},\n            window: {},\n        }}",
            path("EdgeId", edge),
            window(at, "            ")
        ),
        Setting::Rule { rule, applies } => format!(
            "Setting::Rule {{\n            rule: {},\n            applies: {applies},\n        }}",
            path("RuleId", rule)
        ),
        Setting::Movement {
            action,
            phase,
            movement: value,
        } => format!(
            "Setting::Movement {{\n            action: {},\n            phase: {},\n            movement: {},\n        }}",
            path("ActionId", action),
            path("Phase", phase),
            movement(value, "            ")
        ),
    }
}

fn movement(value: Movement, indent: &str) -> String {
    format!(
        "Movement {{\n{indent}    steering_percent: {},\n{indent}    momentum_percent: {},\n{indent}    max_acceleration_cms2: {:?},\n{indent}}}",
        value.steering_percent, value.momentum_percent, value.max_acceleration_cms2
    )
}

fn array(out: &mut String, doc: &str, name: &str, kind: &str, items: &[String]) {
    let _ = writeln!(out, "\n/// {doc}");
    if items.is_empty() {
        let _ = writeln!(out, "pub const {name}: [{kind}; 0] = [];");
        return;
    }
    let _ = writeln!(out, "pub const {name}: [{kind}; {}] = [", items.len());
    for item in items {
        let _ = writeln!(out, "    {item},");
    }
    let _ = writeln!(out, "];");
}

/// The production graph's source for `graph`: `header` (everything up to and
/// including [`MARKER`]) followed by the generated definitions.
#[must_use]
pub fn rust_source(header: &str, graph: &ActionGraph) -> String {
    let mut out = header.trim_end().to_owned();
    out.push_str("\n\nuse super::*;\n");
    let edges: Vec<String> = graph
        .edges
        .iter()
        .map(|def| {
            format!(
                "EdgeDef {{\n        id: {},\n        from: {},\n        input: {},\n        anchor: {},\n        window: {},\n        conditions: {},\n        effects: {},\n        precision: {},\n        priority: {},\n    }}",
                path("EdgeId", def.id),
                path("ActionId", def.from),
                path("ActionInput", def.input),
                path("Anchor", def.anchor),
                window(def.window, "        "),
                list("Condition", def.conditions),
                effects(def.effects),
                precision(def.precision),
                def.priority
            )
        })
        .collect();
    let modifiers: Vec<String> = graph
        .window_modifiers
        .iter()
        .map(|modifier| {
            format!(
                "WindowModifier {{\n        edge: {},\n        conditions: {},\n        actionable_delta_ms: {},\n    }}",
                path("EdgeId", modifier.edge),
                list("Condition", modifier.conditions),
                modifier.actionable_delta_ms
            )
        })
        .collect();
    let rules: Vec<String> = graph
        .rules
        .iter()
        .map(|rule| {
            format!(
                "CompatibilityRule {{\n        id: {},\n        during: {},\n        input: {},\n        conditions: {},\n        effect: {},\n    }}",
                path("RuleId", rule.id),
                path("ActionId", rule.during),
                path("ActionInput", rule.input),
                list("Condition", rule.conditions),
                path("RuleEffect", rule.effect)
            )
        })
        .collect();
    let interruptions: Vec<String> = graph
        .interruptions
        .iter()
        .map(|rule| {
            format!(
                "InterruptionRule {{\n        reaction: {},\n        interrupted: {},\n        phase: {},\n        consequence: {},\n    }}",
                path("ActionId", rule.reaction),
                interrupted(rule.interrupted),
                rule.phase
                    .map_or_else(|| "None".to_owned(), |phase| format!("Some({})", path("Phase", phase))),
                path("Consequence", rule.consequence)
            )
        })
        .collect();
    let overrides: Vec<String> = graph
        .overrides
        .iter()
        .map(|entry| {
            format!(
                "Override {{\n        scope: {},\n        setting: {},\n    }}",
                scope(entry.scope),
                setting(entry.setting)
            )
        })
        .collect();
    array(&mut out, "The baseline edges.", "EDGES", "EdgeDef", &edges);
    array(
        &mut out,
        "The baseline window modifiers.",
        "WINDOW_MODIFIERS",
        "WindowModifier",
        &modifiers,
    );
    array(
        &mut out,
        "The baseline compatibility rules.",
        "RULES",
        "CompatibilityRule",
        &rules,
    );
    array(
        &mut out,
        "The baseline interruptions.",
        "INTERRUPTIONS",
        "InterruptionRule",
        &interruptions,
    );
    let movement_rules: Vec<String> = graph
        .movement
        .iter()
        .map(|rule| {
            format!(
                "MovementRule {{\n        action: {},\n        phase: {},\n        conditions: {},\n        movement: {},\n    }}",
                path("ActionId", rule.action),
                path("Phase", rule.phase),
                list("Condition", rule.conditions),
                movement(rule.movement, "        ")
            )
        })
        .collect();
    array(
        &mut out,
        "The baseline authored movement of action phases.",
        "MOVEMENT",
        "MovementRule",
        &movement_rules,
    );
    array(
        &mut out,
        "The baseline class, weapon and ability overrides.",
        "OVERRIDES",
        "Override",
        &overrides,
    );
    out.push_str(
        "\n/// The production graph.\npub const BASELINE: ActionGraph = ActionGraph {\n    edges: &EDGES,\n    window_modifiers: &WINDOW_MODIFIERS,\n    rules: &RULES,\n    interruptions: &INTERRUPTIONS,\n    movement: &MOVEMENT,\n    overrides: &OVERRIDES,\n};\n",
    );
    out
}

/// The hand-written part of a production graph source, through [`MARKER`].
pub fn header(source: &str) -> Result<&str, String> {
    let at = source
        .find(MARKER)
        .ok_or_else(|| format!("{BASELINE_SOURCE} lacks the line {MARKER:?}"))?;
    Ok(&source[..at + MARKER.len()])
}

fn mean_delay(run: &Run) -> String {
    let delays: Vec<u64> = run
        .commands
        .iter()
        .filter_map(|command| command.delay_ticks)
        .collect();
    if delays.is_empty() {
        return "—".to_owned();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a display average of a few delays"
    )]
    let mean = delays.iter().sum::<u64>() as f64 / delays.len() as f64;
    format!("{mean:.1} t")
}

fn finding_lines(out: &mut String, findings: &[Finding]) {
    let warnings: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Warning)
        .collect();
    if warnings.is_empty() {
        out.push_str("- none\n");
    }
    for finding in warnings {
        let _ = writeln!(out, "- tick {}: {}", finding.tick, finding.summary);
    }
}

/// Validates `request` against the production rules this build runs and
/// produces the promotion's files. `source` is the current
/// [`BASELINE_SOURCE`].
pub fn promote(request: &Request, source: &str) -> Result<Promotion, String> {
    let previous = Config::baseline();
    let production = previous.identity();
    if request.production != production {
        return Err(format!(
            "the lab compared against production {}, this checkout runs {production}: \
             re-run the comparison against the current rules and export again",
            request.production
        ));
    }
    let graph = request.config.graph()?;
    let identity = request.config.identity();
    if identity == production {
        return Err("the configuration equals production: nothing to promote".to_owned());
    }
    let comparison = lab_experiment::compare(&request.scenario, &previous, &request.config)?;
    let slug = format!("{}-v{}", slug(&request.config.name), request.version);
    let mut promoted = request.config.clone();
    promoted.name = format!("{} v{}", request.config.name, request.version);
    let record = Record {
        format: RECORD_FORMAT.to_owned(),
        name: request.config.name.clone(),
        version: request.version,
        identity: identity.clone(),
        note: request.note.clone(),
        previous: previous.clone(),
        promoted,
        scenario: request.scenario.clone(),
        diverges_at: comparison.diverges_at,
    };
    let title = format!(
        "Action Lab: promote {} v{} ({identity})",
        request.config.name, request.version
    );
    let (a, b) = (&comparison.a, &comparison.b);
    let mut body = String::new();
    let _ = writeln!(
        body,
        "Promotes the Action Lab configuration **{} v{}** (`{identity}`) to the production action graph (#383, Atlas 5.6.7). It becomes gameplay only when this PR merges.\n",
        request.config.name, request.version
    );
    if !request.note.trim().is_empty() {
        let _ = writeln!(body, "{}\n", request.note.trim());
    }
    body.push_str("**Rule changes** (from production `");
    body.push_str(&production);
    body.push_str("`):\n");
    for change in changes(&previous, &request.config) {
        let _ = writeln!(body, "- `{change}`");
    }
    let _ = writeln!(
        body,
        "\n**Evidence**: scenario *{}* ({} ticks, {} commands, `{RECORDS}/{slug}.json`), replayed by `cargo run -p mmorpg-wasm --example action-lab -- promote`.\n",
        request.scenario.name,
        request.scenario.ticks,
        request.scenario.commands.len()
    );
    body.push_str("| | production | promoted |\n| --- | --- | --- |\n");
    let mut row = |name: &str, left: String, right: String| {
        let _ = writeln!(body, "| {name} | {left} | {right} |");
    };
    row(
        "Damage dealt",
        a.summary.damage_dealt.to_string(),
        b.summary.damage_dealt.to_string(),
    );
    row(
        "Damage taken",
        a.summary.damage_taken.to_string(),
        b.summary.damage_taken.to_string(),
    );
    row(
        "Refusals",
        a.summary.refusals.to_string(),
        b.summary.refusals.to_string(),
    );
    row("Mean input→effect", mean_delay(a), mean_delay(b));
    let _ = writeln!(
        body,
        "\n{}\n",
        comparison.diverges_at.map_or_else(
            || "The runs never diverge.".to_owned(),
            |tick| format!("Canonical state first diverges at tick {tick}.")
        )
    );
    body.push_str(
        "**Diagnostic warnings under the promoted rules** (diagnostics, not a ranking):\n",
    );
    finding_lines(&mut body, &b.findings);
    body.push_str("\n**Combos that worked under production and fail now:**\n");
    finding_lines(&mut body, &comparison.regressions);
    let _ = writeln!(
        body,
        "\nRollback: revert this PR. The record's `previous` is the replaced configuration; import the record in the Action Lab to compare both."
    );
    Ok(Promotion {
        slug,
        source: rust_source(header(source)?, graph),
        record: serde_json::to_string_pretty(&record).map_err(|error| error.to_string())? + "\n",
        title,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::action;

    const SOURCE: &str = include_str!("../../mmorpg-core/src/action/baseline.rs");

    #[test]
    fn the_generator_reproduces_the_production_source() {
        let generated = rust_source(header(SOURCE).unwrap(), &action::BASELINE);
        if std::env::var_os("MMORPG_UPDATE_FIXTURES").is_some() {
            std::fs::write(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../mmorpg-core/src/action/baseline.rs"
                ),
                &generated,
            )
            .unwrap();
            return;
        }
        assert_eq!(generated, SOURCE);
    }

    fn scenario() -> ScenarioFile {
        ScenarioFile {
            format: lab_experiment::SCENARIO_FORMAT.to_owned(),
            name: "jump".to_owned(),
            setup: lab_experiment::SetupDto {
                class_name: "warden".to_owned(),
                level: 6,
                gear: "standard".to_owned(),
                main_hand: 0,
                resource: Some(60),
            },
            seed: "0".to_owned(),
            content_revision: crate::host::hosted_content().revision().to_string(),
            protocol_id: mmorpg_protocol::PROTOCOL_ID,
            ticks: 30,
            commands: Vec::new(),
            recorded: None,
        }
    }

    #[test]
    fn a_promotion_rewrites_the_source_and_records_the_previous_rules() {
        let mut config = Config::baseline();
        config.name = "Longer bash window".to_owned();
        config.edges[0].actionable_ms = 800;
        let request = Request {
            format: REQUEST_FORMAT.to_owned(),
            version: 2,
            production: Config::baseline().identity(),
            config: config.clone(),
            scenario: scenario(),
            note: "Wider window for new players.".to_owned(),
        };
        let promotion = promote(&request, SOURCE).unwrap();
        assert_eq!(promotion.slug, "longer-bash-window-v2");
        assert!(promotion.source.starts_with(header(SOURCE).unwrap()));
        assert!(promotion.source.contains("actionable_ms: 800,"));
        assert!(
            promotion
                .body
                .contains("`edges[0].actionableMs: 600 → 800`")
        );
        let record: Record = serde_json::from_str(&promotion.record).unwrap();
        assert_eq!(record.previous, Config::baseline());
        assert_eq!(record.identity, config.identity());
        // Fails closed against changed production or no change at all.
        let stale = Request {
            production: "exp-0".to_owned(),
            ..request.clone()
        };
        assert!(promote(&stale, SOURCE).is_err());
        let unchanged = Request {
            config: Config::baseline(),
            ..request
        };
        assert!(promote(&unchanged, SOURCE).is_err());
    }

    #[test]
    fn slugs_are_branch_safe() {
        assert_eq!(slug("  Faster Bash (test)! "), "faster-bash-test");
    }
}
