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
};

/** The action phases movement can be authored for (`mmorpg_core::action::phases`, reactions excluded). */
export const MOVEMENT_PHASES: readonly { action: string; phase: string }[] = [
  { action: "Airborne", phase: "Active" },
  { action: "Landing", phase: "Recovery" },
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

/** `actionLabCheck`: the configuration's identity, whether it equals production, or why it is invalid. */
export type ConfigCheck = { identity: string | null; baseline: boolean; error: string | null };

/** One recorded command: its command-wire bytes in hex, applied before tick `tick + 1`. */
export type LabCommand = { tick: number; bytes: string };

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

export type LabTick = {
  tick: number; regions: string[]; events: string[]; health: number; resource: number | null;
  /** Body centre in cm and horizontal speed in cm/s after the tick's physics step (#413). */
  position: [number, number, number]; speed: number;
  digest: string;
};
export type LabCommandResult = {
  index: number; tick: number; label: string; refused: string[]; effectTick: number | null; delayTicks: number | null;
};
export type LabRun = {
  config: string; configName: string; baseline: boolean; contentRevision: string; contentChanged: boolean;
  reproducesRecording: boolean | null;
  ticks: LabTick[]; commands: LabCommandResult[];
  summary: {
    damageDealt: number; damageTaken: number; refusals: number; kills: number;
    finalHealth: number; finalResource: number | null; finalDigest: string;
  };
};
export type LabComparison = { a: LabRun; b: LabRun; divergesAt: number | null; differences: { tick: number; a: LabTick; b: LabTick }[] };

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

/** Reads an exported bundle, a lone scenario or a lone configuration; the zone validates the content. */
export function importFile(text: string): Partial<Omit<LabBundle, "format">> {
  const value = JSON.parse(text) as { format?: string };
  switch (value.format) {
    case BUNDLE_FORMAT: {
      const bundle = value as LabBundle;
      return { scenario: bundle.scenario, a: bundle.a, b: bundle.b };
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

  #read(): { scenarios: LabScenario[]; configs: LabConfig[] } {
    try {
      const text = this.#storage?.getItem(LabLibrary.KEY);
      if (text) { return JSON.parse(text) as { scenarios: LabScenario[]; configs: LabConfig[] }; }
    } catch {
      // Unreadable storage: an empty library.
    }
    return { scenarios: [], configs: [] };
  }

  #write(library: { scenarios: LabScenario[]; configs: LabConfig[] }): void {
    try { this.#storage?.setItem(LabLibrary.KEY, JSON.stringify(library)); } catch { /* storage denied: kept for the visit only */ }
  }

  get scenarios(): LabScenario[] { return this.#read().scenarios; }
  get configs(): LabConfig[] { return this.#read().configs; }

  /** Saves (or replaces by name) a scenario. */
  saveScenario(scenario: LabScenario): void {
    const library = this.#read();
    library.scenarios = [...library.scenarios.filter((entry) => entry.name !== scenario.name), scenario];
    this.#write(library);
  }

  /** Saves (or replaces by name) a configuration. */
  saveConfig(config: LabConfig): void {
    const library = this.#read();
    library.configs = [...library.configs.filter((entry) => entry.name !== config.name), config];
    this.#write(library);
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
