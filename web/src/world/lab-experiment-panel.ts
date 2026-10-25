/**
 * The Action Lab's experiments section (#382): record the arena's inputs as a named scenario, edit
 * its command timeline tick by tick, edit two experimental action configurations (A and B), replay
 * the scenario in the arena under either, and compare both runs side by side with the tick their
 * canonical state diverges at. Every run is the shared simulation's (`LabExperiments`); nothing
 * here decides an outcome, and nothing reaches production rules or stored characters.
 */
import { PROTOCOL_ID } from "../protocol-id";
import { ticksFromMs } from "../replication";
import { element, type LabSetup } from "./action-lab";
import {
  addLandingEdge, copyConfig, exportBundle, importFile, insertCopy, LANDING_EDGES, MOVEMENT_PHASES, moveCommand, movementRule,
  recoveryOverride, removeCommand, removeEdge, ruleApplies, setDuration, setMovementRule, setRecoveryOverride, setRuleApplies,
  shortLabel, traversalSample, SCENARIO_FORMAT,
  type ConfigCheck, type LabComparison, type LabConfig, type LabConfigEdge, type LabEffect, type LabLibrary, type LabRun, type LabScenario,
} from "./lab-experiment";
import type { LabExperiments } from "./wasm-runtime";

export type ExperimentControls = {
  /** What the arena recorded since its fresh start. */
  recording(): { commands: readonly { tick: number; bytes: string }[]; ticks: number; digest: string | null };
  /** The recording's size, cheaply. */
  recordingStatus(): { commands: number; ticks: number };
  replaying(): boolean;
  /** Restarts the arena fresh with `setup` under `config`, the player in control. */
  playLive(setup: LabSetup, config: LabConfig): Promise<void>;
  /** Restarts the arena replaying `scenario` under `config`, pausing at `pauseAt` when given. */
  replay(scenario: LabScenario, config: LabConfig, pauseAt: number | null): Promise<void>;
  /** Ends a replay: the player takes over where it is. */
  takeOver(): void;
};

type Slot = "a" | "b";
const CONDITIONS = ["PositiveHealthDamage", "TargetAlive", "Alive", "Warden", "AttackedAirborne", "Airborne", "Stunned"];
const FLAG_EFFECTS = ["TargetConfirmed", "IgnoreGlobalCooldown", "KeepOpening"] as const;
/** The recoveries the zone reads per class (Atlas 5.7.6). */
const OVERRIDABLE_RECOVERIES = ["DirectedSlash", "GlobalCooldown"];

function download(name: string, text: string): void {
  const link = element("a", { href: URL.createObjectURL(new Blob([text], { type: "application/json" })), download: name });
  link.click();
  URL.revokeObjectURL(link.href);
}

export class ExperimentPanel {
  readonly root = element("div", { className: "lab-experiments" });
  readonly #runtime: LabExperiments;
  readonly #library: LabLibrary;
  readonly #controls: ExperimentControls;
  readonly #seed: bigint;
  readonly #recordingStatus = element("p", { className: "lab-muted" });
  readonly #scenarioBox = element("div");
  readonly #configBox = element("div");
  readonly #compareBox = element("div", { className: "lab-compare" });
  #setup: LabSetup;
  #scenario: LabScenario | null = null;
  #configs: Record<Slot, LabConfig>;
  #checks: Record<Slot, ConfigCheck>;
  #editing: Slot = "b";
  /** The configuration the arena runs now. */
  #live: Slot | null = null;
  #comparison: LabComparison | null = null;
  #error: string | null = null;
  #refreshTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(runtime: LabExperiments, library: LabLibrary, setup: LabSetup, seed: bigint, controls: ExperimentControls) {
    this.#runtime = runtime;
    this.#library = library;
    this.#setup = setup;
    this.#seed = seed;
    this.#controls = controls;
    const baseline = runtime.baseline();
    this.#configs = { a: baseline, b: copyConfig(baseline, "Experiment B") };
    this.#checks = { a: runtime.check(this.#configs.a), b: runtime.check(this.#configs.b) };
    this.root.append(this.#recordingBar(), this.#scenarioBox, this.#configBox, this.#compareBox);
    this.#render();
  }

