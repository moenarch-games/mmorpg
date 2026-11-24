import { describe, expect, test } from "bun:test";
import { ingestedProjection, ProjectionDecoder, type ZoneEvent } from "../src/replication";
import { createLocalWorld } from "../src/world/local-world";
import { SharedUnits, appearanceCode } from "../src/world/unit-presenter";
import { expectValidMesh } from "./support/geometry";
import { testCharacters } from "./support/local-characters";
import { localZoneModule } from "./support/local-zone-module";
import { encodeSnapshot } from "./support/wire";

// The shared Rust unit presenter (#184) through the real WASM export; its models, poses and
// effects are tested in crates/mmorpg-presentation and the native client.
const wasm = await localZoneModule();

async function joined() {
  const world = createLocalWorld(wasm, testCharacters());
  await world.source.join();
  const projection = world.source.latestProjection()!;
  const units = new SharedUnits(world.scenery.unitPresenter(projection.viewerId));
  expect(units.push(ingestedProjection(projection)!, 0)).toBe(true);
  return { world, projection, units };
}

const look = { classId: "warden", sex: "female", headwear: "ironcrest-helm" } as const;
/** The residents with an authored preset of their own; guards and villagers vary by entity. */
const NAMED_RESIDENTS = new Set(["Marshal Elden Greywatch", "Innkeeper Bram Tolliver", "Farmer Osric Mill", "Brother Aldous", "Tanner Hilda Brook"]);

