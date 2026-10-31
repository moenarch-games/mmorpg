/**
 * Action Lab experiments (#382, Atlas 5.6.2/5.6.4): recorded input scenarios, their tick-by-tick
 * edits, experimental action configurations and the replay/compare results `mmorpg-wasm` returns.
 *
 * The page owns no rules here. A configuration is the core action graph as JSON, validated by the
 * zone (`actionLabCheck`); a scenario is the command-wire bytes the player sent with the tick each
 * was applied at. Replays and comparisons run in the shared simulation (`actionLabRun`,
 * `actionLabCompare`). Saved scenarios and configurations live in this browser's storage only,
 * apart from characters, and never change production rules.
 */
import { isMovementCommand } from "../command-wire";
import type { LabSetup } from "./action-lab";

export const CONFIG_FORMAT = "mmorpg-action-lab-config";
export const SCENARIO_FORMAT = "mmorpg-action-lab-scenario";
export const BUNDLE_FORMAT = "mmorpg-action-lab-bundle";

export type LabEffect =
  | { kind: "TargetConfirmed" | "IgnoreGlobalCooldown" | "KeepOpening" }
  | { kind: "Recovery"; ms: number }
  | { kind: "ResourceRefund"; amount: number }
  /** A launch along the facing and upward in cm/s (#413). */
  | { kind: "Impulse"; forwardCms: number; upCms: number };

/** An action phase's bounded movement (#413, Atlas 5.9.2); see `mmorpg_core::action::Movement`. */
export type LabMovementRule = {
  action: string; phase: string; conditions: string[];
  steeringPercent: number; momentumPercent: number; maxAccelerationCms2: number | null;
};

export type LabConfigEdge = {
  id: string; from: string; input: string; anchor: string;
  openAfterMs: number; actionableMs: number; bufferMs: number;
  conditions: string[]; effects: LabEffect[];
  precision: { afterOpenMs: number; lengthMs: number; benefits: LabEffect[] } | null;
  priority: number;
};

export type LabScope = { kind: "Class"; class: string } | { kind: "Weapon"; item: number } | { kind: "Ability"; ability: number };
export type LabSetting =
  | { kind: "Recovery"; action: string; ms: number }
  | { kind: "Window"; edge: string; openAfterMs: number; actionableMs: number; bufferMs: number }
  | { kind: "Rule"; rule: string; applies: boolean }
  | { kind: "Movement"; action: string; phase: string; steeringPercent: number; momentumPercent: number; maxAccelerationCms2: number | null };

/** An experimental action graph, in `actionLabBaseline()`'s format. */
export type LabConfig = {
  format: typeof CONFIG_FORMAT;
  name: string;
  edges: LabConfigEdge[];
  windowModifiers: { edge: string; conditions: string[]; actionableDeltaMs: number }[];
  rules: { id: string; during: string; input: string; conditions: string[]; effect: string }[];
  interruptions: { reaction: string; interrupted: string; phase: string | null; consequence: string }[];
  /** Per-phase movement; absent when none is authored (production). */
  movement?: LabMovementRule[];
  overrides: { scope: LabScope; setting: LabSetting }[];
  /** New experimental actions (#414); absent when none is authored (production). */
  authored?: LabAuthoredAction[];
};

/** One timed phase of an authored action and its bounded movement. */
export type LabAuthoredPhase = { ms: number; steeringPercent: number; momentumPercent: number; maxAccelerationCms2: number | null };

/**
 * A new experimental action composed from existing primitives (#414, Atlas 5.9.4–5.9.5); see
 * `mmorpg_core::action::AuthoredAction`. The zone validates it; nothing here is a rule.
 */
export type LabAuthoredAction = {
  slot: number; name: string; class: string;
  /** `WardenMelee`, `ShieldBash` or `Ability`. */
  input: string;
  /** Only this ability's use; null is any the input covers. */
  ability: number | null;
  context: string;
  /** Absent for the plain press. */
  gesture?: LabGesture;
  /** Only as a timed follow-up of another authored action; absent otherwise. */
  follows?: LabFollowUp;
  conditions: string[];
  startup: LabAuthoredPhase; active: LabAuthoredPhase; recovery: LabAuthoredPhase;
  effects: LabEffect[];
};

