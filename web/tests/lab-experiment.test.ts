import { describe, expect, test } from "bun:test";
import { PROTOCOL_ID } from "../src/protocol-id";
import { TICK_HZ } from "../src/replication";
import {
  copyConfig, importFile, exportBundle, insertCopy, LabLibrary, moveCommand, movementRule, PROMOTION_FORMAT, promotionFileName,
  promotionRequest, RECORD_FORMAT, removeAuthored, removeCommand, removeEdge, ruleApplies, setAuthored, setMovementRule,
  setRuleApplies, slidingSlashSample, swimmingSample, traversalSample, SCENARIO_FORMAT,
  type ConfigCheck, type LabComparison, type LabConfig, type LabRun, type LabScenario,
} from "../src/world/lab-experiment";
import { ScenarioSource, type ScenarioZoneHandle } from "../src/world/scenario-source";
import { localZoneModule } from "./support/local-zone-module";

type LabModule = {
  LocalZone: { forActionLab(...args: unknown[]): ScenarioZoneHandle; new (): { contentRevision(): bigint; free(): void } };
  actionLabBaseline(): string;
  actionLabCheck(config: string): string;
  actionLabRun(scenario: string, config: string): string;
  actionLabCompare(scenario: string, a: string, b: string): string;
  actionLabChanges(from: string, to: string): string;
};

// Builds crates/mmorpg-wasm for wasm32 and runs wasm-bindgen, like `bun run build`.
const wasm = (await localZoneModule()) as unknown as LabModule;
const TICK_SECONDS = 1 / TICK_HZ;
const SETUP = { className: "warden", level: 6, gear: "standard" as const, mainHand: 0, resource: 60 };

const zone = (config: LabConfig | null) => () => wasm.LocalZone.forActionLab("warden", 6, "standard", 0, 60, 0n, config ? JSON.stringify(config) : undefined);
const baseline = () => JSON.parse(wasm.actionLabBaseline()) as LabConfig;
const check = (config: LabConfig) => JSON.parse(wasm.actionLabCheck(JSON.stringify(config))) as ConfigCheck;

/** Plays a jump and an attack in the real lab and keeps them as a scenario. */
function record(): LabScenario {
  const source = new ScenarioSource(zone(null));
  void source.join();
  for (let tick = 0; tick < 60; tick += 1) {
    if (tick === 3) { source.sendCommand({ kind: "jump" }); }
    if (tick === 20) { source.sendCommand({ kind: "start-attack" }); }
    source.advance(TICK_SECONDS);
  }
  const recording = source.recording();
  source.leave();
  const revision = new wasm.LocalZone();
  const contentRevision = String(revision.contentRevision());
  revision.free();
  return {
    format: SCENARIO_FORMAT, name: "jump and swing", setup: SETUP, seed: "0", contentRevision, protocolId: PROTOCOL_ID,
    ticks: recording.ticks, commands: [...recording.commands],
    recorded: { config: check(baseline()).identity!, digest: recording.digest! },
  };
}

