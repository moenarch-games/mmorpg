import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { GRASS_PACKAGE_DIRECTORY, record } from "../scripts/grass-package";
import { RELIEF_GRID, RELIEF_PACKAGE_DIRECTORY } from "../scripts/relief-package";
import { encodeCommand } from "../src/command-wire";
import { ABILITY_SHAPES, ITEM_SHAPES, decodeSnapshot, type EntityState } from "../src/replication";
import { abilitySlots, useAbilitySlot } from "../src/world/units/abilities";
import { undescribedAbilities } from "../src/world/units/ability-presentation";
import { combatStatus } from "../src/world/units/combat-hud";
import { BagState } from "../src/world/units/bag-state";
import { LootState } from "../src/world/units/loot-state";
import { dialogNpc, dialogView, QuestState, trackerLines } from "../src/world/units/quest-state";
import { createLocalWorld } from "../src/world/local-world";
import { LocalZoneSource, type LocalZoneHandle } from "../src/world/local-zone-source";
import { buildSceneryScene } from "../src/world/scenery-nodes";
import { MemoryLocks, MemoryStorage, testCharacters } from "./support/local-characters";
import { localZoneModule } from "./support/local-zone-module";
import { immediate, worldSourceContract } from "./support/world-source-contract";

// Builds crates/mmorpg-wasm for wasm32 and runs wasm-bindgen, like `bun run build`.
const wasm = await localZoneModule();
const TICK_SECONDS = 1 / 30;
const EAST = 16_384;
const RUN_UNITS_PER_TICK = 21;

function self(source: LocalZoneSource): EntityState {
  const projection = source.latestProjection();
  const entity = projection?.entities.find((candidate) => candidate.kind === "player" && candidate.entityId === projection.viewerId);
  if (!entity) {
    throw new Error("The viewer is missing from its projection");
  }
  return entity;
}

function run(source: LocalZoneSource, ticks: number): void {
  for (let tick = 0; tick < ticks; tick += 1) {
    source.advance(TICK_SECONDS);
  }
}

