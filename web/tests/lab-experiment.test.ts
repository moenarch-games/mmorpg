import { describe, expect, test } from "bun:test";
import { PROTOCOL_ID } from "../src/protocol-id";
import { TICK_HZ } from "../src/replication";
import {
  copyConfig, importFile, exportBundle, insertCopy, moveCommand, removeCommand, ruleApplies, setRuleApplies, SCENARIO_FORMAT,
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
});
