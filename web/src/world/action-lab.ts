/**
 * The Action Lab (#381, Atlas 5.6.1/5.6.3/5.6.5): a configurable ephemeral character in a fresh
 * local world, with a statechart, a tick timeline and a property inspector synchronized to the
 * authoritative action graph `mmorpg-core` reports through `LocalZone.actionInspection`.
 *
 * The page owns no rules here: it only shows what the zone reports, pauses, steps and scrubs the
 * recorded ticks of the scenario source, and restarts the lab with another validated setup.
 */
import type { ContentCatalog } from "./catalog";
import type { ScenarioFrame } from "./scenario-source";

export type LabGear = "starter" | "standard" | "none";

/** A lab character setup, carried in the URL so a reset reproduces it. */
export type LabSetup = {
  className: string;
  level: number;
  gear: LabGear;
  /** A main-hand catalog item replacing the gear's weapon, or 0 to keep it. */
  mainHand: number;
  /** The class resource at the start, or null for the level's usual start. */
  resource: number | null;
};

export type LabOptions = { maxLevel: number; mainHand: readonly { id: number; name: string }[] };

/** The authored graph from `actionGraph()`. */
export type ActionGraph = {
  tickHz: number;
  regions: readonly string[];
  actions: readonly { id: string; region: string; recoveryMs: number | null; recoveryTicks: number | null }[];
  modifiers: readonly { action: string; condition: string; recoveryMs: number; recoveryTicks: number }[];
  edges: readonly GraphEdge[];
  windowModifiers: readonly { edge: string; conditions: readonly string[]; actionableDeltaMs: number }[];
  rules: readonly { id: string; during: string; input: string; conditions: readonly string[]; effect: "Hold" | "Cancel" }[];
  interruptions: readonly { reaction: string; interrupted: string; phase: string | null; consequence: string }[];
  /** Class-, weapon- and ability-specific values replacing the shared ones (none in production). */
  overrides: readonly { key: string; value: string; scope: string }[];
};

/** An overridable value as it applies now: its key, value and origin ("baseline" or the override's scope). */
export type EffectiveValue = { key: string; value: string; origin: string };

export type GraphEdge = {
  id: string; from: string; input: string; anchor: string;
  openAfterMs: number; actionableMs: number; bufferMs: number;
  openAfterTicks: number; actionableTicks: number; bufferTicks: number;
  conditions: readonly string[]; effects: readonly string[]; priority: number;
  precision: { afterOpenMs: number; lengthMs: number; benefits: readonly string[] } | null;
};

export type BlockReason =
  | { kind: "Condition"; condition: string }
  | { kind: "Cancelled"; reaction: string }
  | { kind: "SourceInactive" | "TooEarly" | "Expired" };
export type EdgeStatus =
  | { kind: "Open" | "Buffering" }
  | { kind: "Suspended"; reaction: string }
  | { kind: "Blocked"; reason: BlockReason };
export type Verdict =
  | { kind: "Won" | "Holding" }
  | { kind: "Outranked"; by: string }
  | { kind: "HeldBy"; rule: string }
  | { kind: "Blocked"; reason: BlockReason };
export type Outcome =
  | { kind: "Proceed"; candidate: string }
  | { kind: "WaitForRule"; rule: string }
  | { kind: "WaitForEdge"; edge: string }
  | { kind: "Cancel"; rule: string };

export type RuleReport = { rule: string; input: string; effect: "Hold" | "Cancel"; releasedAt: number; origin: string };

/** One tick's `actionInspection()`. */
export type Inspection = {
  tick: number;
  regions: readonly {
    region: string; action: string | null; phase: string | null; recoveryLeft: number | null;
    /** The ability the action runs (a cast's, or Heroic Strike for the directed slash). */
    ability: string | null;
    /** Ticks of a cast or channel left. */
    activeLeft: number | null;
  }[];
  edges: readonly {
    edge: string; priority: number; status: EdgeStatus;
    anchorTick: number | null; bufferFrom: number | null; openFrom: number | null; openUntil: number | null;
    precision: { from: number; until: number } | null;
    /** Where the edge's window comes from. */
    origin: string;
  }[];
  rules: readonly RuleReport[];
  interruptions: readonly { interrupted: string; reaction: string; consequence: string; authored: boolean }[];
  arbitrations: readonly {
    input: string; outcome: Outcome; precise: boolean;
    candidates: readonly { candidate: string; verdict: Verdict }[];
  }[];
  effective: readonly EffectiveValue[];
};