describe("WASM local zone host", () => {
  test("a real Greyhaven death publishes fenced loot, commits once and recovers dropped economic projections", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    await source.join();
    const bag = new BagState();
    const loot = new LootState();
    bag.update(source.latestProjection()!);
    loot.reset(source.latestProjection());
    loot.update(source.latestProjection()!);
    const movement = new Map([[0, [1, 0]], [36, [1, 49152]], [121, [1, 32768]],
      [131, [1, 49152]], [375, [1, 32768]], [395, [0, 32768]]]);
    for (let tick = 0; tick < 972; tick += 1) {
      const move = movement.get(tick);
      if (move) {
        source.sendCommand({ kind: "move", forward: move[0] === 0 ? 0 : 1, strafe: 0, facing: move[1] ?? 0 });
      }
      if (tick === 1) {
        source.sendCommand({ kind: "start-attack" });
      }
      if (tick === 380) {
        source.sendCommand({ kind: "select-target", target: { kind: "creature", id: 108 } });
        source.sendCommand({ kind: "start-attack" });
      }
      run(source, 1);
    }
    const before = source.latestProjection()!;
    expect(before.viewer).toMatchObject({ copper: 0, health: 30, experience: 50 });
    loot.update(before);
    const sheet = loot.sheet;
    if (sheet === null) {
      throw new Error("Missing authoritative corpse sheet");
    }
    expect(sheet).toEqual({ creatureId: 108, diedAt: 915n, money: 2, item: { itemId: 1, quantity: 2 }, questItem: null });
    source.sendCommand({ kind: "loot", creatureId: sheet.creatureId, diedAt: sheet.diedAt + 1n });
    run(source, 1);
    expect(source.latestProjection()?.events).toContainEqual({ kind: "error", code: "invalid-loot", target: { kind: "creature", id: 108 } });
    expect(source.latestProjection()?.loot).toEqual(sheet);
    loot.update(source.latestProjection()!);
    const claim = loot.claimIntent();
    if (claim === null) {
      throw new Error("The projected corpse did not produce a claim intent");
    }
    const command = claim(source.latestProjection()!);
    if (command === null) {
      throw new Error("A fresh projected claim was unexpectedly refused locally");
    }
    expect(loot.claimIntent()).toBeNull();
    source.sendCommand(command);
    expect(loot.copper).toBe(0);
    expect(loot.sheet).toEqual(sheet);
    // Lose the claim tick's bag sheet. Later projections retain money and absence;
    // the existing periodic bag resend recovers the missed inventory change.
    const frames = source.advance(4 / 30);
    const later = frames.at(-1)!;
    expect(frames[0]?.inventory).not.toBeNull();
    expect(later.inventory).toBeNull();
    expect(later.viewer.copper).toBe(2);
    expect(later.loot).toBeNull();
    bag.update(later);
    loot.update(later);
    expect(loot.copper).toBe(2);
    expect(loot.sheet).toBeNull();
    expect(loot.claimIntent()).toBeNull();
    expect(bag.ready).toBe(false);
    for (let tick = 0; tick < 10 && !bag.ready; tick += 1) {
      run(source, 1); bag.update(source.latestProjection()!);
    }
    expect(bag.ready).toBe(true);
    expect(bag.slots?.[0]).toEqual({ itemId: 1, quantity: 5 });
    source.sendCommand({ kind: "loot", creatureId: sheet.creatureId, diedAt: sheet.diedAt });
    run(source, 1);
    expect(source.latestProjection()?.events).toContainEqual({ kind: "error", code: "empty-loot", target: { kind: "creature", id: 108 } });
    expect(source.latestProjection()?.viewer.copper).toBe(2);
    source.leave();
    loot.reset();
    expect(loot.copper).toBe(0);
    expect(loot.sheet).toBeNull();
    expect(claim(before)).toBeNull();
    await source.join();
    loot.reset(source.latestProjection());
    loot.update(source.latestProjection()!);
    // The looted copper is the stored character's; the loot window starts empty.
    expect(loot.sheet).toBeNull();
    expect(source.latestProjection()?.viewer.copper).toBe(2);
  });
  test("shared relief queries and both exported terrain grids consume the saved signed-centimetre field", () => {
    const provider = createLocalWorld(wasm, testCharacters()).scenery;
    const field = record(JSON.parse(readFileSync(`${RELIEF_PACKAGE_DIRECTORY}/flattened.heights.json`, "utf8")));
    const heights = field.heights;
    if (!Array.isArray(heights)) {
      throw new Error("Missing saved relief field");
    }
    for (let row = 0; row < RELIEF_GRID.rows; row += 1) {
      for (let column = 0; column < RELIEF_GRID.columns; column += 1) {
        const x = RELIEF_GRID.originXzUnits[0] + column * RELIEF_GRID.stepUnits;
        const z = RELIEF_GRID.originXzUnits[1] + row * RELIEF_GRID.stepUnits;
        expect(provider.reliefAt(x, z)).toEqual(heights[row * RELIEF_GRID.columns + column]);
      }
    }
    for (const grid of [provider.scenery.terrain, provider.scenery.farTerrain]) {
      for (let row = 0; row < grid.rows; row += 1) {
        for (let column = 0; column < grid.columns; column += 1) {
          const x = grid.originXz[0] + column * grid.step;
          const z = grid.originXz[1] + row * grid.step;
          if (x >= -3500 && x <= 3500 && z >= -1300 && z <= 5300) {
            expect(grid.heights[row * grid.columns + column]).toBe(provider.reliefAt(x, z));
          }
        }
      }
    }
    expect(provider.scenery.contentRevision).toBe(14n);
  });
  test("multi-tick frames retain intermediate bag sheets and refusal feedback", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    await source.join();
    const bag = new BagState();
    bag.update(source.latestProjection()!);
    source.sendCommand({ kind: "move-item", source: 0, destination: 15, quantity: 2 });
    const moved = source.advance(4 / 30);
    expect(moved.map((view) => view.tick)).toEqual([1n, 2n, 3n, 4n]);
    expect(moved[0]?.inventory).not.toBeNull();
    expect(moved[3]?.inventory).toBeNull();
    for (const view of moved) {
      bag.update(view);
    }
    expect(bag.ready).toBe(true);
    expect(bag.slots?.[15]).toEqual({ itemId: 1, quantity: 2 });
    // Feedback answers the bag's own pending intent.
    source.sendCommand(bag.move(15, 1, 1)!);
    const refused = source.advance(4 / 30);
    expect(refused[0]?.events).toContainEqual({ kind: "error", code: "invalid-inventory-move", target: null });
    expect(refused[3]?.events).toEqual([]);
    for (const view of refused) {
      bag.update(view);
    }
    expect(bag.feedback).toContain("refused");
    expect(source.advance(0)).toEqual([]);
  });
  test("bag moves use queued core authority and missed sheets recover periodically", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    await source.join();
    const initial = source.latestProjection();
    expect(initial?.inventoryRevision).toBe(1n);
    expect(initial?.inventory?.[0]).toEqual({ itemId: 1, quantity: 3 });
    source.sendCommand({ kind: "move-item", source: 0, destination: 15, quantity: 2 });
    expect(source.latestProjection()).toEqual(initial);
    run(source, 1);
    const changed = source.latestProjection();
    expect(changed?.inventoryRevision).toBe(2n);
    expect(changed?.inventory?.[0]).toEqual({ itemId: 1, quantity: 1 });
    expect(changed?.inventory?.[15]).toEqual({ itemId: 1, quantity: 2 });
    source.sendCommand({ kind: "move-item", source: 15, destination: 1, quantity: 1 });
    run(source, 1);
    expect(source.latestProjection()?.events).toContainEqual({ kind: "error", code: "invalid-inventory-move", target: null });
    run(source, 7);
    expect(source.latestProjection()?.inventory).toBeNull();
    expect(source.latestProjection()?.inventoryRevision).toBe(2n);
    run(source, 1);
    expect(source.latestProjection()?.inventory).toEqual(changed?.inventory);
    source.leave();
    // Re-entering resumes the stored bag under a fresh zone-local revision.
    await source.join();
    expect(source.latestProjection()?.inventoryRevision).toBe(1n);
    expect(source.latestProjection()?.inventory).toEqual(changed?.inventory);
  });
  test("equipping the starter dagger reaches the sheet and the melee range", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    await source.join();
    expect(source.latestProjection()?.equipment).toEqual(Array(6).fill(null));
    expect(source.latestProjection()?.viewer.damage).toEqual({ min: 3, max: 6 });
    source.sendCommand({ kind: "equip-item", bagSlot: 1 });
    run(source, 1);
    const equipped = source.latestProjection();
    expect(equipped?.inventoryRevision).toBe(2n);
    expect(equipped?.inventory?.[1]).toBeNull();
    expect(equipped?.equipment).toEqual([2, null, null, null, null, null]);
    expect(equipped?.stats).toEqual({ stamina: 0, strength: 2, agility: 2, intellect: 0 });
    expect(equipped?.viewer.damage).toEqual({ min: 4, max: 7 });
    source.sendCommand({ kind: "equip-item", bagSlot: 0 });
    run(source, 1);
    expect(source.latestProjection()?.events).toContainEqual({ kind: "error", code: "not-equippable", target: null });
    source.sendCommand({ kind: "unequip-item", equipmentSlot: 0 });
    run(source, 1);
    expect(source.latestProjection()?.equipment).toEqual(Array(6).fill(null));
    expect(source.latestProjection()?.inventory?.[1]).toEqual({ itemId: 2, quantity: 1 });
  });
  test("a spoken line comes back to the speaker, and a second within a second is refused", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    const player = await source.join();
    source.sendCommand({ kind: "chat", channel: "say", text: "Hail, Greyhaven!" });
    source.sendCommand({ kind: "chat", channel: "yell", text: "Too soon" });
    run(source, 1);
    expect(source.latestProjection()?.chat).toEqual([{ speaker: player, channel: "say", text: "Hail, Greyhaven!" }]);
    expect(source.latestProjection()?.events).toContainEqual({ kind: "error", code: "chat-throttled", target: null });
    run(source, 1);
    expect(source.latestProjection()?.chat).toEqual([]);
  });

  test("the real vendor sells and buys back at the inn's counter", async () => {
    const { source, catalog } = createLocalWorld(wasm, testCharacters());
    await source.join();
    expect(catalog.vendors.get(3)?.offers[0]).toEqual({ item: 2, price: 5 });
    expect(catalog.items.get(1)?.sellPrice).toBe(1);
    // Out of reach on the spawn plaza.
    source.sendCommand({ kind: "sell-item", npc: 3, bagSlot: 0, quantity: 3 });
    run(source, 1);
    expect(source.latestProjection()?.events).toContainEqual({ kind: "error", code: "out-of-range", target: { kind: "npc", id: 3 } });
    // The vendor-resume scenario's route: east along the plaza, north to the inn door, open it,
    // then in and along the room to Bram Tolliver behind his bar.
    const walk = (facing: number, ticks: number) => {
      source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing });
      run(source, ticks);
    };
    walk(EAST, 145);
    walk(32_768, 31);
    source.sendCommand({ kind: "move", forward: 0, strafe: 0, facing: 32_768 });
    run(source, 1);
    source.sendCommand({ kind: "use-door", door: 1, open: true });
    run(source, 2);
    expect(source.latestProjection()?.doors).toEqual([{ door: 1, open: true }]);
    walk(32_768, 29);
    walk(EAST, 20);
    walk(32_768, 8);
    source.sendCommand({ kind: "move", forward: 0, strafe: 0, facing: 32_768 });
    run(source, 2);
    source.sendCommand({ kind: "sell-item", npc: 3, bagSlot: 0, quantity: 3 });
    run(source, 1);
    expect(source.latestProjection()?.viewer.copper).toBe(3);
    expect(source.latestProjection()?.inventory?.[0]).toBeNull();
    source.sendCommand({ kind: "buy-item", npc: 3, offer: 0, quantity: 1 });
    run(source, 1);
    expect(source.latestProjection()?.events).toContainEqual({ kind: "error", code: "not-enough-money", target: { kind: "npc", id: 3 } });
    source.sendCommand({ kind: "sell-item", npc: 3, bagSlot: 1, quantity: 1 });
    source.sendCommand({ kind: "buy-item", npc: 3, offer: 0, quantity: 1 });
    run(source, 1);
    const bought = source.latestProjection();
    expect(bought?.viewer.copper).toBe(0);
    expect(bought?.inventory?.[0]).toEqual({ itemId: 2, quantity: 1 });
    expect(bought?.inventory?.slice(1).every((slot) => slot === null)).toBe(true);
  });

  test("loads under Bun and hosts zone 1 with the shared content revision", () => {
    const zone = new wasm.LocalZone() as InstanceType<typeof wasm.LocalZone> & { zoneId(): number };
    const { entries } = testCharacters();
    expect(zone.zoneId()).toBe(1);
    expect(zone.contentRevision()).toBe(14n);
    const player = zone.enter(entries, "local-1", 2, 0, 1n);
    const projection = decodeSnapshot(zone.projection(player));
    expect(projection).toMatchObject({ zoneId: 1, tick: 0n, contentRevision: 14n, viewerId: player, acknowledgedSequence: 0 });
    // A new character has made its class choice on entry.
    expect(projection.viewer).toEqual({
      copper: 0, health: 50, maxHealth: 50, experience: 0, experienceToNextLevel: 100, level: 1, dead: false, inCombat: false, autoAttacking: false, target: null,
      classChoice: { classId: "arcanist", sex: "female" }, resource: { kind: "mana", value: 110, max: 110 }, cast: null, globalCooldown: 0, damage: { min: 3, max: 6 },
      guard: { raised: null, protecting: false, broken: 0 }, stance: null, combo: null,
    });
    expect(() => zone.submit(player, 1, Uint8Array.of(1, 1, 0, 0, 0, 0))).toThrow();
    expect(zone.submit(player, 1, encodeCommand({ kind: "jump" }))).toBe(true);
    expect(zone.submit(player, 1, encodeCommand({ kind: "jump" }))).toBe(false);
    zone.exit(entries, player);
    expect(() => zone.projection(player)).toThrow();
  });

  test("a local character keeps its progress and place across pages and plays in one tab only", async () => {
    const storage = new MemoryStorage();
    const characters = testCharacters(storage);
    const first = createLocalWorld(wasm, characters).source;
    await first.join({ id: "local-1", classId: "warden", sex: "female" });
    first.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: EAST });
    run(first, 20);
    const moved = self(first).position;
    // Another tab of the same browser cannot play the character meanwhile.
    const otherTab = createLocalWorld(wasm, characters).source;
    await expect(otherTab.join({ id: "local-1", classId: "warden", sex: "female" })).rejects.toThrow("another tab");
    // A page that goes away without leaving keeps its last commit; once the browser
    // dropped its lock, the next page's newer epoch takes the character over.
    first.persist();
    const reloaded = createLocalWorld(wasm, testCharacters(storage, new MemoryLocks())).source;
    await reloaded.join({ id: "local-1", classId: "ranger", sex: "male" });
    expect(self(reloaded).position).toEqual(moved);
    expect(reloaded.latestProjection()?.viewer.classChoice).toEqual({ classId: "warden", sex: "female" });
    // The browser's one world resumed too, at the tick the first page saved.
    expect(reloaded.latestProjection()!.tick).toBeGreaterThanOrEqual(20n);
    // The superseded page is fenced: its next commit fails closed.
    expect(() => first.persist()).toThrow("no longer current");
    reloaded.leave();
  });

  test("an unusable saved world is replaced by a fresh one, and the source says so", async () => {
    const storage = new MemoryStorage();
    storage.setItem("mmorpg.local-world.v1", btoa("not a checkpoint"));
    const { source } = createLocalWorld(wasm, testCharacters(storage));
    await source.join({ id: "local-1", classId: "warden", sex: "male" });
    expect(source.worldNotice).toContain("reset");
    expect(source.latestProjection()!.tick).toBe(0n);
    source.leave();
    expect(storage.getItem("mmorpg.local-world.v1")).not.toBe(btoa("not a checkpoint"));
  });

  test("camera-relative movement moves the decoded viewer in the expected direction", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    await source.join();
    const spawn = self(source).position;
    source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: EAST });
    run(source, 10);
    const east = self(source).position;
    expect(source.latestProjection()?.tick).toBe(10n);
    expect(east).toEqual([spawn[0] + 10 * RUN_UNITS_PER_TICK, spawn[1], spawn[2]]);
    expect(self(source).facing).toBe(EAST);

    // The character's right is yaw - 90°: facing +X, strafing right heads toward +Z.
    source.sendCommand({ kind: "move", forward: 0, strafe: 1, facing: EAST });
    run(source, 5);
    const right = self(source).position;
    expect(right[0]).toBe(east[0]);
    expect(right[2]).toBe(east[2] + 5 * RUN_UNITS_PER_TICK);

    source.sendCommand({ kind: "move", forward: 0, strafe: 0, facing: EAST });
    run(source, 3);
    expect(self(source).position).toEqual(right);
  });

  test("a grounded jump rises and physics brings the unit back down", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    await source.join();
    const ground = self(source).position[1];
    source.sendCommand({ kind: "jump" });
    run(source, 1);
    const lifted = self(source).position[1];
    expect(lifted).toBeGreaterThan(ground);
    let apex = lifted;
    for (let tick = 0; tick < 60; tick += 1) {
      run(source, 1);
      apex = Math.max(apex, self(source).position[1]);
    }
    expect(apex).toBeGreaterThan(lifted);
    expect(self(source).position[1]).toBe(ground);
  });

  test("interpolated samples stay between authoritative ticks", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    await source.join();
    source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: EAST });
    run(source, 2);
    const before = self(source).position[0];
    source.advance(TICK_SECONDS / 2);
    const sampled = source.sample().find((entity) => entity.kind === "player" && entity.entityId === source.latestProjection()?.viewerId);
    expect(sampled?.position[0]).toBeGreaterThan(before - RUN_UNITS_PER_TICK);
    expect(sampled?.position[0]).toBeLessThan(before);
  });

  test("scenery and areas come from the same content as the zone", async () => {
    const { source, scenery } = createLocalWorld(wasm, testCharacters());
    expect(scenery.scenery.contentRevision).toBe(14n);
    expect(scenery.scenery.source).toBe("mmorpg-scenery");
    expect(scenery.scenery.playerHalfExtents).toEqual([30, 90, 30]);
    await source.join();
    const [x, , z] = self(source).position;
    expect(scenery.areaAt(x, z)?.name).toBe("Greyhaven Outpost");
    expect(scenery.areaAt(-5_500, 0)?.name).toBe("Wolfrun Woods");
    // Region streaming (#240): the woods need wolf and boar models, the outpost none.
    expect([...scenery.regionModels(2)].sort()).toEqual(["Pig", "Wolf"]);
    expect(scenery.regionModels(1)).toEqual([]);
    expect(scenery.areaAt(-50_000, -50_000)).toBeNull();
    // Spawn slots are flat; the mountains beyond the walls are not.
    expect(scenery.reliefAt(x, z)).toBe(0);
    expect(scenery.reliefAt(0, -27_000)).toBeGreaterThan(1_000);
    // The keep is its exact collider, and the lake is walkable water.
    const keep = scenery.scenery.props.find((prop) => prop.collider?.id === 100);
    expect(keep?.kind).toBe("keep");
    expect(keep?.collider?.halfExtents).toEqual(keep?.halfExtents);
    expect(scenery.scenery.water).toEqual([{ centerXz: [5_500, -5_500], radiiXz: [2_200, 1_600], surfaceY: 15 }]);
    expect(scenery.scenery.roads.map((road) => road.name)).toContain("Hollow Road");
    expect(Math.max(...scenery.scenery.farTerrain.heights)).toBeGreaterThan(Math.max(...scenery.scenery.terrain.heights));
  });

  test("WASM scenery consumes the exact selected Outpost anchors and presentation transforms", () => {
    const provider = createLocalWorld(wasm, testCharacters()).scenery;
    const selected = record(JSON.parse(readFileSync(`${GRASS_PACKAGE_DIRECTORY}/selected.instances.json`, "utf8"))).instances;
    const transforms: unknown = JSON.parse(readFileSync(`${GRASS_PACKAGE_DIRECTORY}/selected.transforms.json`, "utf8"));
    if (!Array.isArray(selected) || !Array.isArray(transforms)) {
      throw new Error("Missing selected grass package");
    }
    const expected = selected.map((value, index) => {
      const position = record(value).positionMicro;
      const transform = record(transforms[index]);
      if (!Array.isArray(position)) {
        throw new Error("Missing selected grass coordinates");
      }
      const x = Number(position[0]) / 10_000;
      const y = Number(position[1]) / 10_000;
      const z = Number(position[2]) / 10_000 + 2000;
      return { kind: "grass-tuft", position: [x, y + provider.reliefAt(x, z), z], yaw: transform.yaw,
        scale: Number(transform.scalePermille) / 1000, halfExtents: transform.halfExtents, collider: null };
    });
    const actual = provider.scenery.props.filter((prop) => prop.kind === "grass-tuft" && prop.position[0] >= -3500 && prop.position[0] <= 3500 && prop.position[2] >= -1300 && prop.position[2] <= 5300);
    expect(actual).toEqual(expected);
    expect(actual.length).toBe(55);
    expect(provider.scenery.presentationFingerprint).toBe("88c9ce651488dc71");
    expect(provider.scenery.contentRevision).toBe(14n);
  });

  test("the vale's static scene models every prop within a bounded node and vertex budget", () => {
    const { scenery } = createLocalWorld(wasm, testCharacters());
    const scene = buildSceneryScene(scenery.scenery, scenery.staticScene);
    const modelled = Object.values(scene.stats.props).reduce((sum, count) => sum + (count ?? 0), 0);
    expect(modelled).toBe(scenery.scenery.props.length);
    // Draw-call and per-frame validation budgets: ~7 100 props merge into about 1 300 batches
    // across the expanded valley (#159); distance culling keeps far batches out of a frame.
    expect(scene.stats.staticNodes).toBeLessThanOrEqual(1_500);
    expect(scene.stats.staticVertices).toBeLessThanOrEqual(230_000);
    expect(scenery.scenery.props.length).toBeGreaterThan(scene.stats.staticNodes * 5);
    expect(new Set(scene.batches.map((batch) => batch.node.geometry.resourceKey)).size).toBe(scene.batches.length);
    // Terrain and far-ring meshes span the whole vale, so none is ever culled: keep their palette small.
    const terrainMeshes = scene.batches.filter((batch) => /^static-all-(terrain|far):/.test(batch.node.id)).length;
    expect(terrainMeshes).toBeLessThanOrEqual(44);
  });
});

