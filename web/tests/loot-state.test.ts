import { expect, test } from "bun:test";
import type { ErrorCode, LootView, ZoneEvent } from "../src/replication";
import { LootState } from "../src/world/units/loot-state";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

function sheet(diedAt = 10n): LootView {
  return { creatureId: 108, diedAt, money: 2, item: { itemId: 1, quantity: 2 }, questItem: null };
}

function projection(tick = 12n, loot: LootView | null = sheet(), copper = 0) {
  return testSnapshot({ zoneId: 1, contentRevision: 5n, viewerId: 1, tick,
    acknowledgedSequence: 5, entities: [playerEntity(1, [0, 90, 0])],
    viewer: { ...HEALTHY_VIEWER, copper, target: { kind: "creature", id: 108 } }, loot });
}

function intent(state: LootState) {
  const action = state.claimIntent();
  if (action === null) {
    throw new Error("Expected an eligible claim intent");
  }
  return action;
}

test("a claim is one fenced command, never a local reward or repeated dispatch", () => {
  const state = new LootState();
  expect(state.claimIntent()).toBeNull();
  state.update(projection());
  const queued = intent(state);
  expect(state.canClaim).toBe(false);
  expect(state.claimIntent()).toBeNull();
  expect(queued(projection())).toEqual({ kind: "loot", creatureId: 108, diedAt: 10n });
  expect(queued(projection())).toBeNull();
  expect(state.canClaim).toBe(false);
  expect(state.claimIntent()).toBeNull();
  expect(state.feedback).toContain("Waiting");
  expect(state.sheet).toEqual(sheet());
  expect(state.copper).toBe(0);
  state.update({ ...projection(13n), acknowledgedSequence: 6 });
  expect(state.canClaim).toBe(true);
});

test("dropped publications recover complete loot and copper; complete absence clears", () => {
  const state = new LootState();
  state.update(projection());
  const queued = intent(state);
  queued(projection());
  state.update(projection(18n, null, 2));
  expect(state.copper).toBe(2);
  expect(state.sheet).toBeNull();
  expect(state.canClaim).toBe(false);
  expect(state.claimIntent()).toBeNull();
  expect(state.feedback).toBe("");
});

test("stale ticks and mismatched joined identities cannot replace received state", () => {
  const state = new LootState();
  state.reset(projection());
  state.update(projection());
  for (const wrong of [projection(11n, null, 9), projection(12n, null, 9),
    { ...projection(13n), viewerId: 2 }, { ...projection(13n), zoneId: 2 },
    { ...projection(13n), contentRevision: 6n }]) {
    expect(state.update(wrong)).toBe(false);
    expect(state.copper).toBe(0);
    expect(state.sheet).toEqual(sheet());
  }
});

test("queued intent rechecks current death, target, life, absence, tick and session", () => {
  for (const wrong of [projection(13n, sheet(11n)), projection(13n, null), projection(11n),
    { ...projection(13n), viewerId: 2 }, { ...projection(13n), zoneId: 2 },
    { ...projection(13n), contentRevision: 6n },
    { ...projection(13n), viewer: { ...HEALTHY_VIEWER, target: null } },
    { ...projection(13n), viewer: { ...HEALTHY_VIEWER, health: 0, dead: true } }]) {
    const state = new LootState();
    state.update(projection());
    expect(intent(state)(wrong)).toBeNull();
    expect(state.copper).toBe(0);
    expect(state.sheet).toEqual(sheet());
  }
});

test("reset invalidates a queued action even when session-local IDs are reused", () => {
  const state = new LootState();
  state.update(projection());
  const queued = intent(state);
  state.reset(projection());
  expect(state.copper).toBe(0);
  expect(state.sheet).toBeNull();
  expect(state.feedback).toBe("");
  state.update(projection());
  expect(queued(projection())).toBeNull();
  expect(intent(state)(projection())).not.toBeNull();
});

test("full bag and money/revision refusal retain rewards and allow a later retry", () => {
  const refusals: ErrorCode[] = ["inventory-full", "money-overflow", "invalid-inventory-move", "out-of-range", "too-many-intents"];
  for (const code of refusals) {
    const state = new LootState();
    state.update(projection());
    intent(state)(projection());
    state.update({ ...projection(13n), acknowledgedSequence: 6,
      events: [{ kind: "error", code, target: { kind: "creature", id: 108 } }] });
    expect(state.sheet).toEqual(sheet());
    expect(state.copper).toBe(0);
    expect(state.feedback).not.toBe("");
    expect(state.canClaim).toBe(true);
    state.update(projection(14n));
    expect(state.feedback).not.toBe("");
    expect(intent(state)(projection(14n))).not.toBeNull();
  }
});

