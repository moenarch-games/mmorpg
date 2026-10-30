import { describe, expect, test } from "bun:test";
import { TICK_HZ, type EntityState, type QuestSheet, type ZoneEvent, type ZoneSnapshot } from "../src/replication";
import { decodeCatalog } from "../src/world/catalog";
import { dialogNpc, dialogView, QuestState, questViews, trackerLines } from "../src/world/units/quest-state";
import { catalogJson } from "./support/catalog";
import { HEALTHY_VIEWER, NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots";

const catalog = decodeCatalog(JSON.stringify(catalogJson()));
const MARSHAL = 1;
const TANNER = 2;

function npc(entityId: number, x: number, z: number): EntityState {
  return {
    kind: "npc", entityId, appearance: entityId, position: [x, 90, z], velocity: [0, 0, 0], facing: 0, level: 10,
    healthPercent: 100, flags: NO_FLAGS,
  };
}

function sheet(fields: Partial<QuestSheet> = {}): QuestSheet {
  return { completed: 0, entries: [], markers: [], ...fields };
}

function projection(tick: number, fields: Partial<ZoneSnapshot> = {}, entities: EntityState[] = []): ZoneSnapshot {
  return testSnapshot({
    zoneId: 1, tick: BigInt(tick), contentRevision: 3n, acknowledgedSequence: 0, viewerId: 7,
    entities: [playerEntity(7, [0, 90, 0]), ...entities], ...fields,
  });
}

describe("quest presentation", () => {
  test("objectives read their received progress, collect counts included", () => {
    const views = questViews(sheet({ entries: [{ quest: 1, progress: [4, 0, 0] }, { quest: 2, progress: [5, 0, 0] }] }), catalog);
    expect(views.map((view) => [view.quest.name, view.objectives.map((objective) => objective.text), view.complete])).toEqual([
      ["Trouble in the Woods", ["Timber Wolf slain: 4/6"], false],
      ["Pelts for the Tanner", ["Wolf Pelt: 5/5"], true],
    ]);
    const errand = questViews(sheet({ completed: 0b11, entries: [{ quest: 3, progress: [1, 0, 0] }] }), catalog)[0]!;
    expect(errand.objectives.map((objective) => objective.text)).toEqual([
      "Speak with Marshal Elden Greywatch (done)", "Explore Greyhaven Outpost",
    ]);
    expect(() => questViews(sheet({ entries: [{ quest: 9, progress: [0, 0, 0] }] }), catalog)).toThrow("catalog");
  });

  test("an NPC offers available quests and takes back the ones that end with it", () => {
    const tanner = { npc: TANNER, name: "Tanner Hilda Brook", inReach: true };
    // Pelts need Trouble in the Woods turned in.
    expect(dialogView(sheet(), catalog, tanner).offered).toEqual([]);
    const unlocked = dialogView(sheet({ completed: 0b1 }), catalog, tanner);
    expect(unlocked.offered.map((quest) => quest.id)).toEqual([2]);
    const active = sheet({ completed: 0b1, entries: [{ quest: 2, progress: [3, 0, 0] }] });
    expect(dialogView(active, catalog, tanner)).toMatchObject({ offered: [], ready: [] });
    expect(dialogView(active, catalog, tanner).inProgress.map((view) => view.quest.id)).toEqual([2]);
    const ready = sheet({ completed: 0b1, entries: [{ quest: 2, progress: [5, 0, 0] }] });
    expect(dialogView(ready, catalog, tanner).ready.map((view) => view.quest.id)).toEqual([2]);
    // Once Pelts is turned in, the errand to the Marshal is offered; the Marshal ends it.
    const done = sheet({ completed: 0b11 });
    expect(dialogView(done, catalog, tanner).offered.map((quest) => quest.id)).toEqual([3]);
    const marshal = { npc: MARSHAL, name: "Marshal Elden Greywatch", inReach: true };
    const errand = sheet({ completed: 0b11, entries: [{ quest: 3, progress: [1, 1, 0] }] });
    expect(dialogView(errand, catalog, marshal).ready.map((view) => view.quest.id)).toEqual([3]);
    // At night the woken Marshal (#158) greets you and takes only turn-ins.
    expect(dialogView(sheet(), catalog, marshal).offered.map((quest) => quest.id)).toEqual([1]);
    expect(dialogView(sheet(), catalog, marshal, true)).toMatchObject({ greeting: "Mm? At this hour?", offered: [] });
    expect(dialogView(errand, catalog, marshal, true).ready.map((view) => view.quest.id)).toEqual([3]);
    expect(dialogView(done, catalog, tanner, true).greeting).toBeNull();
    expect(trackerLines(errand, catalog)).toEqual([
      "The Missing Farmhand — return to Marshal Elden Greywatch",
      "  ✓ Speak with Marshal Elden Greywatch (done)",
      "  ✓ Explore Greyhaven Outpost (done)",
    ]);
  });

  test("an explicitly selected non-quest or missing NPC never redirects Talk to a nearby quest giver", () => {
    for (const id of [3, 6, 999]) {
      const snapshot = projection(1, { viewer: { ...HEALTHY_VIEWER, target: { kind: "npc", id } } }, [
        npc(MARSHAL, 100, 0), npc(3, 200, 0), npc(6, 300, 0),
      ]);
      expect(dialogNpc(snapshot, catalog)).toBeNull();
    }
  });

  test("talking prefers the selected quest NPC, then the nearest, and hints reach", () => {
    const entities = [npc(MARSHAL, 300, 0), npc(TANNER, 100, 400), npc(3, 50, 0)];
    expect(dialogNpc(projection(1, {}, entities), catalog)).toEqual({ npc: MARSHAL, name: "Marshal Elden Greywatch", inReach: true });
    const selected = projection(1, { viewer: { ...HEALTHY_VIEWER, target: { kind: "npc", id: TANNER } } }, entities);
    expect(dialogNpc(selected, catalog)).toEqual({ npc: TANNER, name: "Tanner Hilda Brook", inReach: true });
    expect(dialogNpc(projection(1, {}, [npc(MARSHAL, 600, 0)]), catalog)?.inReach).toBe(false);
    // The vendor gives no quests.
    expect(dialogNpc(projection(1, {}, [npc(3, 50, 0)]), catalog)).toBeNull();
  });
});

describe("quest state", () => {
  test("keeps the latest sheet and markers and ignores older ticks", () => {
    const state = new QuestState();
    state.update(projection(2, { inventory: Array(16).fill(null), quests: sheet({ markers: [{ npc: MARSHAL, marker: "available" }] }) }), catalog);
    expect(state.marker(MARSHAL)).toBe("available");
    expect(state.marker(TANNER)).toBeNull();
    state.update(projection(1, { inventory: Array(16).fill(null), quests: sheet() }), catalog);
    expect(state.marker(MARSHAL)).toBe("available");
    // A projection without a sheet keeps the received one.
    state.update(projection(3), catalog);
    expect(state.sheet?.markers).toEqual([{ npc: MARSHAL, marker: "available" }]);
  });

  test("requests need reach and life; refusals and answers become feedback", () => {
    const state = new QuestState();
    const near = { npc: MARSHAL, name: "Marshal", inReach: true };
    expect(state.accept({ ...near, inReach: false }, 1)).toBeNull();
    expect(state.accept(null, 1)).toBeNull();
    expect(state.abandon(1)).toBeNull();
    expect(state.accept(near, 1)).toEqual({ kind: "accept-quest", npc: MARSHAL, quest: 1 });
    expect(state.feedback).toBe("Accepting…");
    // A pending request blocks repeats until the zone answers.
    expect(state.busy).toBe(true);
    expect(state.accept(near, 1)).toBeNull();
    expect(state.complete(near, 1, 0)).toBeNull();
    const refusal = (tick: number, event: ZoneEvent) => projection(tick, { events: [event] });
    // A refusal about another NPC does not answer it.
    state.update(refusal(1, { kind: "error", code: "invalid-quest", target: { kind: "npc", id: 3 } }), catalog);
    expect(state.feedback).toBe("Accepting…");
    state.update(refusal(2, { kind: "error", code: "quest-log-full", target: { kind: "npc", id: MARSHAL } }), catalog);
    expect(state.feedback).toBe("Your quest log is full. Abandon a quest first.");
    expect(state.complete(near, 1, 0)).toEqual({ kind: "complete-quest", npc: MARSHAL, quest: 1, choice: 0 });
    state.update(refusal(3, { kind: "error", code: "quest-incomplete", target: { kind: "npc", id: MARSHAL } }), catalog);
    expect(state.feedback).toBe("That quest is not complete yet.");

    const bag = Array(16).fill(null);
    state.update(projection(4, { inventory: bag, quests: sheet() }), catalog);
    state.update(projection(5, { inventory: bag, quests: sheet({ entries: [{ quest: 1, progress: [0, 0, 0] }] }) }), catalog);
    expect(state.feedback).toBe("Accepted: Trouble in the Woods.");
    state.update(refusal(6, { kind: "quest-progress", quest: 1, objective: 0, count: 2 }), catalog);
    expect(state.feedback).toBe("Timber Wolf slain: 2/6");
    expect(state.abandon(1)).toEqual({ kind: "abandon-quest", quest: 1 });
    state.update(projection(7, { inventory: bag, quests: sheet() }), catalog);
    expect(state.feedback).toBe("Abandoned: Trouble in the Woods.");
    state.update(refusal(8, { kind: "quest-completed", quest: 2 }), catalog);
    expect(state.feedback).toBe("Pelts for the Tanner completed.");
    state.update(projection(9, { viewer: { ...HEALTHY_VIEWER, health: 0, dead: true } }), catalog);
    expect(state.accept(near, 1)).toBeNull();
    state.reset();
    expect([state.sheet, state.feedback]).toEqual([null, ""]);
  });

  test("a turn-in whose completion event was lost is answered by the next sheet", () => {
    const state = new QuestState();
    const near = { npc: MARSHAL, name: "Marshal", inReach: true };
    const bag = Array(16).fill(null);
    state.update(projection(1, { inventory: bag, quests: sheet({ entries: [{ quest: 1, progress: [6, 0, 0] }] }) }), catalog);
    expect(state.complete(near, 1, 0)).not.toBeNull();
    expect(state.feedback).toBe("Turning in…");
    // The projection carrying `quest-completed` never arrived; a periodic sheet did.
    state.update(projection(3, { inventory: bag, quests: sheet({ completed: 1 }) }), catalog);
    expect(state.feedback).toBe("Trouble in the Woods completed.");
    expect(state.busy).toBe(false);
  });

  test("an unanswered request stops blocking after three seconds", () => {
    const state = new QuestState();
    const near = { npc: MARSHAL, name: "Marshal", inReach: true };
    state.update(projection(10, { inventory: Array(16).fill(null), quests: sheet() }), catalog);
    expect(state.accept(near, 1)).not.toBeNull();
    state.update(projection(10 + 3 * TICK_HZ), catalog);
    expect(state.busy).toBe(true);
    state.update(projection(11 + 3 * TICK_HZ), catalog);
    expect(state.busy).toBe(false);
    expect(state.accept(near, 1)).not.toBeNull();
  });

  test("confirmed progress highlights its objective once; hydration, repeats and stale sheets do not", () => {
    const state = new QuestState();
    const bag = Array(16).fill(null);
    const wolves = (tick: number, count: number) =>
      projection(tick, { inventory: bag, quests: sheet({ entries: [{ quest: 1, progress: [count, 0, 0] }] }) });
    // The first sheet after a reset (initial load, refetch, reconnect) only synchronizes.
    state.update(wolves(10, 3), catalog);
    expect([state.highlighted(1, 0), state.notice]).toEqual([false, ""]);
    state.update(wolves(11, 4), catalog);
    expect(state.highlighted(1, 0)).toBe(true);
    expect(state.notice).toBe("");
    // A repeated sheet keeps the cue without renewing it; an older one is ignored.
    state.update(wolves(12, 4), catalog);
    state.update(wolves(9, 2), catalog);
    expect(state.sheet?.entries[0]?.progress[0]).toBe(4);
    state.update(wolves(11 + 3 * TICK_HZ, 4), catalog);
    expect(state.highlighted(1, 0)).toBe(true);
    state.update(wolves(12 + 3 * TICK_HZ, 4), catalog);
    expect(state.highlighted(1, 0)).toBe(false);
    state.reset();
    state.update(wolves(12 + 6 * TICK_HZ, 5), catalog);
    expect([state.highlighted(1, 0), state.notice]).toEqual([false, ""]);
  });

  test("a confirmed completion shows a short notice and freshens the right turn-in marker", () => {
    const state = new QuestState();
    const bag = Array(16).fill(null);
    state.update(projection(1, { inventory: bag, quests: sheet({ entries: [{ quest: 1, progress: [5, 0, 0] }], markers: [{ npc: MARSHAL, marker: "in-progress" }] }) }), catalog);
    expect(state.freshlyReady(MARSHAL)).toBe(false);
    state.update(projection(2, { inventory: bag, quests: sheet({ entries: [{ quest: 1, progress: [6, 0, 0] }], markers: [{ npc: MARSHAL, marker: "complete" }] }) }), catalog);
    expect(state.notice).toBe("Trouble in the Woods ready — return to Marshal Elden Greywatch");
    expect(state.freshlyReady(MARSHAL)).toBe(true);
    expect(state.freshlyReady(TANNER)).toBe(false);
    // The cue expires on its own; dismissal never gates the turn-in.
    state.update(projection(2 + 7 * TICK_HZ, { inventory: bag, quests: sheet({ entries: [{ quest: 1, progress: [6, 0, 0] }], markers: [{ npc: MARSHAL, marker: "complete" }] }) }), catalog);
    expect([state.notice, state.freshlyReady(MARSHAL)]).toEqual(["", false]);
  });
});