  /** Refreshes the live recording line; cheap enough for every frame. */
  update(): void {
    const recording = this.#controls.recordingStatus();
    const live = this.#live === null ? "production rules" : `${this.#live.toUpperCase()} (${this.#configs[this.#live].name})`;
    const text = this.#controls.replaying()
      ? `Replaying under ${live} · tick ${recording.ticks}`
      : `Recording ${recording.commands} commands over ${recording.ticks} ticks under ${live}`;
    if (this.#recordingStatus.textContent !== text) { this.#recordingStatus.textContent = text; }
  }

  #recordingBar(): HTMLElement {
    const name = element("input", { type: "text", value: "Combo", title: "Scenario name", className: "lab-name" });
    const save = element("button", { type: "button", title: "Keep the arena's inputs since its fresh start as the scenario" }, "Save recording");
    const restart = element("button", { type: "button", title: "Restart the arena fresh: a new recording" }, "Record anew");
    const takeOver = element("button", { type: "button", title: "Stop replaying and play on from here" }, "Take over");
    save.addEventListener("click", () => {
      const recording = this.#controls.recording();
      const config = this.#live === null ? null : this.#checks[this.#live];
      const identity = config?.identity ?? this.#runtime.check(this.#runtime.baseline()).identity;
      this.#setScenario({
        format: SCENARIO_FORMAT, name: name.value.trim() || "Scenario", setup: this.#setup, seed: String(this.#seed),
        contentRevision: this.#runtime.contentRevision(), protocolId: PROTOCOL_ID, ticks: Math.max(recording.ticks, 1),
        commands: [...recording.commands],
        recorded: recording.digest && identity ? { config: identity, digest: recording.digest } : null,
      });
      this.#library.saveScenario(this.#scenario!);
    });
    restart.addEventListener("click", () => void this.#playLive(this.#live ?? "a"));
    takeOver.addEventListener("click", () => this.#controls.takeOver());
    return element("div", {}, element("div", { className: "lab-playback" }, name, save, restart, takeOver), this.#recordingStatus);
  }

  async #playLive(slot: Slot): Promise<void> {
    if (!this.#valid(slot)) { return; }
    this.#live = slot;
    await this.#controls.playLive(this.#setup, this.#configs[slot]);
    this.#render();
  }

  async #replay(slot: Slot, pauseAt: number | null): Promise<void> {
    const scenario = this.#scenario;
    if (!scenario || !this.#valid(slot)) { return; }
    this.#live = slot;
    this.#setup = scenario.setup;
    await this.#controls.replay(scenario, this.#configs[slot], pauseAt);
    this.#render();
  }

  #valid(slot: Slot): boolean {
    const check = this.#checks[slot];
    if (check.error) { this.#error = `${slot.toUpperCase()}: ${check.error}`; this.#render(); }
    return check.error === null;
  }

  #setScenario(scenario: LabScenario | null): void {
    this.#scenario = scenario;
    this.#scheduleCompare();
  }

  #setConfig(slot: Slot, config: LabConfig): void {
    this.#configs[slot] = config;
    this.#checks[slot] = this.#runtime.check(config);
    this.#scheduleCompare();
  }

  /** Re-runs the comparison shortly after an edit (and renders now). */
  #scheduleCompare(): void {
    this.#render();
    if (this.#refreshTimer !== null) { clearTimeout(this.#refreshTimer); }
    this.#refreshTimer = setTimeout(() => {
      this.#refreshTimer = null;
      this.#compare();
    }, 120);
  }

  #compare(): void {
    this.#comparison = null;
    this.#error = null;
    const scenario = this.#scenario;
    if (scenario && !this.#checks.a.error && !this.#checks.b.error) {
      try {
        this.#comparison = this.#runtime.compare(scenario, this.#configs.a, this.#configs.b);
      } catch (error) {
        this.#error = error instanceof Error ? error.message : String(error);
      }
    }
    this.#render();
  }

  #render(): void {
    this.#renderScenario();
    this.#renderConfig();
    this.#renderComparison();
  }

  #renderScenario(): void {
    const saved = element("select", { title: "Saved scenarios" });
    saved.append(element("option", { value: "" }, "saved scenarios…"));
    for (const entry of this.#library.scenarios) { saved.append(element("option", { value: entry.name }, entry.name)); }
    saved.addEventListener("change", () => {
      const found = this.#library.scenarios.find((entry) => entry.name === saved.value);
      if (found) { this.#setScenario(found); }
    });
    const exportButton = element("button", { type: "button" }, "Export");
    exportButton.addEventListener("click", () => download(`${this.#scenario?.name ?? "action-lab"}.json`,
      exportBundle({ scenario: this.#scenario, a: this.#configs.a, b: this.#configs.b })));
    const file = element("input", { type: "file", accept: "application/json,.json", title: "Import a scenario or configuration" });
    file.addEventListener("change", () => void file.files?.[0]?.text().then((text) => {
      try {
        const imported = importFile(text);
        if (imported.a) { this.#setConfig("a", imported.a); }
        if (imported.b) { this.#setConfig("b", imported.b); }
        if (imported.scenario !== undefined) { this.#setScenario(imported.scenario); }
      } catch (error) {
        this.#error = error instanceof Error ? error.message : String(error);
        this.#render();
      }
    }));
    const bar = element("div", { className: "lab-playback" }, saved, exportButton, element("label", { className: "lab-import" }, "Import ", file));
    const scenario = this.#scenario;
    if (!scenario) {
      this.#scenarioBox.replaceChildren(bar, element("p", { className: "lab-muted" }, "Play a combo in the arena, then Save recording."));
      return;
    }
    const duration = element("input", { type: "number", min: "1", step: "1", value: String(scenario.ticks), title: "Ticks the scenario lasts" });
    duration.addEventListener("change", () => this.#setScenario(setDuration(scenario, Number(duration.value))));
    const results = this.#comparison?.a.commands ?? [];
    const delaysB = new Map((this.#comparison?.b.commands ?? []).map((command) => [command.index, command]));
    const rows = scenario.commands.map((command, index) => {
      const result = results.find((entry) => entry.index === index);
      const other = delaysB.get(index);
      const tick = element("input", { type: "number", min: "0", step: "1", value: String(command.tick), title: "Applied before this tick + 1" });
      tick.addEventListener("change", () => this.#setScenario(moveCommand(scenario, index, Number(tick.value))));
      const button = (text: string, title: string, action: () => LabScenario) => {
        const node = element("button", { type: "button", title }, text);
        node.addEventListener("click", () => this.#setScenario(action()));
        return node;
      };
      const delay = (entry: typeof result) => entry?.delayTicks === null || !entry ? "—" : `${entry.delayTicks} t`;
      return element("tr", {},
        element("td", {}, tick),
        element("td", { className: "lab-actions" },
          button("−", "One tick earlier", () => moveCommand(scenario, index, command.tick - 1)),
          button("+", "One tick later", () => moveCommand(scenario, index, command.tick + 1)),
          button("⧉", "Insert a copy one tick later", () => insertCopy(scenario, index, command.tick + 1)),
          button("✕", "Remove", () => removeCommand(scenario, index))),
        element("td", { className: "lab-label", title: result?.label ?? command.bytes }, shortLabel(result?.label ?? command.bytes)),
        element("td", { title: "Input-to-effect delay under A / B" }, `${delay(result)} / ${delay(other)}`),
        element("td", { className: result?.refused.length ? "lab-refused" : "" }, result?.refused.length ? `refused ${result.refused.join(", ")}` : "ok"));
    });
    const recorded = scenario.recorded
      ? `recorded under ${scenario.recorded.config}`
      : "edited (no longer the recording)";
    this.#scenarioBox.replaceChildren(bar,
      element("p", { className: "lab-muted" },
        `${scenario.name}: ${scenario.setup.className} ${scenario.setup.level}, seed ${scenario.seed}, content ${scenario.contentRevision}, ${recorded}.`),
      element("label", {}, "Duration (ticks) ", duration),
      element("div", { className: "lab-scroll" }, element("table", { className: "lab-table" },
        element("thead", {}, element("tr", {}, ...["Tick", "Edit", "Command", "Delay A/B", "Result A"].map((text) => element("th", {}, text)))),
        element("tbody", {}, ...rows))));
  }

  #renderConfig(): void {
    const slot = this.#editing;
    const config = this.#configs[slot];
    const check = this.#checks[slot];
    const tabs = (["a", "b"] as const).map((entry) => {
      const node = element("button", { type: "button", className: entry === slot ? "selected" : "" }, `${entry.toUpperCase()}: ${this.#configs[entry].name}`);
      node.addEventListener("click", () => { this.#editing = entry; this.#render(); });
      return node;
    });
    const name = element("input", { type: "text", value: config.name, className: "lab-name", title: "Configuration name" });
    name.addEventListener("change", () => this.#setConfig(slot, { ...config, name: name.value }));
    const action = (text: string, title: string, run: () => void) => {
      const node = element("button", { type: "button", title }, text);
      node.addEventListener("click", run);
      return node;
    };
    const savedConfigs = element("select", { title: "Saved configurations" });
    savedConfigs.append(element("option", { value: "" }, "saved configurations…"));
    for (const entry of this.#library.configs) { savedConfigs.append(element("option", { value: entry.name }, entry.name)); }
    savedConfigs.addEventListener("change", () => {
      const found = this.#library.configs.find((entry) => entry.name === savedConfigs.value);
      if (found) { this.#setConfig(slot, found); }
    });
    const status = check.error
      ? element("p", { className: "lab-refused" }, `Invalid: ${check.error}`)
      : element("p", { className: "lab-muted" }, `${check.identity}${check.baseline ? " · equals production" : " · experimental, lab only"}`);
    const raw = element("textarea", { className: "lab-json", rows: 6, spellcheck: false, value: JSON.stringify(config, null, 1) });
    const landing = LANDING_EDGES.filter((edge) => !config.edges.some((entry) => entry.id === edge.id))
      .map((edge) => action(`+ ${edge.id}`, `Add an edge out of the landing recovery answering ${edge.input}`,
        () => this.#setConfig(slot, addLandingEdge(config, edge.id))));
    const body = element("div", { className: "lab-config" },
      ...config.edges.map((edge, index) => this.#edgeEditor(slot, edge, index)),
      ...(landing.length ? [element("div", { className: "lab-playback" }, ...landing)] : []),
      this.#movementEditor(slot),
      ...config.windowModifiers.map((modifier, index) => this.#numberField(`Modifier ${modifier.edge} if ${modifier.conditions.join(", ")}: actionable Δ ms`,
        modifier.actionableDeltaMs, (value) => this.#setConfig(slot, {
          ...config, windowModifiers: config.windowModifiers.map((entry, at) => at === index ? { ...entry, actionableDeltaMs: value } : entry),
        }), -100_000)),
      element("h4", {}, `${this.#setup.className} overrides`),
      ...[...new Set(config.rules.map((rule) => rule.id))].map((id) => {
        const box = element("input", { type: "checkbox", checked: ruleApplies(config, id, this.#setup.className) });
        box.addEventListener("change", () => this.#setConfig(slot, setRuleApplies(config, id, this.#setup.className, box.checked)));
        const what = config.rules.filter((rule) => rule.id === id).map((rule) => `${rule.effect} ${rule.input} during ${rule.during}`);
        return element("label", { className: "lab-check" }, box, `${id} applies (${what.join("; ")})`);
      }),
      ...OVERRIDABLE_RECOVERIES.map((id) => {
        const value = recoveryOverride(config, id, this.#setup.className);
        const input = element("input", { type: "number", min: "0", step: "1", placeholder: "shared", value: value === null ? "" : String(value) });
        input.addEventListener("change", () => this.#setConfig(slot,
          setRecoveryOverride(config, id, this.#setup.className, input.value === "" ? null : Number(input.value))));
        return element("label", {}, `${id} recovery ms${value === null ? "" : ` (${ticksFromMs(value)} t)`} `, input);
      }),
      element("details", {}, element("summary", {}, "JSON (interruptions, other scopes)"), raw,
        action("Apply JSON", "Replace this configuration with the JSON", () => {
          try { this.#setConfig(slot, JSON.parse(raw.value) as LabConfig); } catch (error) { this.#error = String(error); this.#render(); }
        })));
    this.#configBox.replaceChildren(
      element("h3", {}, "Configurations"),
      element("div", { className: "lab-playback" }, ...tabs),
      element("div", { className: "lab-playback" }, name,
        action("Production", "Reset this configuration to the production rules", () => this.#setConfig(slot, copyConfig(this.#runtime.baseline(), config.name))),
        action("Traversal sample", "Momentum through the air, slowed landings and landing follow-ups after an aerial hit (#413)",
          () => this.#setConfig(slot, traversalSample(config))),
        action(slot === "a" ? "Copy to B" : "Copy to A", "Copy this configuration to the other slot", () => {
          const other = slot === "a" ? "b" : "a";
          this.#setConfig(other, copyConfig(config, `${config.name} copy`));
        }),
        action("Save", "Save this configuration in this browser", () => { this.#library.saveConfig(config); this.#render(); }),
        savedConfigs),
      status, body,
      element("div", { className: "lab-playback" },
        action(`Play live with ${slot.toUpperCase()}`, "Restart the arena under this configuration", () => void this.#playLive(slot))));
  }

  /** Per action phase: authored movement or free (Atlas 5.9.2); Rapier still resolves every contact. */
  #movementEditor(slot: Slot): HTMLElement {
    const config = this.#configs[slot];
    const rows = MOVEMENT_PHASES.map(({ action, phase }) => {
      const rule = movementRule(config, action, phase);
      const authored = element("input", { type: "checkbox", checked: rule !== null });
      authored.addEventListener("change", () => this.#setConfig(slot, setMovementRule(config, action, phase,
        authored.checked ? { steeringPercent: 100, momentumPercent: 100, maxAccelerationCms2: 600 } : null)));
      const fields = rule ? [
        this.#numberField("steering %", rule.steeringPercent, (value) => this.#setConfig(slot,
          setMovementRule(config, action, phase, { ...rule, steeringPercent: value }))),
        this.#numberField("momentum %", rule.momentumPercent, (value) => this.#setConfig(slot,
          setMovementRule(config, action, phase, { ...rule, momentumPercent: value }))),
        (() => {
          const input = element("input", { type: "number", min: "1", step: "50", placeholder: "unlimited",
            value: rule.maxAccelerationCms2 === null ? "" : String(rule.maxAccelerationCms2) });
          input.addEventListener("change", () => this.#setConfig(slot, setMovementRule(config, action, phase,
            { ...rule, maxAccelerationCms2: input.value === "" ? null : Number(input.value) })));
          return element("label", {}, "max acceleration cm/s² ", input);
        })(),
      ] : [];
      return element("div", { className: "lab-flags" },
        element("label", { className: "lab-check" }, authored, `${action}·${phase}`), ...fields);
    });
    return element("fieldset", { className: "lab-edge-editor" },
      element("legend", {}, "Movement per action phase (free when unchecked)"),
      element("p", { className: "lab-muted" },
        "Velocity = intent × steering + last realized velocity × momentum × (1 − steering), changing at most the acceleration limit per second; running phases combine to the lowest steering, highest momentum and lowest limit."),
      ...rows);
  }

  #numberField(label: string, value: number, change: (value: number) => void, min = 0, ticks = false): HTMLElement {
    const input = element("input", { type: "number", min: String(min), step: "1", value: String(value) });
    input.addEventListener("change", () => change(Number(input.value)));
    return element("label", {}, `${label}${ticks ? ` (${ticksFromMs(value)} t)` : ""} `, input);
  }

  #effectsEditor(effects: readonly LabEffect[], change: (effects: LabEffect[]) => void): HTMLElement[] {
    const flags = FLAG_EFFECTS.map((kind) => {
      const box = element("input", { type: "checkbox", checked: effects.some((effect) => effect.kind === kind) });
      box.addEventListener("change", () => change(box.checked
        ? [...effects, { kind }]
        : effects.filter((effect) => effect.kind !== kind)));
      return element("label", { className: "lab-check" }, box, kind);
    });
    const recovery = effects.find((effect) => effect.kind === "Recovery");
    const refund = effects.find((effect) => effect.kind === "ResourceRefund");
    const impulse = effects.find((effect) => effect.kind === "Impulse");
    const launch = (label: string, value: number | null, axis: "forwardCms" | "upCms") => {
      const input = element("input", { type: "number", step: "50", placeholder: "none", value: value === null ? "" : String(value) });
      input.addEventListener("change", () => {
        const rest = effects.filter((effect) => effect.kind !== "Impulse");
        const current = impulse?.kind === "Impulse" ? impulse : { kind: "Impulse" as const, forwardCms: 0, upCms: 0 };
        const next = { ...current, [axis]: input.value === "" ? 0 : Number(input.value) };
        change(next.forwardCms === 0 && next.upCms === 0 ? rest : [...rest, next]);
      });
      return element("label", {}, `${label} `, input);
    };
    const optional = (label: string, value: number | null, make: (value: number) => LabEffect, kind: string) => {
      const input = element("input", { type: "number", min: "0", step: "1", placeholder: "none", value: value === null ? "" : String(value) });
      input.addEventListener("change", () => {
        const rest = effects.filter((effect) => effect.kind !== kind);
        change(input.value === "" ? rest : [...rest, make(Number(input.value))]);
      });
      return element("label", {}, `${label} `, input);
    };
    return [
      element("div", { className: "lab-flags" }, ...flags),
      optional(`Recovery ms${recovery?.kind === "Recovery" ? ` (${ticksFromMs(recovery.ms)} t)` : ""}`,
        recovery?.kind === "Recovery" ? recovery.ms : null, (value) => ({ kind: "Recovery", ms: value }), "Recovery"),
      optional("Resource refund", refund?.kind === "ResourceRefund" ? refund.amount : null, (value) => ({ kind: "ResourceRefund", amount: value }), "ResourceRefund"),
      launch("Impulse forward cm/s", impulse?.kind === "Impulse" ? impulse.forwardCms : null, "forwardCms"),
      launch("Impulse up cm/s", impulse?.kind === "Impulse" ? impulse.upCms : null, "upCms"),
    ];
  }

  #edgeEditor(slot: Slot, edge: LabConfigEdge, index: number): HTMLElement {
    const config = this.#configs[slot];
    const set = (patch: Partial<LabConfigEdge>) => this.#setConfig(slot, {
      ...config, edges: config.edges.map((entry, at) => at === index ? { ...entry, ...patch } : entry),
    });
    const conditions = CONDITIONS.map((condition) => {
      const box = element("input", { type: "checkbox", checked: edge.conditions.includes(condition) });
      box.addEventListener("change", () => set({ conditions: box.checked ? [...edge.conditions, condition] : edge.conditions.filter((entry) => entry !== condition) }));
      return element("label", { className: "lab-check" }, box, condition);
    });
    const precision = edge.precision;
    const precise = element("input", { type: "checkbox", checked: precision !== null });
    precise.addEventListener("change", () => set({ precision: precise.checked ? { afterOpenMs: 0, lengthMs: 50, benefits: [{ kind: "Recovery", ms: 800 }] } : null }));
    const remove = edge.from === "Landing" ? [(() => {
      const node = element("button", { type: "button", title: "Remove this experimental edge" }, "Remove");
      node.addEventListener("click", () => this.#setConfig(slot, removeEdge(config, edge.id)));
      return node;
    })()] : [];
    return element("fieldset", { className: "lab-edge-editor" },
      element("legend", {}, `${edge.id}: ${edge.from} —${edge.input}→ (anchor ${edge.anchor})`),
      ...remove,
      this.#numberField("Opens after ms", edge.openAfterMs, (value) => set({ openAfterMs: value }), 0, true),
      this.#numberField("Actionable ms", edge.actionableMs, (value) => set({ actionableMs: value }), 0, true),
      this.#numberField("Buffer ms", edge.bufferMs, (value) => set({ bufferMs: value }), 0, true),
      this.#numberField("Priority", edge.priority, (value) => set({ priority: value })),
      element("div", { className: "lab-flags" }, element("span", {}, "Conditions:"), ...conditions),
      element("div", {}, element("span", {}, "Effects:"), ...this.#effectsEditor(edge.effects, (effects) => set({ effects }))),
      element("label", { className: "lab-check" }, precise, "Precision window"),
      ...(precision ? [
        this.#numberField("Precise after opening ms", precision.afterOpenMs, (value) => set({ precision: { ...precision, afterOpenMs: value } }), 0, true),
        this.#numberField("Precise length ms", precision.lengthMs, (value) => set({ precision: { ...precision, lengthMs: value } }), 0, true),
        element("div", {}, element("span", {}, "Precise benefits:"),
          ...this.#effectsEditor(precision.benefits, (benefits) => set({ precision: { ...precision, benefits } }))),
      ] : []));
  }

  #renderComparison(): void {
    const action = (text: string, run: () => void, title = "") => {
      const node = element("button", { type: "button", title }, text);
      node.addEventListener("click", run);
      return node;
    };
    const scenario = this.#scenario;
    const header = element("div", { className: "lab-playback" },
      action("Replay A", () => void this.#replay("a", null), "Replay the scenario in the arena under A"),
      action("Replay B", () => void this.#replay("b", null), "Replay the scenario in the arena under B"),
      action("Compare", () => this.#compare(), "Run the scenario under A and B"));
    const error = this.#error ? [element("p", { className: "lab-refused" }, this.#error)] : [];
    const comparison = this.#comparison;
    if (!scenario || !comparison) {
      this.#compareBox.replaceChildren(element("h3", {}, "Compare A / B"), header, ...error,
        element("p", { className: "lab-muted" }, scenario ? "Fix the configurations to compare." : "Save or import a scenario to compare."));
      return;
    }
    const { a, b } = comparison;
    const row = (name: string, left: string, right: string) =>
      element("tr", { className: left === right ? "" : "lab-differs" }, element("th", {}, name), element("td", {}, left), element("td", {}, right));
    const averageDelay = (run: LabRun) => {
      const values = run.commands.flatMap((command) => command.delayTicks === null ? [] : [command.delayTicks]);
      return values.length ? `${(values.reduce((sum, value) => sum + value, 0) / values.length).toFixed(1)} t` : "—";
    };
    const reproduces = (run: LabRun) => run.reproducesRecording === null ? "other rules" : run.reproducesRecording ? "yes" : "NO";
    const summary = element("table", { className: "lab-table" },
      element("thead", {}, element("tr", {}, element("th", {}, ""), element("th", {}, `A: ${a.configName}`), element("th", {}, `B: ${b.configName}`))),
      element("tbody", {},
        row("Rules", `${a.config}${a.baseline ? " (production)" : ""}`, `${b.config}${b.baseline ? " (production)" : ""}`),
        row("Damage dealt", String(a.summary.damageDealt), String(b.summary.damageDealt)),
        row("Damage taken", String(a.summary.damageTaken), String(b.summary.damageTaken)),
        row("Kills", String(a.summary.kills), String(b.summary.kills)),
        row("Refusals", String(a.summary.refusals), String(b.summary.refusals)),
        row("Resource at end", String(a.summary.finalResource ?? "—"), String(b.summary.finalResource ?? "—")),
        row("Health at end", String(a.summary.finalHealth), String(b.summary.finalHealth)),
        row("Mean input→effect", averageDelay(a), averageDelay(b)),
        row("Reproduces recording", reproduces(a), reproduces(b))));
    const diverges = comparison.divergesAt === null
      ? "The runs never diverge: identical canonical state every tick."
      : `Canonical state diverges at tick ${comparison.divergesAt}.`;
    const content = a.contentChanged ? [element("p", { className: "lab-refused" }, `Content changed since recording (now ${a.contentRevision}).`)] : [];
    const differences = comparison.differences.map((difference) => {
      const cell = (tick: typeof difference.a, slot: Slot) => {
        const node = element("td", { title: [...tick.regions, ...tick.events].join("\n") },
          [tick.regions.filter(Boolean).join(" · "), ...tick.events.map(shortLabel)].filter(Boolean).join(" | ") || "idle",
          ` · hp ${tick.health}${tick.resource === null ? "" : ` · res ${tick.resource}`} · at ${tick.position.join(", ")} cm · ${tick.speed} cm/s`);
        node.addEventListener("click", () => void this.#replay(slot, difference.tick));
        return node;
      };
      return element("tr", {}, element("th", {}, String(difference.tick)), cell(difference.a, "a"), cell(difference.b, "b"));
    });
    this.#compareBox.replaceChildren(element("h3", {}, "Compare A / B"), header, ...error, ...content,
      element("p", { className: comparison.divergesAt === null ? "lab-muted" : "lab-diverges" }, diverges), summary,
      element("p", { className: "lab-muted" }, `${comparison.differences.length} ticks differ in actions, events, health, resource or position. Click a side to replay it paused there.`),
      element("div", { className: "lab-scroll" }, element("table", { className: "lab-table lab-diff" },
        element("thead", {}, element("tr", {}, element("th", {}, "Tick"), element("th", {}, "A"), element("th", {}, "B"))),
        element("tbody", {}, ...differences))));
  }
}