describe("WASM local zone combat intents", () => {
  test("the catalog names the hosted units and matches the zone's content", () => {
    const { catalog } = createLocalWorld(wasm, testCharacters());
    expect(catalog.contentRevision).toBe(14n);
    expect([...catalog.items.values()].map((item) => item.name)).toEqual([
      "Torn Fur", "Worn Dagger", "Militia Shortsword", "Apprentice Wand", "Pine Buckler", "Cloth Hood", "Padded Tunic",
      "Padded Trousers", "Worn Boots", "Wolf Pelt",
    ]);
    // The decoder's item table matches the catalog the zone binds.
    const slots = ["mainHand", "offHand", "head", "chest", "legs", "feet"];
    expect(catalog.items.size).toBe(ITEM_SHAPES.size);
    for (const item of catalog.items.values()) {
      const shape = ITEM_SHAPES.get(item.id);
      const { stamina, strength, agility, intellect } = item.stats;
      expect([shape?.maxStack, shape?.slot, shape?.stats]).toEqual([
        item.maxStack, item.slot === null ? null : slots.indexOf(item.slot), [stamina, strength, agility, intellect],
      ]);
    }
    expect([...catalog.creatureTemplates.values()].map((template) => template.name)).toEqual([
      "Timber Wolf", "Young Boar", "Grain Rat", "Field Marauder", "Mirefin Lurker", "Redbrand Bandit", "Garrick Redbrand",
    ]);
    expect(catalog.npcs.get(5)).toEqual({ id: 5, name: "Brother Aldous", role: "spirit_healer", level: 10 });
    expect(catalog.areas.get(2)).toBe("Wolfrun Woods");
    expect([...catalog.classes.values()].map((record) => [record.name, record.resource])).toEqual([
      ["warden", "rage"], ["ranger", "focus"], ["arcanist", "mana"],
    ]);
    // The decoder's ability table matches the catalog the zone binds.
    const auraKinds = ["damage-over-time", "heal-over-time", "absorb", "root", "snare", "stun", "haste"];
    expect(catalog.abilities.size).toBe(ABILITY_SHAPES.size);
    for (const ability of catalog.abilities.values()) {
      const shape = ABILITY_SHAPES.get(ability.id);
      expect([shape?.castTicks, shape?.channel, shape?.cooldown, shape?.aura]).toEqual([
        ability.castTicks, ability.channel, ability.cooldown, ability.aura === null ? null : auraKinds[ability.aura - 1],
      ]);
    }
    // Every ability has presentation facts, and the spell visuals key the abilities they name.
    expect(undescribedAbilities(catalog)).toEqual([]);
    expect([5, 9, 10, 11, 12].map((id) => catalog.abilities.get(id)?.name)).toEqual(["Aimed Shot", "Firebolt", "Frost Nova", "Arcane Barrier", "Blizzard"]);
    expect(catalog.abilities.get(12)?.channel).toBe(true);
  });

  test("the chosen class reaches the zone and ability slots are refused or validated there", async () => {
    const { source, catalog } = createLocalWorld(wasm, testCharacters());
    await source.join({ id: "local-1", classId: "arcanist", sex: "female" });
    run(source, 1);
    const projection = source.latestProjection()!;
    expect(projection.viewer.classChoice).toEqual({ classId: "arcanist", sex: "female" });
    expect(self(source).appearance).toBe(5);
    expect(abilitySlots(projection, catalog).map((ability) => ability.name)).toEqual([
      "Firebolt", "Frost Nova", "Arcane Barrier", "Blizzard",
    ]);
    expect(combatStatus(projection, catalog)).toBe("Health 50/50 · Mana 110/110 · Level 1");
    // Firebolt needs a target; Frost Nova is learned at level 2.
    for (const slot of [1, 2]) {
      source.sendCommand(useAbilitySlot(slot, projection, catalog)!);
    }
    run(source, 1);
    expect(source.latestProjection()?.events).toEqual([
      { kind: "error", code: "invalid-target", target: null },
      { kind: "error", code: "not-learned", target: null },
    ]);
  });

  test("a directed Heroic Strike reaches the zone, which validates it like the slot's ability", async () => {
    for (const [classId, code] of [["warden", "not-enough-resource"], ["ranger", "not-learned"]] as const) {
      const { source } = createLocalWorld(wasm, testCharacters());
      await source.join({ id: "local-1", classId, sex: "male" });
      run(source, 1);
      // A fresh Warden has no Rage yet; the zone, not the client, refuses the swing.
      source.sendCommand({ kind: "directed-strike", ability: 1, facing: 0, aim: { side: "left", height: "high" } });
      run(source, 1);
      expect(source.latestProjection()?.events).toEqual([{ kind: "error", code, target: null }]);
    }
  });

  test("hub NPCs are visible, selectable and refuse to be attacked", async () => {
    const { source, catalog } = createLocalWorld(wasm, testCharacters());
    await source.join();
    const projection = source.latestProjection();
    const npcs = projection?.entities.filter((entity) => entity.kind === "npc") ?? [];
    expect(npcs.length).toBeGreaterThan(0);
    expect(projection?.entities.some((entity) => entity.kind === "creature")).toBe(false);
    const guard = npcs[0]!;
    expect(catalog.npcs.has(guard.entityId)).toBe(true);
    expect(guard.flags.attackable).toBe(false);
    const target = { kind: "npc", id: guard.entityId } as const;
    source.sendCommand({ kind: "select-target", target });
    source.sendCommand({ kind: "start-attack" });
    run(source, 1);
    const refused = source.latestProjection();
    expect(refused?.viewer.target).toEqual(target);
    expect(refused?.viewer.autoAttacking).toBe(false);
    expect(refused?.events).toEqual([{ kind: "error", code: "not-attackable", target }]);
    source.sendCommand({ kind: "release-spirit" });
    source.sendCommand({ kind: "select-target", target: null });
    run(source, 1);
    expect(source.latestProjection()?.events).toEqual([{ kind: "error", code: "not-dead", target: null }]);
    expect(source.latestProjection()?.viewer.target).toBeNull();
  });

  test("the Marshal offers the first quest, marks it and the zone accepts and abandons it", async () => {
    const { source, catalog } = createLocalWorld(wasm, testCharacters());
    await source.join();
    const quests = new QuestState();
    quests.update(source.latestProjection()!, catalog);
    expect(quests.marker(1)).toBe("available");
    expect([...catalog.quests.values()].map((quest) => quest.name)).toEqual([
      "Trouble in the Woods", "Pelts for the Tanner", "The Missing Farmhand", "Marauders in the Fields", "Scout the Lake",
      "Mirefin Menace", "Into Redbrand Hollow", "Garrick Redbrand", "Return to Greyhaven",
    ]);
    // From spawn slot 0, run west along the plaza to 3.5 m from the Marshal.
    source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: 49_152 });
    run(source, 30);
    source.sendCommand({ kind: "move", forward: 0, strafe: 0, facing: 49_152 });
    run(source, 2);
    const near = source.latestProjection()!;
    const marshal = dialogNpc(near, catalog);
    expect(marshal).toEqual({ npc: 1, name: "Marshal Elden Greywatch", inReach: true });
    expect(dialogView(quests.sheet!, catalog, marshal!).offered.map((quest) => quest.id)).toEqual([1]);
    source.sendCommand(quests.accept(marshal, 1)!);
    run(source, 1);
    quests.update(source.latestProjection()!, catalog);
    expect(quests.sheet?.entries).toEqual([{ quest: 1, progress: [0, 0, 0] }]);
    expect(quests.marker(1)).toBe("in-progress");
    expect(quests.feedback).toBe("Accepted: Trouble in the Woods.");
    expect(trackerLines(quests.sheet!, catalog)).toEqual(["Trouble in the Woods", "  · Timber Wolf slain: 0/6"]);
    source.sendCommand({ kind: "complete-quest", npc: 1, quest: 1, choice: 0 });
    run(source, 1);
    expect(source.latestProjection()?.events).toEqual([{ kind: "error", code: "quest-incomplete", target: { kind: "npc", id: 1 } }]);
    source.sendCommand(quests.abandon(1)!);
    run(source, 1);
    quests.update(source.latestProjection()!, catalog);
    expect(quests.sheet?.entries).toEqual([]);
    expect(quests.marker(1)).toBe("available");
    expect(quests.feedback).toBe("Abandoned: Trouble in the Woods.");
  });

  test("intents beyond the per-tick bound are reported, never a session error", async () => {
    const { source } = createLocalWorld(wasm, testCharacters());
    await source.join();
    for (let press = 0; press < 20; press += 1) {
      expect(() => source.sendCommand({ kind: "stop-attack" })).not.toThrow();
    }
    run(source, 1);
    expect(source.latestProjection()?.events).toEqual([{ kind: "error", code: "too-many-intents", target: null }]);
    source.sendCommand({ kind: "stop-attack" });
    run(source, 1);
    expect(source.latestProjection()?.events).toEqual([]);
  });
});

/** The real WASM zone, but its projections lose their first byte until repaired. */
function refusingWasmSource() {
  const zone = new wasm.LocalZone();
  let corrupt = true;
  const handle: LocalZoneHandle = {
    enter: (entries, characterId, classId, sex, zoneEpoch) => zone.enter(entries, characterId, classId, sex, zoneEpoch),
    save: (entries, player) => zone.save(entries, player),
    exit: (entries, player) => zone.exit(entries, player),
    checkpoint: () => zone.checkpoint(),
    submit: (player, sequence, command) => zone.submit(player, sequence, command),
    tick: () => zone.tick(),
    projection: (player) => (corrupt ? zone.projection(player).subarray(1) : zone.projection(player)),
  };
  return { ...immediate(new LocalZoneSource(() => handle, testCharacters())), repair: () => { corrupt = false; } };
}

worldSourceContract("LocalZoneSource over the WASM zone", () => immediate(createLocalWorld(wasm, testCharacters()).source), refusingWasmSource);
