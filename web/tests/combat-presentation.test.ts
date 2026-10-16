import { describe, expect, test } from "bun:test";
import type { EntityState, ZoneEvent, ZoneSnapshot } from "../src/replication";
import { decodeCatalog } from "../src/world/catalog";
import { placeUnit } from "../src/world/unit-nodes";
import { bodyHalfHeightUnits, creatureBodyNodes, disposition } from "../src/world/units/creature-bodies";
import { abilitySlots, cancelCast, useAbilitySlot } from "../src/world/units/abilities";
import { CombatHud, FEEDBACK_MS, combatStatus, eventText, unitName } from "../src/world/units/combat-hud";
import { CLICK_SLOP_PIXELS, SecondaryClick, attackToggle, nextTabTarget } from "../src/world/units/targeting";
import { catalogJson } from "./support/catalog";
import { HEALTHY_VIEWER, NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots";

const catalog = decodeCatalog(JSON.stringify(catalogJson()));
const WOLF = { kind: "creature", id: 108 } as const;

function creature(entityId: number, x: number, z: number, flags: Partial<EntityState["flags"]> = {}): EntityState {
  return {
    kind: "creature", entityId, appearance: 1, position: [x, 45, z], velocity: [0, 0, 0], facing: 0, level: 2,
    healthPercent: flags.dead ? 0 : 60, flags: { ...NO_FLAGS, hostile: true, attackable: !flags.dead, ...flags },
  };
}

function view(entities: EntityState[], viewer: Partial<ZoneSnapshot["viewer"]> = {}, events: ZoneEvent[] = []): ZoneSnapshot {
  return testSnapshot({
    zoneId: 1, tick: 5n, contentRevision: 3n, acknowledgedSequence: 1, viewerId: 7,
    entities: [playerEntity(7, [0, 90, 0]), ...entities], viewer: { ...HEALTHY_VIEWER, ...viewer }, events,
  });
}

describe("content catalog", () => {
  test("decodes names by ID and refuses malformed exports", () => {
    expect(catalog.contentRevision).toBe(3n);
    expect(catalog.creatureTemplates.get(1)?.name).toBe("Timber Wolf");
    expect(catalog.creatureTemplates.get(2)?.behaviour).toBe("neutral");
    expect(catalog.npcs.get(6)?.role).toBe("guard");
    expect(catalog.areas.get(1)).toBe("Greyhaven Outpost");
    expect(catalog.itemCatalogRevision).toBe(3n);
    expect([...catalog.items.values()].slice(0, 2)).toEqual([
      { id: 1, name: "Torn Fur", maxStack: 20, slot: null, stats: { stamina: 0, strength: 0, agility: 0, intellect: 0 }, sellPrice: 1 },
      { id: 2, name: "Worn Dagger", maxStack: 1, slot: "mainHand", stats: { stamina: 0, strength: 2, agility: 2, intellect: 0 }, sellPrice: 2 },
    ]);
    expect(catalog.quests.get(2)).toEqual({
      id: 2, name: "Pelts for the Tanner", text: "Bring me five wolf pelts.", giver: 2, ender: 2, prerequisite: 1,
      objectives: [{ kind: "collect", target: 10, source: 1, count: 5 }], experience: 200, copper: 15, choices: [9, 2],
    });
    expect(catalog.quests.get(1)?.objectives).toEqual([{ kind: "kill", target: 1, count: 6 }]);
    expect([...catalog.vendors.values()]).toEqual([{ npc: 3, offers: [{ item: 2, price: 9 }] }]);
    const broken = (mutate: (json: Record<string, unknown>) => void) => {
      const json = catalogJson();
      mutate(json);
      return () => decodeCatalog(JSON.stringify(json));
    };
    expect(broken((json) => { json.version = 1; })).toThrow("unsupported format");
    expect(broken((json) => { json.contentFingerprint = "XYZ"; })).toThrow("fingerprint");
    expect(broken((json) => { json.extra = true; })).toThrow("exactly");
    expect(broken((json) => { (json.npcs as Record<string, unknown>[])[0]!.role = "wizard"; })).toThrow("role");
    expect(broken((json) => {
      const templates = json.creatureTemplates as Record<string, unknown>[];
      templates[1]!.id = 1;
    })).toThrow("unique");
    expect(broken((json) => { (json.creatureTemplates as Record<string, unknown>[])[0]!.halfExtents = [40, 45]; })).toThrow("three");
    expect(broken((json) => { json.itemCatalogRevision = "1"; })).toThrow("item catalog revision");
    expect(broken((json) => { (json.items as Record<string, unknown>[])[1]!.slot = "ring"; })).toThrow("slot");
    expect(broken((json) => { (json.items as Record<string, unknown>[])[1]!.stats = { stamina: 0 }; })).toThrow("stats");
    expect(broken((json) => {
      (json.items as Record<string, unknown>[])[1]!.stats = { stamina: 256, strength: 0, agility: 0, intellect: 0 };
    })).toThrow("stamina");
    expect(broken((json) => { (json.items as Record<string, unknown>[])[0]!.maxStack = 0; })).toThrow("stack limit");
    expect(broken((json) => { (json.items as Record<string, unknown>[])[1]!.id = 1; })).toThrow("unique");
    expect(broken((json) => { (json.items as Record<string, unknown>[])[0]!.id = 0; })).toThrow("item 0 id");
    expect(broken((json) => { json.vendorCatalogRevision = "1"; })).toThrow("vendor catalog revision");
    const quests = (json: Record<string, unknown>) => json.quests as Record<string, unknown>[];
    expect(broken((json) => { json.questCatalogRevision = "2"; })).toThrow("quest catalog revision");
    expect(broken((json) => { json.questCatalogRevision = "0"; })).toThrow("without a quest catalog");
    expect(broken((json) => { quests(json)[0]!.giver = 3; })).toThrow("quest giver");
    expect(broken((json) => { quests(json)[0]!.id = 33; })).toThrow("quest 0 id");
    expect(broken((json) => { quests(json)[1]!.id = 1; })).toThrow("unique");
    expect(broken((json) => { quests(json)[0]!.objectives = []; })).toThrow("needs an objective");
    expect(broken((json) => { quests(json)[0]!.objectives = [{ kind: "fish", target: 1, source: null, count: 1 }]; })).toThrow("kind");
    expect(broken((json) => { quests(json)[0]!.objectives = [{ kind: "kill", target: 1, source: 1, count: 1 }]; })).toThrow("source");
    expect(broken((json) => { quests(json)[1]!.objectives = [{ kind: "collect", target: 99, source: 1, count: 1 }]; })).toThrow("unknown item");
    expect(broken((json) => { quests(json)[1]!.choices = [99]; })).toThrow("unknown item");
    expect(broken((json) => { quests(json)[0]!.text = ""; })).toThrow("text");
    expect(broken((json) => { json.vendorCatalogRevision = "0"; })).toThrow("without a vendor catalog");
    const vendors = (json: Record<string, unknown>) => json.vendors as { npc: number; offers: { item: number; price: number }[] }[];
    expect(broken((json) => { vendors(json)[0]!.npc = 6; })).toThrow("vendor npc");
    expect(broken((json) => { vendors(json)[0]!.offers = []; })).toThrow("at least one");
    expect(broken((json) => { vendors(json)[0]!.offers[0]!.item = 99; })).toThrow("unknown item");
    expect(broken((json) => { vendors(json)[0]!.offers[0]!.price = 0; })).toThrow("price");
    expect(broken((json) => { (json.items as Record<string, unknown>[])[0]!.sellPrice = -1; })).toThrow("sale value");
    expect(() => decodeCatalog("{")).toThrow("not JSON");
  });
});

describe("ability input and HUD text", () => {
  const caster = (cast: ZoneSnapshot["viewer"]["cast"] = null) => view([], {
    classChoice: { classId: "warden", sex: "male" },
    resource: { kind: "rage", value: 20, max: 100 },
    cast,
  });

  test("slots 1–4 name the class abilities in catalog order; Escape cancels only a cast", () => {
    expect(abilitySlots(caster(), catalog).map((ability) => ability.id)).toEqual([1, 2]);
    expect(useAbilitySlot(2, caster(), catalog)).toEqual({ kind: "use-ability", ability: 2, target: null });
    expect(useAbilitySlot(3, caster(), catalog)).toBeNull();
    expect(useAbilitySlot(1, view([]), catalog)).toBeNull();
    expect(cancelCast(caster())).toBeNull();
    expect(cancelCast(caster({ ability: 9, elapsed: 3, total: 60, channel: false }))).toEqual({ kind: "cancel-cast" });
  });

  test("the status line shows the resource and cast progress", () => {
    expect(combatStatus(caster({ ability: 9, elapsed: 3, total: 60, channel: false }), catalog))
      .toBe("Health 50/50 · Rage 20/100 · Level 1 · Casting Firebolt 3/60");
    const snapshot = caster();
    const bolt = { kind: "cast-started", source: WOLF, target: { kind: "player", id: 1 }, ability: 13, ticks: 45 } as const;
    expect(eventText(bolt, snapshot, catalog)).toBe("A creature begins Muck Bolt.");
    expect(eventText({ kind: "error", code: "not-ready", target: null }, snapshot, catalog)).toBe("That ability is not ready yet.");
  });
});

describe("targeting input", () => {
  test("Tab selects the nearest attackable creature, then cycles outward", () => {
    const entities = [
      creature(300, 900, 0),
      creature(108, 300, 400),
      creature(200, 100, 0, { dead: true }),
      { ...playerEntity(9, [50, 90, 0]) },
    ];
    expect(nextTabTarget(view(entities))).toEqual(WOLF);
    expect(nextTabTarget(view(entities, { target: WOLF }))).toEqual({ kind: "creature", id: 300 });
    expect(nextTabTarget(view(entities, { target: { kind: "creature", id: 300 } }))).toEqual(WOLF);
    expect(nextTabTarget(view([]))).toBeNull();
  });

  test("F toggles auto-attack", () => {
    expect(attackToggle(view([]))).toEqual({ kind: "start-attack" });
    expect(attackToggle(view([], { autoAttacking: true }))).toEqual({ kind: "stop-attack" });
  });

  test("a right click toggles auto-attack; a right drag, a left click or a cancel does not", () => {
    const pointer = (type: string, button: number, clientX = 10, clientY = 10, pointerId = 1) =>
      ({ type, pointerId, button, clientX, clientY });
    const clicks = new SecondaryClick();
    clicks.down(pointer("pointerdown", 2));
    clicks.move(pointer("pointermove", 2, 10 + CLICK_SLOP_PIXELS, 10));
    expect(clicks.end(pointer("pointerup", 2, 10 + CLICK_SLOP_PIXELS, 10))).toBe(true);
    // The capture loss that follows the same release is not a second click.
    expect(clicks.end(pointer("lostpointercapture", 2))).toBe(false);

    clicks.down(pointer("pointerdown", 2));
    clicks.move(pointer("pointermove", 2, 11 + CLICK_SLOP_PIXELS, 10));
    expect(clicks.end(pointer("pointerup", 2))).toBe(false);

    clicks.down(pointer("pointerdown", 0));
    expect(clicks.end(pointer("pointerup", 0))).toBe(false);

    clicks.down(pointer("pointerdown", 2));
    expect(clicks.end(pointer("pointerup", 2, 10, 10, 2))).toBe(false);
    expect(clicks.end(pointer("pointercancel", 2))).toBe(false);
    expect(clicks.end(pointer("pointerup", 2))).toBe(false);

    clicks.down(pointer("pointerdown", 2));
    clicks.reset();
    expect(clicks.end(pointer("pointerup", 2))).toBe(false);
  });
});

describe("combat HUD text", () => {
  test("shows health, combat state and the target, or how to release", () => {
    const wolf = creature(108, 300, 400, { inCombat: true, targetsViewer: true });
    expect(combatStatus(view([wolf], { health: 38, inCombat: true, autoAttacking: true, target: WOLF }), catalog))
      .toBe("Health 38/50 · Level 1 · In combat · Target: Timber Wolf (level 2, 60%) · Attacking");
    expect(combatStatus(view([], { target: WOLF }), catalog)).toBe("Health 50/50 · Level 1 · Target: a creature (out of sight)");
    expect(combatStatus(view([], { health: 0, dead: true }), catalog)).toContain("Press R to release");
    expect(unitName({ kind: "npc", id: 5 }, view([]), catalog)).toBe("Brother Aldous");
    expect(unitName({ kind: "player", id: 7 }, view([]), catalog)).toBe("you");
  });

  test("describes every event from the viewer's side", () => {
    const snapshot = view([creature(108, 300, 400)]);
    const self = { kind: "player", id: 7 } as const;
    const lines = ([
      { kind: "damage-dealt", source: self, target: WOLF, amount: 7, critical: true },
      { kind: "damage-taken", source: WOLF, target: self, amount: 3, critical: false },
      { kind: "miss", source: self, target: WOLF },
      { kind: "miss", source: WOLF, target: self },
      { kind: "evade", source: self, target: WOLF },
      { kind: "died", entity: WOLF, killer: self },
      { kind: "died", entity: self, killer: WOLF },
      { kind: "error", code: "out-of-range", target: WOLF },
      { kind: "error", code: "too-many-intents", target: null },
    ] satisfies ZoneEvent[]).map((event) => eventText(event, snapshot, catalog));
    expect(lines).toEqual([
      "You hit Timber Wolf for 7 (critical).",
      "Timber Wolf hits you for 3.",
      "You miss Timber Wolf.",
      "Timber Wolf misses you.",
      "Timber Wolf evades.",
      "Timber Wolf dies.",
      "You die.",
      "Out of range.",
      "Too many actions at once.",
    ]);
  });
});

describe("combat HUD lines", () => {
  test("write the status every frame and show each tick's latest event until it fades", () => {
    const status = { textContent: "stale" as string | null };
    const feedback = { textContent: "stale" as string | null };
    const hud = new CombatHud(status, feedback);
    hud.reset();
    expect([status.textContent, feedback.textContent]).toEqual(["", ""]);

    const self = { kind: "player", id: 7 } as const;
    const hit: ZoneEvent = { kind: "damage-taken", source: WOLF, target: self, amount: 3, critical: false };
    const missed: ZoneEvent = { kind: "miss", source: self, target: WOLF };
    const tick = (value: bigint, events: ZoneEvent[], health = 50) =>
      ({ ...view([creature(108, 300, 400)], { health }, events), tick: value });
    hud.update(tick(5n, [missed, hit], 47), catalog, 1_000);
    expect(status.textContent).toBe("Health 47/50 · Level 1");
    expect(feedback.textContent).toBe("Timber Wolf hits you for 3.");
    // Frames that show the same tick again do not restart the fade.
    hud.update(tick(5n, [missed, hit], 47), catalog, 1_000 + FEEDBACK_MS - 1);
    expect(feedback.textContent).toBe("Timber Wolf hits you for 3.");
    hud.update(tick(6n, [], 47), catalog, 1_000 + FEEDBACK_MS);
    expect(feedback.textContent).toBe("");
    hud.update(tick(7n, [missed]), catalog, 5_000);
    expect([status.textContent, feedback.textContent]).toEqual(["Health 50/50 · Level 1", "You miss Timber Wolf."]);
  });
});

describe("placeholder bodies", () => {
  test("colour by disposition, size by template, lie flat when dead and ring the target", () => {
    const wolf = creature(108, 300, 400);
    const boar = { ...creature(120, 0, 0, { hostile: false }), appearance: 2 };
    const guard: EntityState = { ...playerEntity(6, [0, 90, 0]), kind: "npc", appearance: 6, level: 10 };
    expect([wolf, boar, guard].map(disposition)).toEqual(["hostile", "neutral", "friendly"]);
    expect(bodyHalfHeightUnits(wolf, catalog, 90)).toBe(45);
    expect(bodyHalfHeightUnits(guard, catalog, 90)).toBe(90);

    const placement = placeUnit(wolf, 45, 0, 100);
    const nodes = creatureBodyNodes(wolf, placement, catalog, true);
    expect(nodes.map((node) => node.id)).toEqual([
      "unit-creature-108-target-ring", "unit-creature-108-body", "unit-creature-108-nose",
    ]);
    const body = nodes[1]!;
    expect(body.geometry).toEqual({ kind: "box", size: [0.8, 0.9, 0.8] });
    expect(body.transform?.translation).toEqual([3, 0.45, 4]);
    const hostileColour = body.color;
    const neutralColour = creatureBodyNodes(boar, placeUnit(boar, 45, 0, 100), catalog, false)[0]!.color;
    const guardColour = creatureBodyNodes(guard, placeUnit(guard, 90, 0, 100), catalog, false)[0]!.color;
    const red = (color: typeof hostileColour) => Number.parseInt(String(color).slice(1, 3), 16);
    const green = (color: typeof hostileColour) => Number.parseInt(String(color).slice(3, 5), 16);
    expect(red(hostileColour)).toBeGreaterThan(green(hostileColour) + 40);
    expect(green(guardColour)).toBeGreaterThan(red(guardColour) + 40);
    expect(red(neutralColour)).toBeGreaterThan(150);
    expect(green(neutralColour)).toBeGreaterThan(120);

    const corpse = creatureBodyNodes(creature(108, 300, 400, { dead: true }), placement, catalog, false);
    expect(corpse.map((node) => node.id)).toEqual(["unit-creature-108-body"]);
    expect(corpse[0]!.geometry).toEqual({ kind: "box", size: [0.8, 0.16, 0.9] });
    expect(creatureBodyNodes(playerEntity(7, [0, 90, 0]), placement, catalog, true)).toEqual([]);
  });
});