/**
 * How a trigger is performed through `input-bindings`' press/release lifecycle (#414): the press,
 * hold-to-charge (released after `minMs`, at most the startup; an earlier release is the plain
 * use) or a chord while `partner` is held.
 */
export type LabGesture = { kind: "Press" } | { kind: "Charge"; minMs: number } | { kind: "Chord"; partner: string };

/**
 * A timed follow-up (#414): the trigger counts only `openAfterMs`…+`actionableMs` after the use of
 * the authored action in `source`; an earlier press waits up to `bufferMs` before the opening,
 * reaching back into the source's startup, and a press before that loses the follow-up.
 */
export type LabFollowUp = { source: number; openAfterMs: number; actionableMs: number; bufferMs: number };

/** The input contexts a trigger can need (Atlas 5.9.5): the plain binding, jump + attack, direction + action. */
export const INPUT_CONTEXTS: readonly string[] = [
  "Direct", "Airborne", "Moving(Any)", "Moving(Forward)", "Moving(Backward)", "Moving(Sideways)",
];

/** The held controls a chord can need (#414). */
export const CHORD_PARTNERS: readonly string[] = ["Guard", "Input(WardenMelee)", "Input(ShieldBash)", "Input(Ability)"];

/** The gesture choices of the editor, by label. */
export function gestureOf(choice: string, current: LabGesture | undefined, startupMs: number): LabGesture | undefined {
  if (choice === "Press") { return undefined; }
  if (choice === "Charge") { return current?.kind === "Charge" ? current : { kind: "Charge", minMs: Math.min(200, startupMs) }; }
  return { kind: "Chord", partner: choice.slice("Chord ".length) };
}

/** The editor label of `gesture`. */
export function gestureLabel(gesture: LabGesture | undefined): string {
  return !gesture || gesture.kind === "Press" ? "Press" : gesture.kind === "Charge" ? "Charge" : `Chord ${gesture.partner}`;
}

const FREE_PHASE = { steeringPercent: 100, momentumPercent: 0, maxAccelerationCms2: null };

/**
 * Adds a Warden's sliding slash (#414): Heroic Strike while moving forward winds up 100 ms, slides
 * 300 ms on its momentum with a 4 m/s forward launch, recovers 200 ms at half steering, and may
 * follow into Shield Bash 100–300 ms after the hit, waiving the global cooldown. Every part is an
 * existing primitive; the strike itself resolves through its ordinary rules.
 */
export function slidingSlashSample(config: LabConfig): LabConfig {
  const authored = config.authored ?? [];
  const slot = [0, 1, 2, 3, 4, 5, 6, 7].find((candidate) => !authored.some((action) => action.slot === candidate));
  if (slot === undefined) { return config; }
  const action: LabAuthoredAction = {
    slot, name: "Sliding slash", class: "warden", input: "WardenMelee", ability: 1, context: "Moving(Forward)",
    conditions: [],
    startup: { ms: 100, ...FREE_PHASE },
    active: { ms: 300, steeringPercent: 0, momentumPercent: 100, maxAccelerationCms2: null },
    recovery: { ms: 200, ...FREE_PHASE, steeringPercent: 50 },
    effects: [{ kind: "Impulse", forwardCms: 400, upCms: 0 }],
  };
  const used = new Set(config.edges.map((edge) => edge.id));
  const id = Array.from({ length: 16 }, (_, index) => `Authored(${index})`).find((candidate) => !used.has(candidate));
  const edges = id ? [...config.edges, {
    id, from: `Authored(${slot})`, input: "ShieldBash", anchor: "Impact",
    openAfterMs: 100, actionableMs: 200, bufferMs: 0,
    conditions: [], effects: [{ kind: "IgnoreGlobalCooldown" } as LabEffect], precision: null, priority: 2,
  }] : config.edges;
  return { ...config, authored: [...authored, action], edges };
}

