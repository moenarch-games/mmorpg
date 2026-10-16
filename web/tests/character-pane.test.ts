import { expect, test } from "bun:test";
import type { ErrorCode, InventorySlot, ZoneSnapshot } from "../src/replication";
import { decodeCatalog } from "../src/world/catalog";
import { BagState } from "../src/world/units/bag-state";
import { characterPaneModel, itemStatsText } from "../src/world/units/character-pane";
import { catalogJson } from "./support/catalog";
import { HEALTHY_VIEWER, NO_EQUIPMENT, NO_STATS, playerEntity, testSnapshot } from "./support/snapshots";

const catalog = decodeCatalog(JSON.stringify(catalogJson()));
const DAGGER_STATS = { stamina: 0, strength: 2, agility: 2, intellect: 0 };

function bag(dagger = true): InventorySlot[] {
  return [{ itemId: 1, quantity: 3 }, dagger ? { itemId: 2, quantity: 1 } : null, ...Array<null>(14).fill(null)];
}

function projection(tick: number, revision: bigint, sheet: "bagged" | "equipped" | null, fields: Partial<ZoneSnapshot> = {}): ZoneSnapshot {
  const equipped = sheet === "equipped";
  return testSnapshot({
    zoneId: 1, contentRevision: 4n, viewerId: 1, tick: BigInt(tick), acknowledgedSequence: 0,
    entities: [playerEntity(1, [0, 90, 0])], inventoryRevision: revision,
    inventory: sheet === null ? null : bag(!equipped),
    equipment: sheet === null ? null : equipped ? [2, null, null, null, null, null] : NO_EQUIPMENT,
    stats: sheet === null ? null : equipped ? DAGGER_STATS : NO_STATS,
    ...fields,
  });
}

function refusal(tick: number, revision: bigint, code: ErrorCode): ZoneSnapshot {
  return projection(tick, revision, null, { events: [{ kind: "error", code, target: null }] });
}

test("catalog stats read as signed attribute text; items without stats add none", () => {
  expect(itemStatsText(DAGGER_STATS)).toBe("+2 Strength, +2 Agility");
  expect(itemStatsText({ stamina: 1, strength: 0, agility: 0, intellect: 2 })).toBe("+1 Stamina, +2 Intellect");
  expect(itemStatsText(NO_STATS)).toBe("");
});

test("the pane lists six labelled slots, totals, health and damage from received facts only", () => {
  const state = new BagState();
  const waiting = characterPaneModel(state, null, catalog);
  expect(waiting.status).toBe("Waiting for your equipment…");
  expect(waiting.slots.map((slot) => slot.text)).toEqual([
    "Main hand · Waiting", "Off hand · Waiting", "Head · Waiting", "Chest · Waiting", "Legs · Waiting", "Feet · Waiting",
  ]);
  expect(waiting.rows.map((row) => row.value)).toEqual(["—", "—", "—", "—", "—", "—"]);

  state.update(projection(0, 1n, "bagged"));
  const empty = characterPaneModel(state, HEALTHY_VIEWER, catalog);
  expect(empty.slots.every((slot) => slot.text.endsWith("· Empty") && !slot.canUnequip)).toBe(true);
  expect(empty.rows).toEqual([
    { label: "Stamina", value: "0" }, { label: "Strength", value: "0" }, { label: "Agility", value: "0" },
    { label: "Intellect", value: "0" }, { label: "Health", value: "50 / 50" }, { label: "Damage", value: "3–6" },
  ]);

  state.update(projection(1, 2n, "equipped"));
  const viewer = { ...HEALTHY_VIEWER, damage: { min: 4, max: 7 } };
  const equipped = characterPaneModel(state, viewer, catalog);
  expect(equipped.slots[0]).toEqual({
    slot: 0, label: "Main hand", itemName: "Worn Dagger", text: "Main hand · Worn Dagger (+2 Strength, +2 Agility)", canUnequip: true,
  });
  expect(equipped.rows.find((row) => row.label === "Strength")?.value).toBe("2");
  expect(equipped.rows.find((row) => row.label === "Damage")?.value).toBe("4–7");
});

test("an equipped item missing from the catalog fails closed", () => {
  const state = new BagState();
  state.update(projection(0, 1n, "bagged", { equipment: [99, null, null, null, null, null] }));
  expect(() => characterPaneModel(state, HEALTHY_VIEWER, catalog)).toThrow("missing from the content catalog");
});

test("equip and unequip send intent while the received sheet stays unchanged until the zone answers", () => {
  const state = new BagState();
  state.update(projection(0, 1n, "bagged"));
  expect(state.equip(1)).toEqual({ kind: "equip-item", bagSlot: 1 });
  expect(state.pending).toBe(true);
  expect(state.equipment).toEqual(NO_EQUIPMENT);
  expect(state.slots?.[1]).toEqual({ itemId: 2, quantity: 1 });
  expect(state.feedback).toContain("Equip sent");
  state.update(projection(1, 2n, "equipped"));
  expect(state.pending).toBe(false);
  expect(state.feedback).toBe("Equipment updated.");
  expect(state.unequip(0)).toEqual({ kind: "unequip-item", equipmentSlot: 0 });
  // A double click cannot queue a second change from the same sheet, nor can a bag move overtake it.
  expect(state.unequip(0)).toBeNull();
  expect(state.canMoveItems).toBe(false);
  expect(state.move(0, 2, 1)).toBeNull();
  expect(characterPaneModel(state, HEALTHY_VIEWER, catalog).slots[0]?.canUnequip).toBe(false);
  expect(state.equipment?.[0]).toBe(2);
  state.update(projection(2, 3n, "bagged"));
  expect(state.feedback).toBe("Equipment updated.");
  expect(state.equipment?.[0]).toBeNull();
});