describe("Action Lab experiments (#382)", () => {
  test("a recorded scenario replays deterministically and reproduces its recording", async () => {
    const scenario = record();
    expect(scenario.commands.map((command) => command.tick)).toEqual([3, 20]);
    const run = JSON.parse(wasm.actionLabRun(JSON.stringify(scenario), JSON.stringify(baseline()))) as LabRun;
    expect(run.baseline).toBe(true);
    expect(run.ticks).toHaveLength(60);
    expect(run.reproducesRecording).toBe(true);
    expect(run.commands[0]!.label).toStartWith("Jump");
    // The arena replay reaches the same canonical state as the headless run.
    const source = new ScenarioSource(zone(null));
    await source.replay(scenario.commands);
    expect(source.sendCommand({ kind: "jump" })).toBeNull();
    for (let tick = 0; tick < 60; tick += 1) { source.advance(TICK_SECONDS); }
    expect(source.recording().digest).toBe(run.summary.finalDigest);
    source.leave();
  }, 30_000);

  test("edits move, copy and remove commands, and equal rules never diverge", () => {
    const scenario = record();
    const moved = moveCommand(scenario, 0, 8);
    expect(moved.commands.map((command) => command.tick)).toEqual([8, 20]);
    expect(moved.recorded).toBeNull();
    expect(insertCopy(scenario, 1, 99).commands.map((command) => command.tick)).toEqual([3, 20, 59]);
    expect(removeCommand(scenario, 0).commands).toHaveLength(1);
    const same = JSON.parse(wasm.actionLabCompare(JSON.stringify(scenario), JSON.stringify(baseline()), JSON.stringify(baseline()))) as LabComparison;
    expect(same.divergesAt).toBeNull();
    const a = JSON.parse(wasm.actionLabRun(JSON.stringify(scenario), JSON.stringify(baseline()))) as LabRun;
    const b = JSON.parse(wasm.actionLabRun(JSON.stringify(moved), JSON.stringify(baseline()))) as LabRun;
    expect(a.summary.finalDigest).not.toBe(b.summary.finalDigest);
  }, 30_000);

  test("experimental configurations are validated, isolated and exportable", () => {
    const production = baseline();
    expect(check(production)).toMatchObject({ baseline: true, error: null });
    const longer = copyConfig(production, "Longer bash window");
    longer.edges[0]!.actionableMs += 300;
    const result = check(longer);
    expect(result.baseline).toBe(false);
    expect(result.identity).not.toBe(check(production).identity);
    expect(production.edges[0]!.actionableMs).toBe(baseline().edges[0]!.actionableMs);
    const broken = copyConfig(production, "Broken");
    broken.edges[0]!.bufferMs = broken.edges[0]!.openAfterMs + 1_000;
    expect(check(broken).error).toContain("BufferBeforeAnchor");
    expect(() => zone(broken)()).toThrow();
    // A lab world under the experiment reports the experimental values; production keeps its own.
    const effectiveWindow = (config: LabConfig | null) => {
      const lab = zone(config)();
      lab.tick();
      const inspection = JSON.parse(lab.actionInspection!(lab.scenarioPlayer!)!) as { effective: { key: string; value: string }[] };
      lab.free();
      return inspection.effective.find((entry) => entry.key === "Window(SlashToShieldBash)")?.value;
    };
    expect(effectiveWindow(longer)).toContain(`actionable ${longer.edges[0]!.actionableMs} ms`);
    expect(effectiveWindow(null)).toContain(`actionable ${production.edges[0]!.actionableMs} ms`);
    const noHold = setRuleApplies(production, "LandingHoldsMelee", "warden", false);
    expect(ruleApplies(noHold, "LandingHoldsMelee", "warden")).toBe(false);
    expect(check(noHold).error).toBeNull();
    expect(ruleApplies(setRuleApplies(noHold, "LandingHoldsMelee", "warden", true), "LandingHoldsMelee", "warden")).toBe(true);
    const imported = importFile(exportBundle({ scenario: null, a: production, b: longer }));
    expect(imported.b).toEqual(longer);
    expect(() => importFile("{\"format\":\"other\"}")).toThrow();
  }, 30_000);

  test("authored momentum carries a released running jump; landing edges compose (#413)", () => {
    const production = baseline();
    expect(production.movement ?? []).toEqual([]);
    const sample = traversalSample(copyConfig(production, "Traversal sample"));
    expect(check(sample)).toMatchObject({ baseline: false, error: null });
    expect(sample.edges.map((edge) => edge.id)).toEqual(["SlashToShieldBash", "LandingToMelee", "LandingToShieldBash"]);
    expect(movementRule(sample, "Airborne", "Active")).toMatchObject({ steeringPercent: 100, momentumPercent: 100, maxAccelerationCms2: 600 });
    expect(removeEdge(sample, "LandingToMelee").edges).toHaveLength(2);
    expect(check(setMovementRule(sample, "Airborne", "Active", null)).error).toBeNull();
    const late = copyConfig(sample, "Late");
    late.edges[1]!.openAfterMs = 600;
    expect(check(late).error).toContain("OutlastsSource");
    // Run east, jump, let go: production stops at once, the sample flies on its momentum.
    const source = new ScenarioSource(zone(null));
    void source.join();
    for (let tick = 0; tick < 50; tick += 1) {
      if (tick === 1) { source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: 16_384 }); }
      if (tick === 20) { source.sendCommand({ kind: "jump" }); }
      if (tick === 22) { source.sendCommand({ kind: "move", forward: 0, strafe: 0, facing: 16_384 }); }
      source.advance(TICK_SECONDS);
    }
    const recording = source.recording();
    source.leave();
    const scenario: LabScenario = { ...record(), name: "running jump", ticks: recording.ticks, commands: [...recording.commands], recorded: null };
    const comparison = JSON.parse(wasm.actionLabCompare(JSON.stringify(scenario), JSON.stringify(production), JSON.stringify(sample))) as LabComparison;
    expect(comparison.divergesAt).not.toBeNull();
    const at = (run: LabRun, tick: number) => run.ticks.find((entry) => entry.tick === tick)!;
    expect(at(comparison.a, 30).speed).toBe(0);
    expect(at(comparison.b, 30).speed).toBeGreaterThan(300);
    expect(at(comparison.b, 30).regions[0]).toBe("Airborne·Active");
    expect(comparison.differences.length).toBeGreaterThan(0);
  }, 30_000);

  test("the swimming sample authors water refusals and holds; production authors none (#413)", () => {
    const production = baseline();
    expect(production.rules.filter((rule) => rule.during === "Swim")).toEqual([]);
    const sample = swimmingSample(copyConfig(production, "Swimming sample"));
    expect(check(sample)).toMatchObject({ baseline: false, error: null });
    expect(sample.rules.filter((rule) => rule.during === "Swim").map((rule) => `${rule.effect} ${rule.input}`))
      .toEqual(["Refuse WardenMelee", "Refuse ShieldBash", "Hold Ability"]);
    expect(movementRule(sample, "Swim", "Active")).toMatchObject({ momentumPercent: 100, maxAccelerationCms2: 400 });
    // Applying it twice changes nothing; a refusal of movement is not supported.
    expect(swimmingSample(sample).rules).toEqual(sample.rules);
    const moving = copyConfig(sample, "Refuse movement");
    moving.rules.push({ id: "SwimmingRefuses", during: "Swim", input: "Jump", conditions: [], effect: "Refuse" });
    expect(check(moving).error).not.toBeNull();
    expect(check(setRuleApplies(sample, "SwimmingRefuses", "warden", false)).error).toBeNull();
  });

  test("a new authored action validates, names unsupported primitives and replays (#414)", () => {
    const production = baseline();
    expect(production.authored ?? []).toEqual([]);
    const sample = slidingSlashSample(copyConfig(production, "Sliding slash"));
    expect(check(sample)).toMatchObject({ baseline: false, error: null });
    expect(sample.authored).toHaveLength(1);
    expect(sample.edges.map((edge) => edge.id)).toEqual(["SlashToShieldBash", "Authored(0)"]);
    const jumping = setAuthored(sample, { ...sample.authored![0]!, input: "Jump", ability: null });
    expect(check(jumping).error).toContain("BadAuthored(0, Input)");
    expect(removeAuthored(sample, 0).edges.map((edge) => edge.id)).toEqual(["SlashToShieldBash"]);
    // The authored action rides along in a saved version and its promotion request (#383).
    const storage = new Map<string, string>();
    const library = new LabLibrary({ getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) } as unknown as Storage);
    const saved = library.saveConfig(sample, check(sample).identity!);
    expect(saved.config.authored).toEqual(sample.authored);
    const reloaded = library.configs.find((entry) => entry.version === saved.version)!;
    expect(check(reloaded.config).identity).toBe(check(sample).identity);
    const request = promotionRequest(reloaded, check(production).identity!, record(), "sliding slash");
    expect(request.config.authored).toEqual(sample.authored);
    expect(check(request.config).identity).toBe(check(sample).identity);
    // Walk west at the wolves and strike: the sample winds up, then the ordinary strike resolves.
    const source = new ScenarioSource(zone(null));
    void source.join();
    for (let tick = 0; tick < 60; tick += 1) {
      if (tick === 1) { source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: 49_152 }); }
      if (tick === 10) {
        source.sendCommand({ kind: "directed-strike", ability: 1, facing: 49_152, aim: { side: "centre", height: "low" } });
      }
      source.advance(TICK_SECONDS);
    }
    const recording = source.recording();
    source.leave();
    const scenario: LabScenario = { ...record(), name: "sliding slash", ticks: recording.ticks, commands: [...recording.commands], recorded: null };
    const comparison = JSON.parse(wasm.actionLabCompare(JSON.stringify(scenario), JSON.stringify(production), JSON.stringify(sample))) as LabComparison;
    expect(comparison.divergesAt).not.toBeNull();
    expect(comparison.b.ticks.some((tick) => tick.regions.some((region) => region.startsWith("Authored(0)·Startup")))).toBe(true);
  }, 30_000);
});