/**
 * Adds a Warden's charged bash (#414): hold Shield Bash at least 200 ms (at most its 600 ms
 * startup, which slows the Warden) and release to bash with a 3 m/s lunge; a quicker tap is the
 * plain Bash. With a sliding slash present, the charged bash is instead its timed follow-up:
 * pressed 100–300 ms after the slash's hit, buffered from the slash's own wind-up.
 */
export function chargedBashSample(config: LabConfig): LabConfig {
  const authored = config.authored ?? [];
  const slot = [0, 1, 2, 3, 4, 5, 6, 7].find((candidate) => !authored.some((action) => action.slot === candidate));
  if (slot === undefined) { return config; }
  const slash = authored.find((action) => action.name === "Sliding slash");
  const action: LabAuthoredAction = {
    slot, name: slash ? "Charged finisher" : "Charged bash", class: "warden", input: "ShieldBash", ability: 2, context: "Direct",
    gesture: { kind: "Charge", minMs: 200 },
    ...(slash ? { follows: { source: slash.slot, openAfterMs: 100, actionableMs: 200, bufferMs: 200 } } : {}),
    conditions: [],
    startup: { ms: 600, ...FREE_PHASE, steeringPercent: 50 },
    active: { ms: 100, ...FREE_PHASE },
    recovery: { ms: 200, ...FREE_PHASE },
    effects: [{ kind: "Impulse", forwardCms: 300, upCms: 0 }, ...(slash ? [{ kind: "IgnoreGlobalCooldown" } as LabEffect] : [])],
  };
  return { ...config, authored: [...authored, action] };
}

/** Removes an authored action with the edges leaving it. */
export function removeAuthored(config: LabConfig, slot: number): LabConfig {
  const from = `Authored(${slot})`;
  let next: LabConfig = { ...config, authored: (config.authored ?? []).filter((action) => action.slot !== slot) };
  for (const edge of config.edges.filter((entry) => entry.from === from)) { next = removeEdge(next, edge.id); }
  return next;
}

/** Replaces the authored action in `slot`. */
export function setAuthored(config: LabConfig, action: LabAuthoredAction): LabConfig {
  return { ...config, authored: (config.authored ?? []).map((entry) => entry.slot === action.slot ? action : entry) };
}

/** The action phases movement can be authored for (`mmorpg_core::action::phases`, reactions excluded). */
export const MOVEMENT_PHASES: readonly { action: string; phase: string }[] = [
  { action: "Airborne", phase: "Active" },
  { action: "Landing", phase: "Recovery" },
  { action: "Swim", phase: "Active" },
  { action: "DirectedSlash", phase: "Recovery" },
  { action: "DirectedSlash", phase: "FollowUp" },
  { action: "Guard", phase: "Active" },
  { action: "Cast", phase: "Active" },
  { action: "GlobalCooldown", phase: "Recovery" },
];

/** The authored movement of `action` in `phase`, or null when it moves freely. */
export function movementRule(config: LabConfig, action: string, phase: string): LabMovementRule | null {
  return config.movement?.find((rule) => rule.action === action && rule.phase === phase) ?? null;
}

/** Authors (or, with null, removes) the movement of `action` in `phase`. */
export function setMovementRule(config: LabConfig, action: string, phase: string,
  movement: Omit<LabMovementRule, "action" | "phase" | "conditions"> | null): LabConfig {
  const existing = movementRule(config, action, phase);
  const rest = (config.movement ?? []).filter((rule) => !(rule.action === action && rule.phase === phase));
  const next = movement ? [...rest, { action, phase, conditions: existing?.conditions ?? [], ...movement }] : rest;
  // An override of removed movement would be refused; drop it with the rule.
  const overrides = movement ? config.overrides : config.overrides.filter((entry) =>
    !(entry.setting.kind === "Movement" && entry.setting.action === action && entry.setting.phase === phase));
  return { ...config, movement: next, overrides };
}

