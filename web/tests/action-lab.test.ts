import { describe, expect, test } from "bun:test";
import { TICK_HZ } from "../src/replication";
import {
  describeAnimation, explainEdge, explainOutcome, explainRule, labQuery, labSetupFromQuery, parseBlend, timelineSegments, type ActionGraph,
  type Inspection,
} from "../src/world/action-lab";
import { scenarioChoice, ScenarioSource } from "../src/world/scenario-source";
import { localZoneModule } from "./support/local-zone-module";

// Builds crates/mmorpg-wasm for wasm32 and runs wasm-bindgen, like `bun run build`.
const wasm = await localZoneModule();
const TICK_SECONDS = 1 / TICK_HZ;

describe("Action Lab (#381)", () => {
  test("a setup survives the URL round trip a reset uses", () => {
    const setup = { className: "ranger", level: 7, gear: "none" as const, mainHand: 2, resource: 30 };
    const query = labQuery(setup, 5n);
    expect(labSetupFromQuery(query)).toEqual(setup);
    expect(scenarioChoice(query)).toEqual({ name: "action-lab", seed: 5n, lab: setup });
    expect(scenarioChoice("?scenario=ogre-combat")?.lab).toBeNull();
    expect(() => labSetupFromQuery("?scenario=action-lab&gear=gold")).toThrow();
  });

  test("the real lab zone pauses, steps and scrubs its inspected ticks", () => {
    const create = () => wasm.LocalZone.forActionLab("warden", 6, "standard", 0, 60, 0n);
    const source = new ScenarioSource(create);
    void source.join();
    for (let tick = 0; tick < 10; tick += 1) { source.advance(TICK_SECONDS); }
    const live = source.frames.at(-1)!;
    expect(live.tick).toBe(10n);
    const inspection = JSON.parse(live.inspection!) as Inspection;
    expect(inspection.tick).toBe(10);
    expect(inspection.regions.map((region) => region.region)).toEqual(["Movement", "Offense", "Defense", "Reaction"]);
    expect(explainEdge(inspection.edges[0]!)).toBe("Blocked: its source action is not running.");
    expect(inspection.arbitrations.map((entry) => explainOutcome(entry.outcome))).toContain("resolves now as its ordinary use");

    source.setPaused(true);
    source.advance(1);
    expect(source.frames.at(-1)!.tick).toBe(10n);
    source.step();
    source.advance(0);
    expect(source.frames.at(-1)!.tick).toBe(11n);

    source.scrubTo(4n);
    expect(source.shownFrame()?.tick).toBe(4n);
    expect(source.sample()).toBe(source.shownFrame()!.snapshot.entities);
    source.setPaused(false);
    expect(source.scrubTick).toBeNull();
    expect(() => wasm.LocalZone.forActionLab("warden", 0, "standard", 0, undefined, 0n)).toThrow();
    const graph = JSON.parse(wasm.actionGraph()) as ActionGraph;
    expect(graph.edges[0]?.id).toBe("SlashToShieldBash");
    source.leave();
  });

  test("every class runs the shared graph, whose values all come from the baseline", () => {
    const graph = JSON.parse(wasm.actionGraph()) as ActionGraph;
    expect(graph.overrides).toEqual([]);
    expect(graph.rules.filter((rule) => rule.effect === "Cancel").map((rule) => rule.input)).toEqual(["Move", "Jump"]);
    const source = new ScenarioSource(() => wasm.LocalZone.forActionLab("arcanist", 8, "standard", 0, undefined, 0n));
    void source.join();
    source.advance(TICK_SECONDS);
    const inspection = JSON.parse(source.frames.at(-1)!.inspection!) as Inspection;
    expect(inspection.effective.length).toBeGreaterThan(0);
    expect(inspection.effective.every((value) => value.origin === "baseline")).toBe(true);
    expect(inspection.arbitrations.map((entry) => entry.input)).toEqual(["WardenMelee", "ShieldBash", "GuardProtection", "Ability", "Move", "Jump"]);
    source.leave();
    expect(explainOutcome({ kind: "Cancel", rule: "MovementCancelsCast" })).toBe("resolves now and ends the running action (rule MovementCancelsCast)");
    expect(explainRule({ rule: "MovementCancelsCast", input: "Move", effect: "Cancel", releasedAt: 140, origin: "baseline" }))
      .toBe("MovementCancelsCast: Move ends the running action before tick 140");
    expect(explainRule({ rule: "MovementCancelsCast", input: "Jump", effect: "Cancel", releasedAt: 9, origin: "ability Firebolt" }))
      .toBe("MovementCancelsCast: Jump ends the running action before tick 9 (ability Firebolt)");
  });

  test("timeline segments merge consecutive ticks of one action and phase", () => {
    const at = (tick: number, action: string | null, phase: string | null): { tick: number; inspection: Inspection } => ({
      tick,
      inspection: { tick, regions: [{ region: "Movement", action, phase, recoveryLeft: null }], edges: [], rules: [], interruptions: [], arbitrations: [] },
    });
    const rows = timelineSegments([at(1, "Airborne", "Active"), at(2, "Airborne", "Active"), at(3, "Landing", "Recovery"), at(4, null, null)], ["Movement"]);
    expect(rows.get("Movement")).toEqual([
      { action: "Airborne", phase: "Active", from: 1, to: 2 },
      { action: "Landing", phase: "Recovery", from: 3, to: 3 },
    ]);
  });
});

describe("Action Lab animation (#384)", () => {
  test("the readout names each region's layers and the blend inputs parse exactly", () => {
    expect(describeAnimation(null)).toBe("procedural");
    expect(describeAnimation({
      upper: ["Sword_Attack", 0.2], lower: ["Jump_Land", 0.1],
      layers: [
        { clip: "Jump_Land", time: 0.1, weight: 0.625, mask: "lower" },
        { clip: "Jump_Loop", time: 1, weight: 0.375, mask: "lower" },
        { clip: "Sword_Attack", time: 0.2, weight: 1, mask: "upper" },
      ],
    })).toBe("legs Jump_Land 0.63 + Jump_Loop 0.38 · upper body Sword_Attack 1.00");
    expect(parseBlend(["0.2", "0.05", "1", "0.5"])).toEqual({ lowerSeconds: 0.2, upperSeconds: 0.05, lowerWeight: 1, upperWeight: 0.5 });
    expect(parseBlend(["", "0.1", "1", "1"])).toBeNull();
    expect(parseBlend(["0.1", "2", "1", "1"])).toBeNull();
  });
});
