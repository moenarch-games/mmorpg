import { describe, expect, test } from "bun:test";
import { projectionBytes } from "../src/replication";
import { createLocalWorld } from "../src/world/local-world";
import { SharedUnits, appearanceCode } from "../src/world/unit-presenter";
import { expectValidMesh } from "./support/geometry";
import { testCharacters } from "./support/local-characters";
import { localZoneModule } from "./support/local-zone-module";

// The shared Rust unit presenter (#184) through the real WASM export; its models, poses and
// effects are tested in crates/mmorpg-presentation and the native client.
const wasm = await localZoneModule();

async function joined() {
  const world = createLocalWorld(wasm, testCharacters());
  await world.source.join();
  const projection = world.source.latestProjection()!;
  const units = new SharedUnits(world.scenery.unitPresenter(projection.viewerId));
  units.push(projectionBytes(projection)!, 0);
  return { world, projection, units };
}

const look = { classId: "warden", sex: "female", headwear: "ironcrest-helm" } as const;

describe("the shared unit presenter in the browser", () => {
  test("draws every sampled unit procedurally until its imported model is ready", async () => {
    const { world, projection, units } = await joined();
    const sampled = world.source.sample();
    const frame = units.frame(sampled, { seconds: 0, dt: 0, reducedMotion: false, barYaw: 0, look });
    expect(frame.units.map((unit) => unit.identity).sort()).toEqual(sampled.map((e) => `unit-${e.kind}-${e.entityId}`).sort());
    expect(frame.imported).toEqual([]);
    const viewer = frame.units.find((unit) => unit.entity.kind === "player" && unit.entity.id === projection.viewerId)!;
    expect(viewer.body.length).toBeGreaterThan(5);
    expect(viewer.body.every((node) => node.id.startsWith(`${viewer.identity}-`))).toBe(true);
    for (const node of frame.nodes) {
      if (node.geometry.kind === "mesh") { expectValidMesh(node.geometry, node.id); }
    }
    // The next frame reuses every geometry the page already holds.
    const next = units.frame(sampled, { seconds: 0.1, dt: 0.1, reducedMotion: false, barYaw: 0, look });
    const keys = new Map(frame.nodes.map((node) => [node.geometry.kind === "mesh" ? node.geometry.resourceKey : "", node.geometry]));
    for (const node of next.nodes) {
      if (node.geometry.kind === "mesh" && keys.has(node.geometry.resourceKey)) {
        expect(node.geometry).toBe(keys.get(node.geometry.resourceKey)!);
      }
    }
  });

  test("a ready model is requested with its chosen clip, sword and shield and headwear", async () => {
    const { world, projection, units } = await joined();
    units.modelReady("Ranger", [["Idle_Loop", 2], ["Walk_Loop", 1], ["Jog_Fwd_Loop", 1], ["Sword_Attack", 1], ["Death01", 1], ["Idle_Shield_Loop", 1]]);
    const frame = units.frame(world.source.sample(), { seconds: 0, dt: 0, reducedMotion: false, barYaw: 0, look });
    const viewer = frame.imported.find((unit) => unit.entity.id === projection.viewerId)!;
    // The imported model itself wears the viewer's headwear (#374); no procedural hat is drawn.
    expect(viewer).toMatchObject({ kind: "Ranger", clip: "Idle_Loop", loadout: "sword-and-shield", resident: null, headwear: true, tint: null });
    // The viewer wears the Warden's starter outfit (#379), no drop equipped yet.
    expect(viewer.outfit).toEqual({ preset: "class.warden", replaced: [] });
    // Standing idle, the whole body plays one clip (#384).
    expect(viewer.layers).toEqual([{ clip: "Idle_Loop", time: 0, weight: 1, mask: "all" }]);
    expect(viewer.lower).toEqual(["Idle_Loop", 0]);
    // Residents stay procedural until their presets loaded.
    expect(frame.imported.some((unit) => unit.entity.kind === "npc")).toBe(false);
  });

  test("named residents become Rangers in their presets once those loaded (#418)", async () => {
    const { world, units } = await joined();
    units.modelReady("Ranger", [["Idle_Loop", 2], ["Walk_Loop", 1], ["Jog_Fwd_Loop", 1], ["Sword_Attack", 1], ["Death01", 1], ["Idle_Shield_Loop", 1]]);
    units.residentsReady();
    const frame = units.frame(world.source.sample(), { seconds: 0, dt: 0, reducedMotion: false, barYaw: 0, look });
    const residents = frame.imported.filter((unit) => unit.entity.kind === "npc");
    expect(residents.length).toBeGreaterThan(0);
    for (const resident of residents) {
      expect(resident.kind).toBe("Ranger");
      expect(resident.resident).toMatch(/^resident\./);
      expect(resident.headwear).toBe(false);
      expect(resident.loadout).toBe(resident.resident === "resident.guard" ? "sword" : null);
    }
  });

  test("the viewer look maps to the appearance code the presenter reads", () => {
    expect([appearanceCode({ classId: "warden", sex: "female" }), appearanceCode({ classId: "arcanist", sex: "male" }),
      appearanceCode({ classId: null, sex: "female" })]).toEqual([1, 6, 0]);
  });
});