/** The edges out of a landing (#413), by id, with the input each answers. */
export const LANDING_EDGES: readonly { id: string; input: string }[] = [
  { id: "LandingToMelee", input: "WardenMelee" },
  { id: "LandingToShieldBash", input: "ShieldBash" },
  { id: "LandingToAbility", input: "Ability" },
];

/**
 * Adds a landing edge: 100 ms after touchdown for 100 ms (inside the 300 ms landing), buffering
 * 100 ms, after an airborne melee hit, outranking the slash combo.
 */
export function addLandingEdge(config: LabConfig, id: string): LabConfig {
  const known = LANDING_EDGES.find((edge) => edge.id === id);
  if (!known || config.edges.some((edge) => edge.id === id)) { return config; }
  const edge: LabConfigEdge = {
    id, from: "Landing", input: known.input, anchor: "Landing",
    openAfterMs: 100, actionableMs: 100, bufferMs: 100,
    conditions: ["AttackedAirborne"], effects: [], precision: null, priority: 2,
  };
  return { ...config, edges: [...config.edges, edge] };
}

/** Removes an edge an experiment added, with its window modifiers, overrides and interruptions. */
export function removeEdge(config: LabConfig, id: string): LabConfig {
  return {
    ...config,
    edges: config.edges.filter((edge) => edge.id !== id),
    windowModifiers: config.windowModifiers.filter((modifier) => modifier.edge !== id),
    interruptions: config.interruptions.filter((rule) => rule.interrupted !== `Edge(${id})`),
    overrides: config.overrides.filter((entry) => !(entry.setting.kind === "Window" && entry.setting.edge === id)),
  };
}

/**
 * The traversal sample (#413, Atlas 5.9.3): momentum through the air with 6 m/s² of air control,
 * slowed landings, and a Warden's aerial hit landing into an early Heroic Strike or Shield Bash that
 * waives the global cooldown. Ordinary actions compose into run → jump → aerial attack → landing →
 * follow-up without a named move.
 */
export function traversalSample(config: LabConfig): LabConfig {
  let next = setMovementRule(config, "Airborne", "Active", { steeringPercent: 100, momentumPercent: 100, maxAccelerationCms2: 600 });
  next = setMovementRule(next, "Landing", "Recovery", { steeringPercent: 50, momentumPercent: 0, maxAccelerationCms2: null });
  for (const id of ["LandingToMelee", "LandingToShieldBash"]) {
    next = addLandingEdge(next, id);
    next = {
      ...next,
      edges: next.edges.map((edge) => edge.id === id
        ? { ...edge, conditions: ["Warden", "AttackedAirborne"], effects: [{ kind: "IgnoreGlobalCooldown" }] }
        : edge),
    };
  }
  return next;
}

/** The water rules (#413): what the swimming sample refuses and holds while the character swims. */
export const SWIMMING_RULES: readonly { id: string; input: string; effect: "Refuse" | "Hold" }[] = [
  { id: "SwimmingRefuses", input: "WardenMelee", effect: "Refuse" },
  { id: "SwimmingRefuses", input: "ShieldBash", effect: "Refuse" },
  { id: "SwimmingHolds", input: "Ability", effect: "Hold" },
];

/**
 * The swimming sample (#413): in deep water (Stillwater Lake, east of the lab spot) melee and Shield
 * Bash are refused, other abilities wait until the swimmer is ashore, and the swim glides: full
 * momentum with 4 m/s² of steering. Production keeps allowing attacks while swimming.
 */
export function swimmingSample(config: LabConfig): LabConfig {
  const next = setMovementRule(config, "Swim", "Active", { steeringPercent: 100, momentumPercent: 100, maxAccelerationCms2: 400 });
  const kept = next.rules.filter((rule) => rule.during !== "Swim");
  const water = SWIMMING_RULES.map(({ id, input, effect }) => ({ id, during: "Swim", input, conditions: [], effect }));
  return { ...next, rules: [...kept, ...water] };
}