export const LAB_SCENARIO = "action-lab";

/** Ready-made setups (Atlas 5.6.3); each restarts the lab reproducibly. */
export const LAB_PRESETS: readonly { name: string; setup: LabSetup }[] = [
  { name: "Warden combo", setup: { className: "warden", level: 6, gear: "standard", mainHand: 0, resource: 60 } },
  { name: "Fresh Warden", setup: { className: "warden", level: 1, gear: "starter", mainHand: 0, resource: null } },
  { name: "Ranger", setup: { className: "ranger", level: 6, gear: "standard", mainHand: 0, resource: null } },
  { name: "Arcanist", setup: { className: "arcanist", level: 8, gear: "standard", mainHand: 0, resource: null } },
];

const DEFAULT_SETUP = LAB_PRESETS[0]!.setup;
const GEARS: readonly LabGear[] = ["starter", "standard", "none"];

function integer(value: string | null, name: string, fallback: number): number {
  if (value === null || value === "") { return fallback; }
  if (!/^\d{1,5}$/.test(value)) { throw new Error(`Action Lab ${name} "${value}" is not a whole number.`); }
  return Number(value);
}

/** The lab setup in `?scenario=action-lab&class=…&level=…&gear=…&weapon=…&resource=…`. */
export function labSetupFromQuery(search: string): LabSetup {
  const query = new URLSearchParams(search);
  const gear = query.get("gear") ?? DEFAULT_SETUP.gear;
  if (!GEARS.includes(gear as LabGear)) { throw new Error(`Action Lab gear "${gear}" is not one of ${GEARS.join(", ")}.`); }
  const resource = query.get("resource");
  return {
    className: query.get("class") ?? DEFAULT_SETUP.className,
    level: integer(query.get("level"), "level", DEFAULT_SETUP.level),
    gear: gear as LabGear,
    mainHand: integer(query.get("weapon"), "weapon", 0),
    resource: resource === null || resource === "" ? null : integer(resource, "resource", 0),
  };
}

/** The page query that restarts the lab with `setup` and `seed`. */
export function labQuery(setup: LabSetup, seed: bigint): string {
  const query = new URLSearchParams({ scenario: LAB_SCENARIO, class: setup.className, level: String(setup.level), gear: setup.gear });
  if (setup.mainHand !== 0) { query.set("weapon", String(setup.mainHand)); }
  if (setup.resource !== null) { query.set("resource", String(setup.resource)); }
  if (seed !== 0n) { query.set("seed", String(seed)); }
  return `?${query}`;
}

/** A run of ticks one region spent in one action and phase. */
export type TimelineSegment = { action: string; phase: string | null; from: number; to: number };

/** Per region, the runs of active actions over `frames` (ticks without inspection are skipped). */
export function timelineSegments(frames: readonly { tick: number; inspection: Inspection }[], regions: readonly string[]):
  Map<string, TimelineSegment[]> {
  const rows = new Map<string, TimelineSegment[]>(regions.map((region) => [region, []]));
  for (const { tick, inspection } of frames) {
    for (const report of inspection.regions) {
      const row = rows.get(report.region);
      if (!row || report.action === null) { continue; }
      const last = row.at(-1);
      if (last && last.action === report.action && last.phase === report.phase && last.to === tick - 1) {
        last.to = tick;
      } else {
        row.push({ action: report.action, phase: report.phase, from: tick, to: tick });
      }
    }
  }
  return rows;
}

function reasonText(reason: BlockReason): string {
  switch (reason.kind) {
    case "Condition": return `condition ${reason.condition} does not hold`;
    case "SourceInactive": return "its source action is not running";
    case "TooEarly": return "the input is earlier than the buffer";
    case "Expired": return "the actionable window has closed";
    case "Cancelled": return `${reason.reaction} cancels the opening`;
  }
}