test("bounds, empty slots, a missed revision and death offer no equipment intent", () => {
  const state = new BagState();
  expect(state.equip(1)).toBeNull();
  state.update(projection(0, 1n, "bagged"));
  for (const slot of [-1, 2, 16, 1.5, NaN]) {
    expect(state.equip(slot)).toBeNull();
  }
  for (const slot of [-1, 0, 6, 0.5]) {
    expect(state.unequip(slot)).toBeNull();
  }
  state.update(projection(1, 2n, null));
  expect(state.ready).toBe(false);
  expect(state.equip(1)).toBeNull();
  expect(characterPaneModel(state, HEALTHY_VIEWER, catalog).status).toBe("Waiting for your equipment…");
  state.update(projection(10, 2n, "equipped"));
  expect(characterPaneModel(state, HEALTHY_VIEWER, catalog).slots[0]?.canUnequip).toBe(true);
  state.update(projection(11, 2n, null, { viewer: { ...HEALTHY_VIEWER, health: 0, dead: true } }));
  expect(state.unequip(0)).toBeNull();
  const dead = characterPaneModel(state, { ...HEALTHY_VIEWER, health: 0, dead: true }, catalog);
  expect(dead.status).toBe("You cannot change equipment while dead.");
  expect(dead.slots[0]?.canUnequip).toBe(false);
});

test("refusals read for the pending equipment change and leave items unchanged", () => {
  const cases: [ErrorCode, "equip" | "unequip", string][] = [
    ["not-equippable", "equip", "cannot be equipped"],
    ["inventory-full", "unequip", "bag is full. The item stays equipped"],
    ["invalid-inventory-move", "unequip", "equipment change was refused"],
    ["too-many-intents", "equip", "Try the equipment change again"],
    ["you-are-dead", "equip", "cannot change equipment while dead"],
  ];
  for (const [code, intent, message] of cases) {
    const state = new BagState();
    state.update(projection(0, 1n, "equipped"));
    expect(intent === "equip" ? state.equip(0) : state.unequip(0)).not.toBeNull();
    state.update(refusal(1, 1n, code));
    expect(state.feedback).toContain(message);
    expect(state.pending).toBe(false);
    expect(state.equipment?.[0]).toBe(2);
    expect(state.slots).toEqual(bag(false));
  }
  const moving = new BagState();
  moving.update(projection(0, 1n, "bagged"));
  expect(moving.move(0, 2, 1)).not.toBeNull();
  // Bag moves may follow each other, but equipment waits for their answer.
  expect(moving.canMoveItems).toBe(true);
  expect(moving.equip(1)).toBeNull();
  moving.move(0, 1, 1);
  moving.update(refusal(1, 1n, "inventory-full"));
  expect(moving.feedback).toBe("That stack is full. Your items are unchanged.");
});

test("a refusal whose feedback was lost unlocks once a later projection acknowledges the intent", () => {
  const state = new BagState();
  state.update(projection(0, 1n, "equipped", { acknowledgedSequence: 4 }));
  expect(state.unequip(0)).not.toBeNull();
  // An earlier command's acknowledgement does not answer the unequip sent as sequence 6.
  state.sent(6);
  state.update(projection(1, 1n, null, { acknowledgedSequence: 5 }));
  expect(state.canChangeEquipment).toBe(false);
  // Its own sequence was acknowledged, the bag did not change and its refusal event was lost.
  state.update(projection(2, 1n, null, { acknowledgedSequence: 6 }));
  expect(state.pending).toBe(false);
  expect(state.canChangeEquipment).toBe(true);
  expect(state.feedback).toBe("The zone did not change your items.");
  expect(state.equipment?.[0]).toBe(2);
});

test("a confirmed change is not overwritten by other intents' errors in the same projection", () => {
  const state = new BagState();
  state.update(projection(0, 1n, "bagged"));
  expect(state.equip(1)).not.toBeNull();
  state.update(projection(1, 2n, "equipped", { events: [
    { kind: "error", code: "inventory-full", target: { kind: "creature", id: 7 } },
    { kind: "error", code: "too-many-intents", target: null },
  ] }));
  expect(state.feedback).toBe("Equipment updated.");
  // Targeted errors never answer a bag or equipment intent.
  expect(state.unequip(0)).not.toBeNull();
  state.update(projection(2, 2n, null, { events: [{ kind: "error", code: "inventory-full", target: { kind: "creature", id: 7 } }] }));
  expect(state.pending).toBe(true);
});

test("an item intent the source dropped unlocks at once", () => {
  const state = new BagState();
  state.update(projection(0, 1n, "equipped"));
  expect(state.unequip(0)).not.toBeNull();
  state.sent(null);
  expect(state.pending).toBe(false);
  expect(state.feedback).toContain("not sent");
});