test("new target/death clears old feedback; unrelated combat errors are ignored", () => {
  const state = new LootState();
  state.update(projection());
  intent(state)(projection());
  state.update({ ...projection(13n), events: [{ kind: "error", code: "inventory-full", target: { kind: "creature", id: 109 } }] });
  expect(state.feedback).toContain("Waiting");
  state.update({ ...projection(14n, sheet(14n)), events: [{ kind: "error", code: "inventory-full", target: { kind: "creature", id: 108 } }] });
  expect(state.feedback).toBe("");
  const queued = intent(state);
  state.update({ ...projection(15n, null), viewer: { ...HEALTHY_VIEWER, target: null } });
  expect(queued(projection(16n))).toBeNull();
  expect(state.sheet).toBeNull();
  expect(state.feedback).toBe("");
});

test("received and returned sheet mutation cannot change cached rewards or fences", () => {
  const state = new LootState();
  const received = projection();
  state.update(received);
  if (received.loot === null || received.loot.item === null || received.viewer.target === null) {
    throw new Error("Incomplete fixture");
  }
  received.loot.money = 999;
  received.loot.diedAt = 999n;
  received.loot.item.quantity = 20;
  received.viewer.copper = 999;
  received.viewer.target.id = 999;
  const shown = state.sheet;
  if (shown === null) {
    throw new Error("Missing received sheet");
  }
  shown.money = 888;
  expect(state.sheet).toEqual(sheet());
  expect(state.copper).toBe(0);
  expect(intent(state)(projection())).toEqual({ kind: "loot", creatureId: 108, diedAt: 10n });
});

test("a lost refusal event recovers usable received rewards on a later acknowledgement", () => {
  const state = new LootState();
  state.update(projection());
  intent(state)(projection());
  state.update({ ...projection(20n), acknowledgedSequence: 6 });
  expect(state.canClaim).toBe(true);
  expect(state.sheet).toEqual(sheet());
  expect(state.copper).toBe(0);
  expect(state.feedback).toBe("Rewards are still available.");
});

test("dispatch cannot rewind a newer accepted projection", () => {
  const state = new LootState();
  state.update(projection());
  const queued = intent(state);
  state.update(projection(20n));
  expect(queued(projection(19n))).toBeNull();
  expect(state.sheet).toEqual(sheet());
  expect(state.copper).toBe(0);
});

test("targetless bag failures cannot report retained rewards after a successful claim", () => {
  const codes: ErrorCode[] = ["inventory-full", "invalid-inventory-move", "you-are-dead"];
  for (const code of codes) {
    const state = new LootState();
    state.update(projection());
    intent(state)(projection());
    state.update({ ...projection(13n, null, 2), acknowledgedSequence: 7,
      events: [{ kind: "error", code, target: null }] });
    expect(state.sheet).toBeNull();
    expect(state.copper).toBe(2);
    expect(state.feedback).toBe("");
  }
});

test("generic intent-capacity feedback may have no target", () => {
  const state = new LootState();
  state.update(projection());
  intent(state)(projection());
  state.update({ ...projection(13n), acknowledgedSequence: 6,
    events: [{ kind: "error", code: "too-many-intents", target: null }] });
  expect(state.sheet).toEqual(sheet());
  expect(state.copper).toBe(0);
  expect(state.feedback).toContain("Too many actions");
  expect(state.canClaim).toBe(true);
});

test("a later dropped intent cannot turn a consumed claim into capacity refusal feedback", () => {
  const state = new LootState();
  state.update(projection());
  intent(state)(projection());
  state.update({ ...projection(13n, null, 2), acknowledgedSequence: 24,
    events: [{ kind: "error", code: "too-many-intents", target: null }] });
  expect(state.sheet).toBeNull();
  expect(state.copper).toBe(2);
  expect(state.feedback).toBe("");
  expect(state.canClaim).toBe(false);
});

test("generic death feedback describes the currently dead viewer", () => {
  const state = new LootState();
  state.update(projection());
  intent(state)(projection());
  state.update({ ...projection(13n, null), viewer: { ...HEALTHY_VIEWER, health: 0, dead: true, target: { kind: "creature", id: 108 } },
    events: [{ kind: "error", code: "you-are-dead", target: null }] });
  expect(state.feedback).toContain("while dead");
  expect(state.sheet).toBeNull();
  expect(state.canClaim).toBe(false);
});

test("a specific claim refusal takes priority over generic capacity feedback in either order", () => {
  const specific: ZoneEvent = { kind: "error", code: "inventory-full", target: { kind: "creature", id: 108 } };
  const generic: ZoneEvent = { kind: "error", code: "too-many-intents", target: null };
  for (const events of [[specific, generic], [generic, specific]]) {
    const state = new LootState();
    state.update(projection());
    intent(state)(projection());
    state.update({ ...projection(13n), acknowledgedSequence: 24, events });
    expect(state.feedback).toContain("bags are full");
    expect(state.sheet).toEqual(sheet());
    expect(state.copper).toBe(0);
    expect(state.canClaim).toBe(true);
  }
});