describe("Action Lab diagnostics and promotion (#383)", () => {
  test("runs carry explained findings, and only an explicitly chosen saved version is promoted", () => {
    const scenario = record();
    const production = baseline();
    const comparison = JSON.parse(wasm.actionLabCompare(JSON.stringify(scenario), JSON.stringify(production), JSON.stringify(production))) as LabComparison;
    // The jump-and-swing scenario never runs the directed slash: its combo edge is not exercised.
    expect(comparison.a.findings.some((finding) => finding.kind === "unreachableTransition" && finding.severity === "note")).toBe(true);
    expect(comparison.regressions).toEqual([]);
    expect(comparison.a.commands.find((command) => command.label.startsWith("Jump"))?.decision?.input).toBe("Jump");

    const storage = new Map<string, string>();
    const library = new LabLibrary({ getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) } as unknown as Storage);
    const longer = copyConfig(production, "Longer bash window");
    longer.edges[0]!.actionableMs = 800;
    const first = library.saveConfig(longer, check(longer).identity!);
    expect(library.saveConfig(longer, check(longer).identity!)).toEqual(first);
    longer.edges[0]!.actionableMs = 900;
    const second = library.saveConfig(longer, check(longer).identity!);
    expect([first.version, second.version]).toEqual([1, 2]);
    expect(library.configs.map((entry) => entry.config.edges[0]!.actionableMs)).toEqual([800, 900]);

    const request = promotionRequest(first, check(production).identity!, scenario, "wider window");
    expect(request).toMatchObject({ format: PROMOTION_FORMAT, version: 1, config: { edges: [{ actionableMs: 800 }] } });
    expect(promotionFileName(first)).toBe("promote-longer-bash-window-v1.json");
    expect(JSON.parse(wasm.actionLabChanges(JSON.stringify(production), JSON.stringify(first.config))) as string[])
      .toEqual(["edges[0].actionableMs: 600 → 800"]);
    // A checked-in record imports as A (replaced) / B (promoted) for comparison and rollback.
    const imported = importFile(JSON.stringify({
      format: RECORD_FORMAT, name: first.name, version: 1, identity: first.identity, note: "", previous: production,
      promoted: first.config, scenario, divergesAt: null,
    }));
    expect(imported.a).toEqual(production);
    expect(imported.b).toEqual(first.config);
  }, 30_000);
});