/**
 * The climbing sample (#490): while the character holds a climbable face (jump at a crag, palisade or
 * ladder) abilities wait until it is off the face again. Production keeps allowing attacks while climbing.
 */
export function climbingSample(config: LabConfig): LabConfig {
  const kept = config.rules.filter((rule) => rule.during !== "Climb");
  const hold = { id: "ClimbingHolds", during: "Climb", input: "Ability", conditions: [], effect: "Hold" as const };
  return { ...config, rules: [...kept, hold] };
}

/**
 * The rolling sample (#516): abilities pressed during a dodge roll (Shift) wait until it ends instead of
 * being cancelled by it. Production keeps the roll movement only: a cast started mid-roll is cancelled.
 */
export function rollingSample(config: LabConfig): LabConfig {
  const kept = config.rules.filter((rule) => rule.during !== "Roll");
  const hold = { id: "RollingHolds", during: "Roll", input: "Ability", conditions: [], effect: "Hold" as const };
  return { ...config, rules: [...kept, hold] };
}

/** `actionLabCheck`: the configuration's identity, whether it equals production, or why it is invalid. */
export type ConfigCheck = { identity: string | null; baseline: boolean; error: string | null };

/** One recorded command: its command-wire bytes in hex, applied before tick `tick + 1`. */
export type LabCommand = { tick: number; bytes: string };

/**
 * Whether a recorded command is movement (Move or Rise). A recorded combo holds hundreds of them,
 * so the timeline hides them unless asked (#382); they are still replayed.
 */
export function isMovement(command: LabCommand): boolean {
  const bytes = (command.bytes.match(/../g) ?? []).map((pair) => parseInt(pair, 16));
  return isMovementCommand(Uint8Array.from(bytes));
}

/** A named, exportable lab scenario. */
export type LabScenario = {
  format: typeof SCENARIO_FORMAT;
  name: string;
  setup: LabSetup;
  seed: string;
  contentRevision: string;
  protocolId: number;
  ticks: number;
  commands: LabCommand[];
  /** The rules the recording ran and its final canonical digest, to check a replay reproduces it. */
  recorded: { config: string; digest: string } | null;
};

/** An edge whose source runs: its status (condition) and eligible window in absolute ticks (#383). */
export type LabEdgeTrace = {
  edge: string; status: string; anchorTick: number | null; bufferFrom: number | null; openFrom: number | null;
  openUntil: number | null; precision: [number, number] | null;
};
/** How the zone arbitrated a command's graph input (Atlas 5.7.4). */
export type LabDecision = {
  input: string; outcome: string; candidates: { candidate: string; verdict: string }[]; precise: boolean;
  /** Ticks of lateness the zone made up for, and whether they kept the winning edge open (#415). */
  late: number; compensated: boolean;
};
export type LabTick = {
  tick: number; regions: string[]; events: string[]; health: number; resource: number | null;
  /** Body centre in cm and horizontal speed in cm/s after the tick's physics step (#413). */
  position: [number, number, number]; speed: number;
  digest: string; edges: LabEdgeTrace[];
};
export type LabCommandResult = {
  index: number; tick: number; label: string; refused: string[]; effectTick: number | null; delayTicks: number | null;
  /** Under the run's simulated network (#415): when the input reached the zone, the tick it was made against, its lateness. */
  arrivedTick: number | null; observedTick: number | null; lateTicks: number;
  /** How the zone arbitrated the input on arrival, for inputs the action graph judges. */
  decision: LabDecision | null;
};
/** An explained diagnostic (#383, Atlas 5.6.6): never a claim that a configuration is better. */
export type LabFinding = {
  kind: "unreachableTransition" | "expiredBuffer" | "inputLag" | "arbitrationConflict" | "cancelLoop" | "comboRegression";
  severity: "warning" | "note";
  tick: number;
  summary: string;
  trace: {
    tick: number; regions: string[]; pending: string[]; edge: LabEdgeTrace | null; sourceEvents: string[];
    decision: LabDecision | null; command: number | null; divergesAt: number | null;
  };
};

