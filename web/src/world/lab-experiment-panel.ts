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
  addLandingEdge, copyConfig, exportBundle, importFile, arrivalText, insertCopy, isMovement, LANDING_EDGES, LOCAL_NETWORK, MOVEMENT_PHASES, moveCommand,
  movementRule, NETWORK_PRESETS, promoteCommand, promotionFileName, promotionRequest, recoveryOverride, removeAuthored, removeCommand, removeEdge,
  ruleApplies, setAuthored, setDuration, setMovementRule, setRecoveryOverride, setRuleApplies, shortLabel, slidingSlashSample,
  climbingSample, swimmingSample, traversalSample, chargedBashSample, gestureLabel, gestureOf, CHORD_PARTNERS, INPUT_CONTEXTS, SCENARIO_FORMAT,
  type ConfigCheck, type LabAuthoredAction, type LabAuthoredPhase, type LabComparison, type LabConfig, type LabConfigEdge,
  type LabConfigVersion, type LabEffect, type LabFinding,
  type LabLibrary, type LabNetwork, type LabRun, type LabScenario,
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

const versionKey = (version: LabConfigVersion) => `${version.name}\u0000${version.version}`;

const FINDING_TITLES: Record<LabFinding["kind"], string> = {
  unreachableTransition: "Unreachable transition",
  expiredBuffer: "Expired buffer",
  inputLag: "Input-to-action lag",
  arbitrationConflict: "Arbitration conflict",
  cancelLoop: "Cancel loop",
  comboRegression: "Combo regression",
};

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
  readonly #promoteBox = element("div", { className: "lab-promote" });
  /** The saved version explicitly chosen for promotion (`versionKey`), or "". */
  #promoting = "";
  #promotionNote = "";
  #setup: LabSetup;
  #scenario: LabScenario | null = null;
  #configs: Record<Slot, LabConfig>;
  #checks: Record<Slot, ConfigCheck>;
  #editing: Slot = "b";
  /** Whether the timeline lists movement commands; hidden by default (#382), always replayed. */
  #showMovement = false;
  /** The configuration the arena runs now. */
  #live: Slot | null = null;
  #comparison: LabComparison | null = null;
  /** The simulated network the comparison runs across (#415). */
  #network: LabNetwork = LOCAL_NETWORK;
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
    this.root.append(this.#recordingBar(), this.#scenarioBox, this.#configBox, this.#compareBox, this.#promoteBox);
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
        this.#comparison = this.#runtime.compare(scenario, this.#configs.a, this.#configs.b, this.#network);
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
    this.#renderPromotion();
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
    const rows = scenario.commands.flatMap((command, index) => {
      if (!this.#showMovement && isMovement(command)) { return []; }
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
      return [element("tr", {},
        element("td", {}, tick),
        element("td", { className: "lab-actions" },
          button("−", "One tick earlier", () => moveCommand(scenario, index, command.tick - 1)),
          button("+", "One tick later", () => moveCommand(scenario, index, command.tick + 1)),
          button("⧉", "Insert a copy one tick later", () => insertCopy(scenario, index, command.tick + 1)),
          button("✕", "Remove", () => removeCommand(scenario, index))),
        element("td", { className: "lab-label", title: result?.label ?? command.bytes }, shortLabel(result?.label ?? command.bytes)),
        element("td", { title: "Input-to-effect delay under A / B" }, `${delay(result)} / ${delay(other)}`),
        element("td", { title: "Arrival at the zone and its arbitration under A and the simulated network" }, result ? arrivalText(result) : "—"),
        element("td", { className: result?.refused.length ? "lab-refused" : "" }, result?.refused.length ? `refused ${result.refused.join(", ")}` : "ok"))];
    });
    const recorded = scenario.recorded
      ? `recorded under ${scenario.recorded.config}`
      : "edited (no longer the recording)";
    const movement = scenario.commands.filter(isMovement).length;
    const showMovement = element("input", { type: "checkbox", checked: this.#showMovement, title: "Movement commands are always replayed" });
    showMovement.addEventListener("change", () => { this.#showMovement = showMovement.checked; this.#render(); });
    this.#scenarioBox.replaceChildren(bar,
      element("p", { className: "lab-muted" },
        `${scenario.name}: ${scenario.setup.className} ${scenario.setup.level}, seed ${scenario.seed}, content ${scenario.contentRevision}, ${recorded}.`),
      element("label", {}, "Duration (ticks) ", duration),
      element("label", {}, " ", showMovement, ` Show movement (${movement})`),
      element("div", { className: "lab-scroll" }, element("table", { className: "lab-table" },
        element("thead", {}, element("tr", {}, ...["Tick", "Edit", "Command", "Delay A/B", "Arrival A", "Result A"].map((text) => element("th", {}, text)))),
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
    for (const entry of this.#library.configs) {
      savedConfigs.append(element("option", { value: versionKey(entry) }, `${entry.name} v${entry.version} · ${entry.identity}`));
    }
    savedConfigs.addEventListener("change", () => {
      const found = this.#library.configs.find((entry) => versionKey(entry) === savedConfigs.value);
      if (found) { this.#setConfig(slot, { ...structuredClone(found.config), name: found.name }); }
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
      this.#authoredEditor(slot),
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
        action("Swimming sample", "In deep water (Stillwater Lake, east of here) melee and Shield Bash are refused, other abilities wait until ashore, and the swim glides (#413)",
          () => this.#setConfig(slot, swimmingSample(config))),
        action("Climbing sample", "While the character holds a climbable face (jump at a crag, palisade or ladder), abilities wait until it is off the face (#490)",
          () => this.#setConfig(slot, climbingSample(config))),
        action("+ Sliding slash", "Author a new Warden action from existing primitives: forward + Heroic Strike winds up, slides and recovers (#414)",
          () => this.#setConfig(slot, slidingSlashSample(config))),
        action("+ Charged bash", "Hold Shield Bash ≥ 200 ms and release to lunge; a tap is the plain Bash. After “+ Sliding slash” it is the slash's timed, buffered follow-up instead (#414)",
          () => this.#setConfig(slot, chargedBashSample(config))),
        action(slot === "a" ? "Copy to B" : "Copy to A", "Copy this configuration to the other slot", () => {
          const other = slot === "a" ? "b" : "a";
          this.#setConfig(other, copyConfig(config, `${config.name} copy`));
        }),
        action("Save version", "Save these validated rules as the next version of this name, in this browser", () => {
          if (check.error || !check.identity) { this.#error = `${slot.toUpperCase()}: only a valid configuration is saved`; this.#render(); return; }
          this.#library.saveConfig(config, check.identity);
          this.#render();
        }),
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

  /**
   * New experimental actions (#414, Atlas 5.9.4–5.9.5): a trigger in a context, timed phases with
   * bounded movement and effects on the ordinary use. The zone validates the whole definition and
   * names any unsupported primitive; outgoing edges are edited with the other edges above.
   */
  #authoredEditor(slot: Slot): HTMLElement {
    const config = this.#configs[slot];
    const rows = (config.authored ?? []).map((authored) => {
      const change = (next: LabAuthoredAction) => this.#setConfig(slot, setAuthored(config, next));
      const name = element("input", { type: "text", value: authored.name, title: "Action name" });
      name.addEventListener("change", () => change({ ...authored, name: name.value }));
      const context = element("select", { title: "Input context" });
      for (const entry of INPUT_CONTEXTS) { context.append(element("option", { value: entry, selected: entry === authored.context }, entry)); }
      context.addEventListener("change", () => change({ ...authored, context: context.value }));
      // How the input is performed (hold-to-charge, chords) and a timed follow-up (#414).
      const gesture = element("select", { title: "Gesture: the press, hold-to-charge and release, or a chord with a held control" });
      for (const entry of ["Press", "Charge", ...CHORD_PARTNERS.map((partner) => `Chord ${partner}`)]) {
        gesture.append(element("option", { value: entry, selected: entry === gestureLabel(authored.gesture) }, entry));
      }
      gesture.addEventListener("change", () => {
        const { gesture: _, ...rest } = authored;
        const next = gestureOf(gesture.value, authored.gesture, authored.startup.ms);
        change(next ? { ...rest, gesture: next } : rest);
      });
      const charge = authored.gesture?.kind === "Charge"
        ? [this.#numberField("charged after ms", authored.gesture.minMs, (minMs) => change({ ...authored, gesture: { kind: "Charge", minMs } }), 0, true)]
        : [];
      const sources = (config.authored ?? []).filter((other) => other.slot !== authored.slot);
      const follows = element("select", { title: "Only as a timed follow-up of another authored action" });
      follows.append(element("option", { value: "", selected: !authored.follows }, "any time"));
      for (const other of sources) {
        follows.append(element("option", { value: String(other.slot), selected: authored.follows?.source === other.slot }, `after ${other.name}`));
      }
      follows.addEventListener("change", () => {
        const { follows: _, ...rest } = authored;
        change(follows.value === "" ? rest : {
          ...rest, follows: { openAfterMs: 100, actionableMs: 200, bufferMs: 100, ...authored.follows, source: Number(follows.value) },
        });
      });
      const timed = authored.follows;
      const followFields = timed ? [element("div", { className: "lab-flags" },
        this.#numberField("opens after its hit ms", timed.openAfterMs, (openAfterMs) => change({ ...authored, follows: { ...timed, openAfterMs } }), 0, true),
        this.#numberField("open for ms", timed.actionableMs, (actionableMs) => change({ ...authored, follows: { ...timed, actionableMs } }), 0, true),
        this.#numberField("buffer ms", timed.bufferMs, (bufferMs) => change({ ...authored, follows: { ...timed, bufferMs } }), 0, true))] : [];
      const phase =(label: string, key: "startup" | "active" | "recovery") => {
        const value: LabAuthoredPhase = authored[key];
        const set = (patch: Partial<LabAuthoredPhase>) => change({ ...authored, [key]: { ...value, ...patch } });
        return element("div", { className: "lab-flags" },
          this.#numberField(`${label} ms`, value.ms, (ms) => set({ ms }), 0, true),
          this.#numberField("steering %", value.steeringPercent, (steeringPercent) => set({ steeringPercent })),
          this.#numberField("momentum %", value.momentumPercent, (momentumPercent) => set({ momentumPercent })));
      };
      const impulse = authored.effects.find((effect) => effect.kind === "Impulse");
      const launch = this.#numberField("launch forward cm/s", impulse?.kind === "Impulse" ? impulse.forwardCms : 0, (forwardCms) => change({
        ...authored,
        effects: [
          ...authored.effects.filter((effect) => effect.kind !== "Impulse"),
          ...(forwardCms === 0 ? [] : [{ kind: "Impulse", forwardCms, upCms: impulse?.kind === "Impulse" ? impulse.upCms : 0 } as LabEffect]),
        ],
      }), -1_200);
      const remove = element("button", { type: "button", title: "Remove this action and the edges leaving it" }, "Remove");
      remove.addEventListener("click", () => this.#setConfig(slot, removeAuthored(config, authored.slot)));
      return element("fieldset", { className: "lab-edge-editor" },
        element("legend", {}, `Authored(${authored.slot}) · ${authored.class} · ${authored.input}${authored.ability === null ? "" : ` ability ${authored.ability}`}`),
        element("div", { className: "lab-flags" }, name, element("label", {}, "when ", context), element("label", {}, "as ", gesture), ...charge,
          element("label", {}, "", follows), remove),
        ...followFields,
        phase("Startup (the input waits)", "startup"), phase("Active (the use resolves)", "active"), phase("Recovery", "recovery"),
        launch);
    });
    return element("fieldset", { className: "lab-edge-editor" },
      element("legend", {}, "Authored actions (new experimental moves)"),
      ...(rows.length ? rows : [element("p", { className: "lab-muted" }, "None: add one with “+ Sliding slash” or in the JSON.")]));
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

  /** The simulated network both runs cross (#415): a latency/jitter preset and the zone's late-input grace. */
  #networkControls(): HTMLElement[] {
    const preset = element("select", { title: "Simulated round trip and jitter between the player and the zone (illustrative presets)" });
    for (const [index, entry] of NETWORK_PRESETS.entries()) {
      const option = element("option", { value: String(index) }, entry.name);
      option.selected = entry.roundTripMs === this.#network.roundTripMs && entry.jitterMs === this.#network.jitterMs;
      preset.append(option);
    }
    preset.addEventListener("change", () => {
      const chosen = NETWORK_PRESETS[Number(preset.value)] ?? NETWORK_PRESETS[0]!;
      this.#network = { ...this.#network, roundTripMs: chosen.roundTripMs, jitterMs: chosen.jitterMs };
      this.#compare();
    });
    const compensate = element("input", { type: "checkbox", title: "Combo inputs report the tick they were made against, so the zone's bounded grace applies" });
    compensate.checked = this.#network.compensate;
    compensate.addEventListener("change", () => {
      this.#network = { ...this.#network, compensate: compensate.checked };
      this.#compare();
    });
    return [element("label", {}, "Network ", preset), element("label", {}, compensate, " late-input grace")];
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
      action("Compare", () => this.#compare(), "Run the scenario under A and B"),
      ...this.#networkControls());
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
      this.#diagnostics(comparison),
      element("p", { className: "lab-muted" }, `${comparison.differences.length} ticks differ in actions, events, health, resource or position. Click a side to replay it paused there.`),
      element("div", { className: "lab-scroll" }, element("table", { className: "lab-table lab-diff" },
        element("thead", {}, element("tr", {}, element("th", {}, "Tick"), element("th", {}, "A"), element("th", {}, "B"))),
        element("tbody", {}, ...differences))));
  }

  /** The explained diagnostics of both runs and the combos B lost (#383, Atlas 5.6.6). */
  #diagnostics(comparison: LabComparison): HTMLElement {
    const entries: [string, Slot, LabFinding][] = [
      ...comparison.regressions.map((finding) => ["A→B", "b", finding] as [string, Slot, LabFinding]),
      ...comparison.a.findings.map((finding) => ["A", "a", finding] as [string, Slot, LabFinding]),
      ...comparison.b.findings.map((finding) => ["B", "b", finding] as [string, Slot, LabFinding]),
    ];
    // Warnings first, then by tick.
    entries.sort(([, , left], [, , right]) => Number(left.severity === "note") - Number(right.severity === "note") || left.tick - right.tick);
    const warnings = entries.filter(([, , finding]) => finding.severity === "warning").length;
    const items = entries.map(([run, slot, finding]) => {
      const { trace } = finding;
      const edge = trace.edge;
      const decision = trace.decision;
      const line = (name: string, value: string) => element("tr", {}, element("th", {}, name), element("td", {}, value));
      const replay = element("button", { type: "button", title: `Replay ${slot.toUpperCase()} paused at this tick` }, "Replay here");
      replay.addEventListener("click", () => void this.#replay(slot, trace.tick));
      return element("details", { className: `lab-finding lab-${finding.severity}` },
        element("summary", {}, `${run} · ${FINDING_TITLES[finding.kind]} · tick ${finding.tick}: ${shortLabel(finding.summary)}`),
        element("table", { className: "lab-table lab-trace" }, element("tbody", {},
          line("Active state", trace.regions.filter(Boolean).join(" · ") || "idle"),
          line("Pending commands", trace.pending.map(shortLabel).join("; ") || "none"),
          line("Edge condition", edge ? `${edge.edge}: ${edge.status}` : "—"),
          line("Eligible window", edge?.openFrom === null || !edge
            ? "—"
            : `buffer ${edge.bufferFrom}, open ${edge.openFrom}–${edge.openUntil}${edge.precision ? `, precise ${edge.precision[0]}–${edge.precision[1]}` : ""}`),
          line("Source event", edge?.anchorTick == null ? "—" : `tick ${edge.anchorTick}: ${trace.sourceEvents.map(shortLabel).join(", ") || "—"}`),
          line("Observed decision", decision
            ? `${decision.input} → ${decision.outcome}${decision.precise ? " (precise)" : ""} [${decision.candidates.map((entry) => `${entry.candidate}: ${entry.verdict}`).join("; ")}]`
            : "—"),
          line("First diverging tick", String(trace.divergesAt ?? comparison.divergesAt ?? "none")))),
        replay);
    });
    return element("div", { className: "lab-findings" },
      element("h4", {}, `Diagnostics: ${warnings} warning${warnings === 1 ? "" : "s"}, ${entries.length - warnings} notes`),
      element("p", { className: "lab-muted" }, "Explained from the authoritative ticks of this scenario. Diagnostics, not a ranking: faster or harder-hitting rules are not automatically better."),
      ...items);
  }

  /** Explicit promotion of a saved version as a reviewable change (#383, Atlas 5.6.7). */
  #renderPromotion(): void {
    const versions = this.#library.configs;
    const chosen = versions.find((entry) => versionKey(entry) === this.#promoting) ?? null;
    const select = element("select", { title: "The saved configuration version to promote" });
    select.append(element("option", { value: "" }, "choose a saved version…"));
    for (const entry of versions) {
      select.append(element("option", { value: versionKey(entry), selected: versionKey(entry) === this.#promoting },
        `${entry.name} v${entry.version} · ${entry.identity}`));
    }
    select.addEventListener("change", () => { this.#promoting = select.value; this.#render(); });
    const intro = element("p", { className: "lab-muted" },
      "Promotion never changes rules here. It exports a request; in a checkout, the command below writes the new production graph, "
      + "a promotion record and a pull request description. Only the reviewed, merged pull request changes gameplay.");
    const parts: HTMLElement[] = [element("h3", {}, "Promote"), intro, element("div", { className: "lab-playback" }, select)];
    if (!chosen) {
      parts.push(element("p", { className: "lab-muted" }, versions.length ? "Choose the saved version to promote." : "Save a configuration version first."));
      this.#promoteBox.replaceChildren(...parts);
      return;
    }
    const baseline = this.#runtime.baseline();
    const production = this.#runtime.check(baseline).identity ?? "";
    const changes = this.#runtime.changes(baseline, chosen.config);
    const scenario = this.#scenario;
    parts.push(changes.length
      ? element("ul", { className: "lab-changes" }, ...changes.map((change) => element("li", {}, element("code", {}, change))))
      : element("p", { className: "lab-refused" }, "This version equals production: nothing to promote."));
    parts.push(element("p", { className: scenario ? "lab-muted" : "lab-refused" }, scenario
      ? `Evidence: scenario “${scenario.name}” (${scenario.ticks} ticks, ${scenario.commands.length} commands), replayed again by the command.`
      : "Save or import the scenario that is this promotion's evidence."));
    const note = element("textarea", { className: "lab-json", rows: 2, placeholder: "Why this should become gameplay (for the pull request)", value: this.#promotionNote });
    note.addEventListener("input", () => { this.#promotionNote = note.value; });
    const ready = scenario !== null && changes.length > 0;
    const exportButton = element("button", { type: "button", disabled: !ready, title: "Download the promotion request for the checkout command" },
      `Export promotion of ${chosen.name} v${chosen.version}`);
    const file = promotionFileName(chosen);
    exportButton.addEventListener("click", () => {
      if (!scenario) { return; }
      download(file, JSON.stringify(promotionRequest(chosen, production, scenario, this.#promotionNote), null, 2));
    });
    parts.push(note, element("div", { className: "lab-playback" }, exportButton),
      element("p", { className: "lab-muted" }, "Then, in a checkout:"),
      element("pre", { className: "lab-command" }, promoteCommand(`path/to/${file}`)));
    this.#promoteBox.replaceChildren(...parts);
  }
}