/** Why an edge is or is not usable at the inspected tick, with its absolute window ticks. */
export function explainEdge(report: Inspection["edges"][number]): string {
  const precision = report.precision ? `, precise ${report.precision.from}–${report.precision.until}` : "";
  const window = report.openFrom === null ? "" : ` (buffer ${report.bufferFrom}, open ${report.openFrom}–${report.openUntil}${precision})`;
  switch (report.status.kind) {
    case "Open": return `Open: the input follows this edge now${window}.`;
    case "Buffering": return `Buffering: an input now waits for tick ${report.openFrom}${window}.`;
    case "Suspended": return `Suspended by ${report.status.reaction}: its clock stops and it holds a buffered input${window}.`;
    case "Blocked": return `Blocked: ${reasonText(report.status.reason)}${window}.`;
  }
}

/** What an applying compatibility rule does, until when, and why it applies to this character. */
export function explainRule(rule: RuleReport): string {
  const what = rule.effect === "Cancel" ? `${rule.input} ends the running action before tick` : `holds ${rule.input} until tick`;
  return `${rule.rule}: ${what} ${rule.releasedAt}${rule.origin === "baseline" ? "" : ` (${rule.origin})`}`;
}

/** What the reactions in effect do to one running action, opening or buffer. */
export function explainInterruption(report: Inspection["interruptions"][number]): string {
  return `${report.reaction}: ${report.consequence.toLowerCase()} ${report.interrupted}${report.authored ? "" : " (default)"}`;
}

/** What arbitration would make of one input at the next tick. */
export function explainVerdict(verdict: Verdict): string {
  switch (verdict.kind) {
    case "Won": return "wins";
    case "Holding": return "holds the input until it opens";
    case "Outranked": return `usable, outranked by ${verdict.by}`;
    case "HeldBy": return `held by rule ${verdict.rule}`;
    case "Blocked": return `blocked: ${reasonText(verdict.reason)}`;
  }
}

export function explainOutcome(outcome: Outcome): string {
  switch (outcome.kind) {
    case "Proceed": return outcome.candidate === "Direct" ? "resolves now as its ordinary use" : `resolves now through ${outcome.candidate}`;
    case "WaitForRule": return `waits for rule ${outcome.rule}`;
    case "WaitForEdge": return `waits for edge ${outcome.edge} to open`;
    case "Cancel": return `resolves now and ends the running action (rule ${outcome.rule})`;
  }
}

/** What the inspector shows: an action node, an edge or the arbitration of the shown tick. */
export type LabSelection = { kind: "action"; id: string } | { kind: "edge"; id: string } | { kind: "tick" };

export type LabControls = {
  togglePause(): void;
  step(): void;
  /** A recorded tick to show, or null for live. */
  scrub(tick: number | null): void;
  apply(setup: LabSetup): void;
};

/** The viewer facts the inspector shows besides the graph. */
export type LabViewer = { level: number; health: number; maxHealth: number; resource: string | null; clip: string | null };

const TIMELINE_TICKS = 150;

export function element<K extends keyof HTMLElementTagNameMap>(tag: K, props: Partial<HTMLElementTagNameMap[K]> = {}, ...children: (Node | string)[]):
  HTMLElementTagNameMap[K] {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}

export function section(title: string, body: HTMLElement): HTMLDetailsElement {
  const details = element("details", { open: true });
  details.append(element("summary", {}, title), body);
  return details;
}

/** The lab's dock beside the arena: setup, playback and timeline, statechart and inspector. */
export class ActionLabPanel {
  readonly #root: HTMLElement;
  readonly #graph: ActionGraph;
  readonly #controls: LabControls;
  readonly #status = element("p", { className: "lab-status" });
  readonly #pause = element("button", { type: "button" });
  readonly #tickInput = element("input", { type: "number", min: "0", step: "1", title: "Show tick" });
  readonly #slider = element("input", { type: "range", min: "0", max: "0", step: "1", title: "Scrub recorded ticks" });
  readonly #timeline = element("div", { className: "lab-timeline" });
  readonly #chart = element("div", { className: "lab-chart" });
  readonly #inspector = element("div", { className: "lab-inspector" });
  #selection: LabSelection = { kind: "tick" };
  #renderedKey = "";
  #shown: { tick: number; inspection: Inspection } | null = null;
  readonly #parsed = new WeakMap<ScenarioFrame, Inspection>();
  #first = 0;