/** A simulated network between the scenario's player and the lab world (#415). */
export type LabNetwork = { roundTripMs: number; jitterMs: number; compensate: boolean };

/** Illustrative latency presets, not production thresholds (Atlas 5.9.6). */
export const NETWORK_PRESETS: readonly { name: string; roundTripMs: number; jitterMs: number }[] = [
  { name: "Local (0 ms)", roundTripMs: 0, jitterMs: 0 },
  { name: "50 ms ±10", roundTripMs: 50, jitterMs: 10 },
  { name: "100 ms ±20", roundTripMs: 100, jitterMs: 20 },
  { name: "200 ms ±40", roundTripMs: 200, jitterMs: 40 },
];

export const LOCAL_NETWORK: LabNetwork = { roundTripMs: 0, jitterMs: 0, compensate: true };

/** A command's arrival and arbitration under its run's network, in one short line. */
export function arrivalText(result: LabCommandResult): string {
  if (result.arrivedTick === null) { return "never arrived"; }
  const travel = result.arrivedTick - result.tick;
  const parts = [travel === 0 ? "on time" : `+${travel} t`];
  if (result.observedTick !== null) { parts.push(`late ${result.lateTicks} t`); }
  const arbitration = result.decision;
  if (arbitration) {
    parts.push(arbitration.outcome);
    if (arbitration.compensated) { parts.push("kept open by lateness"); }
    if (arbitration.precise) { parts.push("precise"); }
  }
  return parts.join(" · ");
}
export type LabRun = {
  config: string; configName: string; baseline: boolean; contentRevision: string; contentChanged: boolean;
  reproducesRecording: boolean | null;
  ticks: LabTick[]; commands: LabCommandResult[];
  summary: {
    damageDealt: number; damageTaken: number; refusals: number; kills: number;
    finalHealth: number; finalResource: number | null; finalDigest: string;
  };
  findings: LabFinding[];
};
export type LabComparison = {
  a: LabRun; b: LabRun; divergesAt: number | null; differences: { tick: number; a: LabTick; b: LabTick }[];
  /** Combos that succeeded under A and fail under B. */
  regressions: LabFinding[];
};

