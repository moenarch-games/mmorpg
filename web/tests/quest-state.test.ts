import { describe, expect, test } from "bun:test";
import { TICK_HZ, type EntityState, type QuestSheet, type ZoneEvent, type ZoneSnapshot } from "../src/replication";
import { decodeCatalog } from "../src/world/catalog";
import { QuestState } from "../src/world/units/quest-state";
import { catalogJson } from "./support/catalog";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

const catalog = decodeCatalog(JSON.stringify(catalogJson()));
const MARSHAL = 1;
const TANNER = 2;

function sheet(fields: Partial<QuestSheet> = {}): QuestSheet {
  return { completed: 0, entries: [], markers: [], ...fields };
}

function projection(tick: number, fields: Partial<ZoneSnapshot> = {}, entities: EntityState[] = []): ZoneSnapshot {
  return testSnapshot({
    zoneId: 1, tick: BigInt(tick), contentRevision: 3n, acknowledgedSequence: 0, viewerId: 7,
    entities: [playerEntity(7, [0, 90, 0]), ...entities], ...fields,
  });
}

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