describe("the shared unit presenter in the browser", () => {
  test("draws every interpolated unit procedurally until its imported model is ready", async () => {
    const { world, projection, units } = await joined();
    const shown = world.source.shown();
    const frame = units.frame(shown, projection, { seconds: 0, dt: 0, reducedMotion: false, barYaw: 0, look });
    expect(frame.units.map((unit) => unit.identity).sort()).toEqual(projection.entities.map((e) => `unit-${e.kind}-${e.entityId}`).sort());
    expect(frame.imported).toEqual([]);
    const viewer = frame.units.find((unit) => unit.entity.kind === "player" && unit.entity.id === projection.viewerId)!;
    expect(viewer.body.length).toBeGreaterThan(5);
    expect(viewer.body.every((node) => node.id.startsWith(`${viewer.identity}-`))).toBe(true);
    for (const node of frame.nodes) {
      if (node.geometry.kind === "mesh") { expectValidMesh(node.geometry, node.id); }
    }
    // The next frame reuses every geometry the page already holds.
    const next = units.frame(shown, projection, { seconds: 0.1, dt: 0.1, reducedMotion: false, barYaw: 0, look });
    const keys = new Map(frame.nodes.map((node) => [node.geometry.kind === "mesh" ? node.geometry.resourceKey : "", node.geometry]));
    for (const node of next.nodes) {
      if (node.geometry.kind === "mesh" && keys.has(node.geometry.resourceKey)) {
        expect(node.geometry).toBe(keys.get(node.geometry.resourceKey)!);
      }
    }
  });

  test("keeps each draw's renderer node and updates it in place from the typed layout (#1057)", async () => {
    const { world, projection, units } = await joined();
    const shown = world.source.shown();
    const options = { dt: 0.1, reducedMotion: false, barYaw: 0, look } as const;
    const first = units.frame(shown, projection, { ...options, seconds: 0 });
    const nodes = new Map(first.nodes.map((node) => [node.id, node]));
    const next = units.frame(shown, projection, { ...options, seconds: 0.5 });
    expect(next.nodes.length).toBeGreaterThan(0);
    for (const node of next.nodes) {
      if (nodes.has(node.id)) { expect(node).toBe(nodes.get(node.id)!); }
      // The renderer requires a plain 16-number array.
      expect(Array.isArray(node.modelMatrix) && node.modelMatrix.length === 16).toBe(true);
      expect(node.id.startsWith("fx-")).toBe(false);
    }
    // Each presented unit carries its record in the newest projection, and is found by entity without a search.
    for (const unit of next.units) {
      expect(unit.state).not.toBeNull();
      expect(units.unitOf(unit.entity)).toBe(unit);
    }
    expect(units.unitOf({ kind: "player", id: projection.viewerId })?.state?.entityId).toBe(projection.viewerId);
  });

  test("a ready model is requested with its chosen clip, sword and shield and headwear", async () => {
    const { world, projection, units } = await joined();
    units.modelReady("Ranger", [["Idle_Loop", 2], ["Walk_Loop", 1], ["Jog_Fwd_Loop", 1], ["Sword_Attack", 1], ["Death01", 1], ["Idle_Shield_Loop", 1]]);
    const frame = units.frame(world.source.shown(), projection, { seconds: 0, dt: 0, reducedMotion: false, barYaw: 0, look });
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
    const { world, projection, units } = await joined();
    units.modelReady("Ranger", [["Idle_Loop", 2], ["Walk_Loop", 1], ["Jog_Fwd_Loop", 1], ["Sword_Attack", 1], ["Death01", 1], ["Idle_Shield_Loop", 1]]);
    units.residentsReady();
    const frame = units.frame(world.source.shown(), projection, { seconds: 0, dt: 0, reducedMotion: false, barYaw: 0, look });
    const residents = frame.imported.filter((unit) => unit.entity.kind === "npc");
    expect(residents.length).toBeGreaterThan(0);
    for (const resident of residents) {
      expect(resident.kind).toBe("Ranger");
      expect(resident.resident).toMatch(/^resident\./);
      expect(resident.headwear).toBe(false);
      expect(resident.loadout).toBe(resident.resident === "resident.guard" ? "sword" : null);
      // Each guard and villager (#819) is their own person (asset-tooling #380); the named residents keep their presets.
      const named = NAMED_RESIDENTS.has(world.catalog.npcs.get(resident.entity.id)?.name ?? "");
      expect(resident.variety).toBe(named ? null : resident.entity.id);
      // Tanner Hilda Brook is drawn on the female figure (#493), everyone else on the male.
      expect(resident.figure).toBe(resident.resident === "resident.tanner" ? "humanoid.ranger.female" : "humanoid.ranger.male");
    }
    expect(residents.some((resident) => resident.resident === "resident.tanner")).toBe(true);
  });

  test("an attack press winds the viewer up until the zone answers, and abandoning lets it go (Atlas 6.6.10)", async () => {
    const { world, projection, units } = await joined();
    units.modelReady("Ranger", [["Idle_Loop", 2], ["Walk_Loop", 1], ["Jog_Fwd_Loop", 1], ["Sword_Attack", 1.5], ["Death01", 1], ["Idle_Shield_Loop", 1]]);
    const viewerAt = (seconds: number) => units.frame(world.source.shown(), projection, { seconds, dt: 0.1, reducedMotion: false, barYaw: 0, look })
      .imported.find((unit) => unit.entity.id === projection.viewerId)!;
    viewerAt(0);
    units.prepare(1, 0);
    units.preparedSent(0);
    // The arms wind up at once and hold short of the swing; the legs keep standing.
    viewerAt(0.1);
    expect(viewerAt(0.2).layers.some((layer) => layer.clip === "Sword_Attack" && layer.mask === "upper")).toBe(true);
    expect(viewerAt(0.5).lower[0]).toBe("Idle_Loop");
    units.abandonPreparation();
    for (let step = 1; step <= 10; step += 1) { viewerAt(0.5 + step * 0.1); }
    expect(viewerAt(1.6).layers.every((layer) => layer.clip === "Idle_Loop")).toBe(true);
  });

  test("queues each new projection's confirmed hits and blocks for the page, once (#1058)", async () => {
    const { projection, units } = await joined();
    const wolf = { kind: "creature", id: 9 } as const;
    const me = { kind: "player", id: projection.viewerId } as const;
    const events: ZoneEvent[] = [
      { kind: "damage-dealt", source: me, target: wolf, amount: 5, critical: false },
      { kind: "miss", source: me, target: wolf },
      { kind: "blocked", source: wolf, target: me, lane: "high", amount: 0 },
    ];
    const decoder = new ProjectionDecoder();
    // The strict test encoder refuses what a host's packing would leave for a later tick: the
    // events need the room of the farther units, so the struck projection keeps only the viewer.
    const struck = decoder.decode(encodeSnapshot({ ...projection, tick: projection.tick + 1n, events, entities: projection.entities.slice(0, 1) }));
    units.push(ingestedProjection(struck)!, 0);
    expect(units.takeImpacts()).toEqual([{ family: "puff", target: wolf }, { family: "ring", target: me }]);
    expect(units.takeImpacts()).toEqual([]);
    // A projection pushed again admits nothing twice.
    units.push(ingestedProjection(struck)!, 0);
    expect(units.takeImpacts()).toEqual([]);
  });

  test("the viewer look maps to the appearance code the presenter reads", () => {
    expect([appearanceCode({ classId: "warden", sex: "female" }), appearanceCode({ classId: "arcanist", sex: "male" }),
      appearanceCode({ classId: null, sex: "female" })]).toEqual([1, 6, 0]);
  });
});