export function hex(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** The commands in application order; commands of one tick keep their order. */
function ordered(commands: readonly LabCommand[]): LabCommand[] {
  return commands.map((command, index) => ({ command, index }))
    .sort((left, right) => left.command.tick - right.command.tick || left.index - right.index)
    .map(({ command }) => command);
}

function edited(scenario: LabScenario, commands: LabCommand[]): LabScenario {
  // An edited scenario no longer is the recording.
  return { ...scenario, commands: ordered(commands), recorded: null };
}

const clampTick = (scenario: LabScenario, tick: number) => Math.min(Math.max(Math.round(tick), 0), Math.max(scenario.ticks - 1, 0));

/** Moves command `index` to `tick` (clamped to the scenario). */
export function moveCommand(scenario: LabScenario, index: number, tick: number): LabScenario {
  return edited(scenario, scenario.commands.map((command, at) => at === index ? { ...command, tick: clampTick(scenario, tick) } : command));
}

/** Removes command `index`. */
export function removeCommand(scenario: LabScenario, index: number): LabScenario {
  return edited(scenario, scenario.commands.filter((_, at) => at !== index));
}

/** Inserts a copy of command `index` at `tick` (clamped to the scenario). */
export function insertCopy(scenario: LabScenario, index: number, tick: number): LabScenario {
  const source = scenario.commands[index];
  if (!source) { return scenario; }
  return edited(scenario, [...scenario.commands, { ...source, tick: clampTick(scenario, tick) }]);
}

/** Changes how long the scenario lasts; commands past the end move to its last tick. */
export function setDuration(scenario: LabScenario, ticks: number): LabScenario {
  const next = { ...scenario, ticks: Math.max(1, Math.round(ticks)) };
  return edited(next, next.commands.map((command) => ({ ...command, tick: clampTick(next, command.tick) })));
}

/** A deep copy of `config` named `name`. */
export function copyConfig(config: LabConfig, name: string): LabConfig {
  return { ...structuredClone(config), name };
}

/** The class override of a rule's `applies`, as the editor's checkbox shows it. */
export function ruleApplies(config: LabConfig, rule: string, className: string): boolean {
  const entry = config.overrides.find((value) => value.scope.kind === "Class" && value.scope.class === className
    && value.setting.kind === "Rule" && value.setting.rule === rule);
  return entry?.setting.kind === "Rule" ? entry.setting.applies : true;
}

/** Sets whether `rule` applies to `className`; applying again removes the override. */
export function setRuleApplies(config: LabConfig, rule: string, className: string, applies: boolean): LabConfig {
  const overrides = config.overrides.filter((value) => !(value.scope.kind === "Class" && value.scope.class === className
    && value.setting.kind === "Rule" && value.setting.rule === rule));
  if (!applies) { overrides.push({ scope: { kind: "Class", class: className }, setting: { kind: "Rule", rule, applies: false } }); }
  return { ...config, overrides };
}

/** A class recovery override of `action` in ms, or null for the shared value. */
export function recoveryOverride(config: LabConfig, action: string, className: string): number | null {
  const entry = config.overrides.find((value) => value.scope.kind === "Class" && value.scope.class === className
    && value.setting.kind === "Recovery" && value.setting.action === action);
  return entry?.setting.kind === "Recovery" ? entry.setting.ms : null;
}

/** Sets or (with null) clears the class recovery override of `action`. */
export function setRecoveryOverride(config: LabConfig, action: string, className: string, ms: number | null): LabConfig {
  const overrides = config.overrides.filter((value) => !(value.scope.kind === "Class" && value.scope.class === className
    && value.setting.kind === "Recovery" && value.setting.action === action));
  if (ms !== null) { overrides.push({ scope: { kind: "Class", class: className }, setting: { kind: "Recovery", action, ms } }); }
  return { ...config, overrides };
}

/** A scenario, its A/B configurations and their names, as one exportable file. */
export type LabBundle = { format: typeof BUNDLE_FORMAT; scenario: LabScenario | null; a: LabConfig; b: LabConfig };

export function exportBundle(bundle: Omit<LabBundle, "format">): string {
  return JSON.stringify({ format: BUNDLE_FORMAT, ...bundle }, null, 2);
}

export const PROMOTION_FORMAT = "mmorpg-action-lab-promotion";
export const RECORD_FORMAT = "mmorpg-action-lab-promotion-record";

/** A saved, validated configuration version: the `version`-th distinct rules saved under `name`. */
export type LabConfigVersion = { name: string; version: number; identity: string; config: LabConfig };

/**
 * What `cargo run -p mmorpg-wasm --example action-lab -- promote` turns into a reviewable change (#383, Atlas 5.6.7): an
 * explicitly chosen saved version, the production identity it was compared against and its evidence.
 */
export type LabPromotion = {
  format: typeof PROMOTION_FORMAT; version: number; production: string; config: LabConfig; scenario: LabScenario; note: string;
};

/** A checked-in promotion record: the replaced and the promoted rules and their scenario. */
export type LabPromotionRecord = {
  format: typeof RECORD_FORMAT; name: string; version: number; identity: string; note: string;
  previous: LabConfig; promoted: LabConfig; scenario: LabScenario; divergesAt: number | null;
};

export function promotionRequest(version: LabConfigVersion, production: string, scenario: LabScenario, note: string): LabPromotion {
  return { format: PROMOTION_FORMAT, version: version.version, production, config: { ...version.config, name: version.name }, scenario, note };
}

/** The checkout command that turns an exported promotion into a branch-ready change. */
export function promoteCommand(file: string): string {
  return `cargo run -p mmorpg-wasm --locked --example action-lab -- promote ${file}`;
}

export function promotionFileName(version: LabConfigVersion): string {
  const slug = version.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return `promote-${slug}-v${version.version}.json`;
}

/** Reads an exported bundle, a promotion record (previous as A, promoted as B), a lone scenario or configuration. */
export function importFile(text: string): Partial<Omit<LabBundle, "format">> {
  const value = JSON.parse(text) as { format?: string };
  switch (value.format) {
    case BUNDLE_FORMAT: {
      const bundle = value as LabBundle;
      return { scenario: bundle.scenario, a: bundle.a, b: bundle.b };
    }
    case RECORD_FORMAT: {
      const record = value as LabPromotionRecord;
      return { scenario: record.scenario, a: record.previous, b: record.promoted };
    }
    case SCENARIO_FORMAT: return { scenario: value as LabScenario };
    case CONFIG_FORMAT: return { b: value as LabConfig };
    default: throw new Error(`"${value.format}" is not an Action Lab scenario or configuration.`);
  }
}

/** Named scenarios and configurations saved in this browser (never characters or production rules). */
export class LabLibrary {
  static readonly KEY = "mmorpg.action-lab.library.v1";
  readonly #storage: Storage | null;

  constructor(storage: Storage | null) {
    this.#storage = storage;
  }

  #read(): { scenarios: LabScenario[]; configs: LabConfigVersion[] } {
    try {
      const text = this.#storage?.getItem(LabLibrary.KEY);
      if (text) {
        const library = JSON.parse(text) as { scenarios: LabScenario[]; configs: LabConfigVersion[] };
        // Configurations saved before versions (#383) carry no version: dropped.
        return { scenarios: library.scenarios, configs: library.configs.filter((entry) => typeof entry.version === "number") };
      }
    } catch {
      // Unreadable storage: an empty library.
    }
    return { scenarios: [], configs: [] };
  }

  #write(library: { scenarios: LabScenario[]; configs: LabConfigVersion[] }): void {
    try { this.#storage?.setItem(LabLibrary.KEY, JSON.stringify(library)); } catch { /* storage denied: kept for the visit only */ }
  }

  get scenarios(): LabScenario[] { return this.#read().scenarios; }
  /** Every saved configuration version, by name and then version. */
  get configs(): LabConfigVersion[] {
    return this.#read().configs.sort((left, right) => left.name.localeCompare(right.name) || left.version - right.version);
  }

  /** Saves (or replaces by name) a scenario. */
  saveScenario(scenario: LabScenario): void {
    const library = this.#read();
    library.scenarios = [...library.scenarios.filter((entry) => entry.name !== scenario.name), scenario];
    this.#write(library);
  }

  /**
   * Saves a validated configuration (its `identity` from `actionLabCheck`) as the next version of its name.
   * Saving the rules of the name's latest version again keeps that version. Versions are never replaced.
   */
  saveConfig(config: LabConfig, identity: string): LabConfigVersion {
    const library = this.#read();
    const versions = library.configs.filter((entry) => entry.name === config.name);
    const latest = versions.reduce<LabConfigVersion | null>((found, entry) => !found || entry.version > found.version ? entry : found, null);
    if (latest?.identity === identity) { return latest; }
    const saved = { name: config.name, version: (latest?.version ?? 0) + 1, identity, config: structuredClone(config) };
    library.configs = [...library.configs, saved];
    this.#write(library);
    return saved;
  }

  removeScenario(name: string): void {
    const library = this.#read();
    library.scenarios = library.scenarios.filter((entry) => entry.name !== name);
    this.#write(library);
  }
}

/** The input-to-effect delays of a run's action commands, in ticks, by command index. */
export function delays(run: LabRun): Map<number, number> {
  return new Map(run.commands.flatMap((command) => command.delayTicks === null ? [] : [[command.index, command.delayTicks] as const]));
}

/** A short form of an event's or command's `Debug` text for tables. */
export function shortLabel(text: string): string {
  return text.replace(/\b(Player|Creature|Npc|Minion)\((?:\w+\()?(\d+)\)?\)/g, "$1 $2").replace(/\s+/g, " ");
}
