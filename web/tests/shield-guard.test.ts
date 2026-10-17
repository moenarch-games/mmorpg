import { describe, expect, test } from "bun:test";
import type { Profile } from "@moritzbrantner/input-bindings";
import { encodeCommand } from "../src/command-wire";
import { actionBindingLabels } from "../src/input/binding-labels";
import { decodeSnapshot, type ZoneEvent } from "../src/replication";
import { combatTexts } from "../src/world/units/combat-text";
import { GUARD_LANE_THRESHOLD_PX, ShieldGuardHold, canGuard, guardDisplay, laneFromOffset, padGuardLane } from "../src/world/units/shield-guard";
import { encodeTestSnapshot } from "./support/snapshot-encoder";
import { playerEntity, testSnapshot } from "./support/snapshots";

const VIEWER = { kind: "player", id: 1 } as const;
const WOLF = { kind: "creature", id: 108 } as const;

function snapshot(events: ZoneEvent[], guard = { raised: null, protecting: false, broken: 0 } as const) {
  const base = testSnapshot({
    zoneId: 1, tick: 5n, contentRevision: 1n, acknowledgedSequence: 1, viewerId: 1,
    entities: [playerEntity(1, [0, 90, 0]), { ...playerEntity(108, [0, 45, 150]), kind: "creature" }],
    events,
  });
  return {
    ...base,
    viewer: { ...base.viewer, classChoice: { classId: "warden", sex: "female" } as const, resource: { kind: "rage", value: 0, max: 100 } as const, guard },
  };
}

describe("shield guard input", () => {
  test("pointer travel picks the lane, camera drags never move it, and only a raised lane is lowered", () => {
    const t = GUARD_LANE_THRESHOLD_PX;
    expect([laneFromOffset(0), laneFromOffset(-t), laneFromOffset(t), laneFromOffset(t - 1)]).toEqual(["middle", "high", "low", "middle"]);
    const hold = new ShieldGuardHold();
    hold.track(100, false);
    expect(hold.pending()).toBeUndefined();
    hold.begin();
    expect(hold.pending()).toBe("middle");
    expect(hold.pending()).toBeUndefined();
    hold.track(100 - t, false);
    expect(hold.pending()).toBe("high");
    // A camera drag owns the pointer.
    hold.track(400, true);
    expect(hold.lane()).toBe("high");
    hold.track(400 + 2 * t, false);
    expect(hold.pending()).toBe("low");
    expect(hold.end()).toBe(true);
    expect(hold.active).toBe(false);
    // Releasing the displaced key again sends nothing.
    expect(hold.end()).toBe(false);
  });

  test("the guard has its own configurable binding and wire command", () => {
    const profile: Profile = { id: "character:test", patches: [] };
    expect(actionBindingLabels("combat.guard", profile)).toEqual(["Z", "Pad 6"]);
    expect([...encodeCommand({ kind: "guard", lane: "high" })]).toEqual([11, 22, 3]);
    expect([...encodeCommand({ kind: "guard", lane: null })]).toEqual([11, 22, 0]);
  });
});

describe("shield guard presentation", () => {
  test("the projected guard round-trips the wire and drives the HUD state", () => {
    const guarded = snapshot([
      { kind: "blocked", source: WOLF, target: VIEWER, lane: "middle", amount: 3 },
      { kind: "guard-broken", source: WOLF, target: VIEWER, lane: "middle", ticks: 45 },
    ], { raised: "middle", protecting: false, broken: 45 });
    const decoded = decodeSnapshot(encodeTestSnapshot(guarded));
    expect(decoded.viewer.guard).toEqual({ raised: "middle", protecting: false, broken: 45 });
    expect(decoded.events).toEqual(guarded.events);
    expect(canGuard(decoded)).toBe(true);
    expect(guardDisplay(decoded.viewer.guard)).toEqual({ lane: "middle", state: "broken" });
    expect(guardDisplay({ raised: "low", protecting: true, broken: 0 })).toEqual({ lane: "low", state: "protecting" });
    expect(guardDisplay({ raised: "low", protecting: false, broken: 0 })).toEqual({ lane: "low", state: "suspended" });
    expect(guardDisplay({ raised: null, protecting: false, broken: 0 })).toBeNull();
    expect(combatTexts(decoded)).toEqual([{ kind: "block", text: "Block middle" }, { kind: "guard-break", text: "Guard broken!" }]);
  });

});

describe("pad shield guard", () => {
  const pad = (trigger: number, stickY: number) => ({
    buttons: Array.from({ length: 17 }, (_, index) => ({ value: index === 6 ? trigger : 0 })),
    axes: [0, 0, 0, stickY],
  });

  test("holding LT aims with the right stick and overrides the pointer while held", () => {
    expect(padGuardLane([null, pad(0, -1)])).toBeNull();
    expect([padGuardLane([pad(1, 0)]), padGuardLane([pad(1, -0.8)]), padGuardLane([pad(1, 0.6)])]).toEqual(["middle", "high", "low"]);
    const hold = new ShieldGuardHold();
    hold.begin();
    hold.aimWithPad(padGuardLane([pad(1, -0.9)]));
    expect(hold.pending()).toBe("high");
    hold.aimWithPad(null);
    expect(hold.pending()).toBe("middle");
    expect(hold.end()).toBe(true);
  });
});