  constructor(root: HTMLElement, graph: ActionGraph, options: LabOptions, catalog: ContentCatalog, setup: LabSetup, controls: LabControls) {
    this.#root = root;
    this.#graph = graph;
    this.#controls = controls;
    const collapse = element("button", { type: "button", title: "Collapse or expand the lab" }, "Hide");
    collapse.addEventListener("click", () => {
      const collapsed = root.classList.toggle("collapsed");
      collapse.textContent = collapsed ? "Show" : "Hide";
    });
    root.replaceChildren(
      element("header", { className: "lab-header" }, element("h2", {}, "Action Lab"), this.#status, collapse),
      section("Character", this.#setupForm(options, catalog, setup)),
      section("Timeline", this.#playback()),
      section("Statechart", this.#chart),
      section("Inspector", this.#inspector),
    );
  }

  /** Adds a dock section after the timeline, such as the experiments (#382). */
  addSection(title: string, body: HTMLElement): void {
    this.#root.querySelector("details:nth-of-type(2)")?.after(section(title, body));
  }

  #setupForm(options: LabOptions, catalog: ContentCatalog, setup: LabSetup): HTMLElement {
    const form = element("form", { className: "lab-setup" });
    const classSelect = element("select", { name: "class" });
    for (const record of catalog.classes.values()) {
      classSelect.append(element("option", { value: record.name, selected: record.name === setup.className }, record.name));
    }
    const level = element("input", { type: "number", name: "level", min: "1", max: String(options.maxLevel), step: "1", value: String(setup.level) });
    const gear = element("select", { name: "gear" });
    for (const value of GEARS) { gear.append(element("option", { value, selected: value === setup.gear }, value)); }
    const weapon = element("select", { name: "weapon" });
    weapon.append(element("option", { value: "0", selected: setup.mainHand === 0 }, "gear's weapon"));
    for (const item of options.mainHand) {
      weapon.append(element("option", { value: String(item.id), selected: item.id === setup.mainHand }, item.name));
    }
    const resource = element("input", { type: "number", name: "resource", min: "0", step: "1", placeholder: "level start",
      value: setup.resource === null ? "" : String(setup.resource) });
    const abilities = element("p", { className: "lab-abilities" });
    const showAbilities = () => {
      const names = [...catalog.abilities.values()]
        .filter((ability) => ability.user === classSelect.value && ability.level <= Number(level.value))
        .map((ability) => ability.name);
      abilities.textContent = `Abilities: ${names.join(", ") || "none"}`;
    };
    classSelect.addEventListener("change", showAbilities);
    level.addEventListener("input", showAbilities);
    showAbilities();
    const presets = element("div", { className: "lab-presets" });
    for (const preset of LAB_PRESETS) {
      const button = element("button", { type: "button" }, preset.name);
      button.addEventListener("click", () => this.#controls.apply(preset.setup));
      presets.append(button);
    }
    const label = (text: string, input: HTMLElement) => element("label", {}, text, input);
    form.append(
      label("Class", classSelect), label("Level", level), label("Gear", gear), label("Main hand", weapon), label("Resource", resource),
      element("button", { type: "submit" }, "Apply & reset"), abilities, presets,
    );
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      this.#controls.apply({
        className: classSelect.value,
        level: Number(level.value),
        gear: gear.value as LabGear,
        mainHand: Number(weapon.value),
        resource: resource.value === "" ? null : Number(resource.value),
      });
    });
    return form;
  }

  #playback(): HTMLElement {
    const step = element("button", { type: "button" }, "Step (F8)");
    const live = element("button", { type: "button" }, "Live");
    this.#pause.addEventListener("click", () => this.#controls.togglePause());
    step.addEventListener("click", () => this.#controls.step());
    live.addEventListener("click", () => this.#controls.scrub(null));
    this.#slider.addEventListener("input", () => this.#controls.scrub(Number(this.#slider.value)));
    this.#tickInput.addEventListener("change", () => this.#controls.scrub(Number(this.#tickInput.value)));
    this.#timeline.addEventListener("click", (event) => {
      const track = event.target instanceof Element ? event.target.closest(".lab-track") : null;
      if (!track) { return; }
      const box = track.getBoundingClientRect();
      const fraction = Math.min(Math.max((event.clientX - box.left) / box.width, 0), 1);
      this.#controls.scrub(this.#first + Math.round(fraction * (TIMELINE_TICKS - 1)));
    });
    const bar = element("div", { className: "lab-playback" }, this.#pause, step, live, element("label", {}, "Tick ", this.#tickInput));
    return element("div", {}, bar, this.#slider, this.#timeline);
  }

  /** Redraws for the source's recorded ticks, the shown one and the viewer; skips unchanged frames. */
  update(frames: readonly ScenarioFrame[], shown: ScenarioFrame | null, paused: boolean, viewer: LabViewer): void {
    const key = `${frames.at(-1)?.tick}|${shown?.tick}|${paused}|${JSON.stringify(this.#selection)}|${viewer.clip}|${viewer.health}|${viewer.resource}`;
    if (key === this.#renderedKey) { return; }
    this.#renderedKey = key;
    const shownTick = shown ? Number(shown.tick) : null;
    const first = Number(frames[0]?.tick ?? 0n);
    const last = Number(frames.at(-1)?.tick ?? 0n);
    // Only the timeline's window is parsed, once per recorded frame.
    const windowStart = Math.max(first, Math.min(Math.max((shownTick ?? last) + 10, TIMELINE_TICKS - 1), last) - TIMELINE_TICKS);
    const parsed = frames.flatMap((frame) => {
      if (frame.inspection === null || Number(frame.tick) < windowStart) { return []; }
      let inspection = this.#parsed.get(frame);
      if (!inspection) {
        inspection = JSON.parse(frame.inspection) as Inspection;
        this.#parsed.set(frame, inspection);
      }
      return [{ tick: Number(frame.tick), inspection }];
    });
    this.#shown = parsed.find((frame) => frame.tick === shownTick) ?? null;
    this.#pause.textContent = paused ? "Play (F7)" : "Pause (F7)";
    this.#slider.min = String(first);
    this.#slider.max = String(last);
    this.#slider.value = String(shownTick ?? last);
    this.#tickInput.value = String(shownTick ?? last);
    this.#status.textContent = `${paused ? "Paused" : "Live"} · tick ${shownTick ?? "—"}${shownTick !== null && shownTick < last ? ` of ${last}` : ""} · ${this.#graph.tickHz} Hz`;
    this.#renderTimeline(parsed, shownTick ?? last);
    this.#renderChart();
    this.#renderInspector(viewer);
  }

  #select(selection: LabSelection): void {
    this.#selection = selection;
    this.#renderedKey = "";
  }

  #renderTimeline(parsed: readonly { tick: number; inspection: Inspection }[], shownTick: number): void {
    const end = Math.max(shownTick + 10, TIMELINE_TICKS - 1);
    const last = parsed.at(-1)?.tick ?? 0;
    this.#first = Math.max(0, Math.min(end, last) - (TIMELINE_TICKS - 1));
    const first = this.#first;
    const visible = parsed.filter((frame) => frame.tick >= first && frame.tick < first + TIMELINE_TICKS);
    const rows = timelineSegments(visible, this.#graph.regions);
    const x = (tick: number) => ((tick - first) / TIMELINE_TICKS) * 100;
    const row = (name: string, blocks: HTMLElement[]) => {
      const head = element("span", { className: "lab-playhead" });
      head.style.left = `${x(shownTick)}%`;
      return element("div", { className: "lab-row" }, element("span", { className: "lab-row-name" }, name),
        element("div", { className: "lab-track" }, ...blocks, head));
    };
    const rowsHtml = [...rows].map(([region, segments]) => {
      const blocks = segments.map((segment) => element("span", {
        className: `lab-segment phase-${(segment.phase ?? "active").toLowerCase()}`,
        title: `${segment.action} ${segment.phase ?? ""} · ticks ${segment.from}–${segment.to}`,
        textContent: segment.action,
      }));
      blocks.forEach((block, index) => {
        const segment = segments[index]!;
        block.style.left = `${x(segment.from)}%`;
        block.style.width = `${Math.max(x(segment.to + 1) - x(segment.from), 0.6)}%`;
      });
      return row(region, blocks);
    });
    // The shown tick's edge windows, on their own row.
    const windows = (this.#shown?.inspection.edges ?? []).flatMap((edge) => {
      if (edge.bufferFrom === null || edge.openFrom === null || edge.openUntil === null) { return []; }
      const buffer = element("span", { className: "lab-window buffer", title: `${edge.edge} buffer ${edge.bufferFrom}–${edge.openFrom - 1}` });
      buffer.style.left = `${x(edge.bufferFrom)}%`;
      buffer.style.width = `${x(edge.openFrom) - x(edge.bufferFrom)}%`;
      const open = element("span", { className: "lab-window open", title: `${edge.edge} open ${edge.openFrom}–${edge.openUntil}`, textContent: edge.edge });
      open.style.left = `${x(edge.openFrom)}%`;
      open.style.width = `${x(edge.openUntil + 1) - x(edge.openFrom)}%`;
      return [buffer, open];
    });
    this.#timeline.replaceChildren(...rowsHtml, row("Edges", windows));
  }

  #renderChart(): void {
    const inspection = this.#shown?.inspection;
    const regions = this.#graph.regions.map((region) => {
      const report = inspection?.regions.find((entry) => entry.region === region);
      const nodes = this.#graph.actions.filter((action) => action.region === region).map((action) => {
        const active = report?.action === action.id;
        const button = element("button", {
          type: "button",
          className: `lab-node${active ? " active" : ""}${this.#isSelected("action", action.id) ? " selected" : ""}`,
          textContent: active && report?.phase
            ? `${action.id}${report.ability ? ` (${report.ability})` : ""} · ${report.phase}${report.recoveryLeft !== null ? ` ${report.recoveryLeft}t` : ""}${report.activeLeft !== null ? ` ${report.activeLeft}t` : ""}`
            : action.id,
        });
        button.addEventListener("click", () => this.#select({ kind: "action", id: action.id }));
        return button;
      });
      return element("div", { className: "lab-region" }, element("h3", {}, region), ...nodes);
    });
    const edges = this.#graph.edges.map((edge) => {
      const report = inspection?.edges.find((entry) => entry.edge === edge.id);
      const status = report?.status.kind ?? "Blocked";
      const button = element("button", {
        type: "button",
        className: `lab-edge status-${status.toLowerCase()}${this.#isSelected("edge", edge.id) ? " selected" : ""}`,
        textContent: `${edge.from} —${edge.input}→ · ${status}`,
        title: report ? explainEdge(report) : edge.id,
      });
      button.addEventListener("click", () => this.#select({ kind: "edge", id: edge.id }));
      return button;
    });
    const rules = (inspection?.rules ?? []).map((rule) => element("li", {}, explainRule(rule)));
    const interruptions = (inspection?.interruptions ?? []).map((report) => element("li", {}, explainInterruption(report)));
    const tick = element("button", { type: "button", className: `lab-edge${this.#selection.kind === "tick" ? " selected" : ""}`, textContent: "Why is an input (not) legal?" });
    tick.addEventListener("click", () => this.#select({ kind: "tick" }));
    this.#chart.replaceChildren(
      element("div", { className: "lab-regions" }, ...regions),
      element("div", { className: "lab-edges" }, ...edges, tick),
      rules.length > 0 ? element("ul", { className: "lab-rules" }, ...rules) : element("p", { className: "lab-muted" }, "No compatibility rule applies."),
      ...(interruptions.length > 0 ? [element("ul", { className: "lab-rules" }, ...interruptions)] : []),
    );
  }

  #isSelected(kind: "action" | "edge", id: string): boolean {
    return this.#selection.kind === kind && this.#selection.id === id;
  }

  #renderInspector(viewer: LabViewer): void {
    const rows: [string, string][] = [];
    const inspection = this.#shown?.inspection;
    const selection = this.#selection;
    if (selection.kind === "action") {
      const action = this.#graph.actions.find((entry) => entry.id === selection.id);
      const report = inspection?.regions.find((entry) => entry.action === selection.id);
      rows.push(["Action", selection.id], ["Region", action?.region ?? "?"],
        ["Recovery", action?.recoveryMs === null || !action ? "none" : `${action.recoveryMs} ms · ${action.recoveryTicks} ticks`],
        ["Now", report
          ? `${report.phase ?? "active"}${report.ability ? ` (${report.ability})` : ""}${report.recoveryLeft !== null ? `, ${report.recoveryLeft} ticks left` : ""}${report.activeLeft !== null ? `, ${report.activeLeft} ticks left` : ""}`
          : "not running"]);
      for (const modifier of this.#graph.modifiers.filter((entry) => entry.action === selection.id)) {
        rows.push([`If ${modifier.condition}`, `${modifier.recoveryMs} ms · ${modifier.recoveryTicks} ticks`]);
      }
      for (const rule of this.#graph.rules.filter((entry) => entry.during === selection.id)) {
        rows.push([`Rule ${rule.id}`, `${rule.effect === "Cancel" ? `${rule.input} cancels it` : `holds ${rule.input}`}${rule.conditions.length ? ` if ${rule.conditions.join(", ")}` : ""}`]);
      }
      for (const rule of this.#graph.interruptions.filter((entry) => entry.interrupted === `Action(${selection.id})` || entry.reaction === selection.id)) {
        rows.push([`${rule.reaction} → ${rule.interrupted}`, `${rule.consequence}${rule.phase ? ` in ${rule.phase}` : ""}`]);
      }
    } else if (selection.kind === "edge") {
      const edge = this.#graph.edges.find((entry) => entry.id === selection.id);
      const report = inspection?.edges.find((entry) => entry.edge === selection.id);
      if (edge) {
        rows.push(["Edge", edge.id], ["From", edge.from], ["Input", edge.input], ["Anchor", edge.anchor],
          ["Opens after", `${edge.openAfterMs} ms · ${edge.openAfterTicks} ticks`],
          ["Actionable", `${edge.actionableMs} ms · ${edge.actionableTicks} ticks`],
          ["Buffer", `${edge.bufferMs} ms · ${edge.bufferTicks} ticks`],
          ["Conditions", edge.conditions.join(", ") || "none"], ["Effects", edge.effects.join(", ") || "none"],
          ["Precision", edge.precision ? `${edge.precision.afterOpenMs} ms after opening for ${edge.precision.lengthMs} ms: ${edge.precision.benefits.join(", ")}` : "none"],
          ["Priority", String(edge.priority)]);
        for (const modifier of this.#graph.windowModifiers.filter((entry) => entry.edge === edge.id)) {
          rows.push([`If ${modifier.conditions.join(", ")}`, `actionable ${modifier.actionableDeltaMs > 0 ? "+" : ""}${modifier.actionableDeltaMs} ms`]);
        }
      }
      if (report) {
        rows.push(["Status", explainEdge(report)], ["Anchor tick", String(report.anchorTick ?? "—")], ["Window from", report.origin]);
      }
    } else {
      for (const arbitration of inspection?.arbitrations ?? []) {
        rows.push([arbitration.input, `${explainOutcome(arbitration.outcome)}${arbitration.precise ? " (precise)" : ""}`]);
        for (const { candidate, verdict } of arbitration.candidates) {
          rows.push([`  ${candidate}`, explainVerdict(verdict)]);
        }
      }
      // Effective values and where each comes from (Atlas 5.7.6).
      for (const value of inspection?.effective ?? []) {
        rows.push([value.key, `${value.value} · ${value.origin}`]);
      }
      for (const entry of this.#graph.overrides) {
        rows.push([`Override for ${entry.scope}`, `${entry.key}: ${entry.value}`]);
      }
    }
    rows.push(["Level", String(viewer.level)], ["Health", `${viewer.health} / ${viewer.maxHealth}`],
      ["Resource", viewer.resource ?? "—"], ["Clip", viewer.clip ?? "procedural"]);
    const list = element("dl");
    for (const [name, value] of rows) { list.append(element("dt", {}, name), element("dd", {}, value)); }
    this.#inspector.replaceChildren(list);
  }

  get root(): HTMLElement {
    return this.#root;
  }
}
