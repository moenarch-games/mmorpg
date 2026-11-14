import { describe, expect, test } from "bun:test";
import type { ZoneSnapshot } from "../src/replication";
import { GameControls, type GameAction, type HoldPhase, type ReleasedAction } from "../src/input/game-controls";
import { PowerDrawInput } from "../src/world/units/power-draw";
import { HEALTHY_VIEWER, testSnapshot } from "./support/snapshots";

const AIMED_SHOT_SLOT = 1;
const PIERCING_SHOT_SLOT = 5;
const AIMED_SHOT = 5;
const PIERCING_SHOT = 19;

function ranger(overrides: Partial<ZoneSnapshot["viewer"]> = {}): ZoneSnapshot {
  return {
    ...testSnapshot({ zoneId: 1, tick: 1n, contentRevision: 1n, acknowledgedSequence: 0, viewerId: 1, entities: [] }),
    viewer: { ...HEALTHY_VIEWER, level: 60, classChoice: { classId: "ranger", sex: "male" }, ...overrides },
  };
}

function harness() {
  const events: string[] = [];
  const input = new PowerDrawInput({
    prepare: (ability) => events.push(`draw ${ability}`),
    abandon: () => events.push("abandon"),
    send: (ability) => events.push(`fire ${ability}`),
  });
  return { input, events };
}

describe("hold-to-draw power shots (#607)", () => {
  test("press draws and sends nothing; the release fires once", () => {
    const { input, events } = harness();
    expect(input.press(AIMED_SHOT_SLOT, AIMED_SHOT, ranger(), false)).toBe(true);
    expect(events).toEqual([`draw ${AIMED_SHOT}`]);
    input.release(AIMED_SHOT_SLOT, false);
    input.release(AIMED_SHOT_SLOT, false);
    expect(events).toEqual([`draw ${AIMED_SHOT}`, `fire ${AIMED_SHOT}`]);
  });

  test("a tap fires from the partial draw", () => {
    const { input, events } = harness();
    input.press(PIERCING_SHOT_SLOT, PIERCING_SHOT, ranger(), false);
    input.release(PIERCING_SHOT_SLOT, false);
    expect(events).toEqual([`draw ${PIERCING_SHOT}`, `fire ${PIERCING_SHOT}`]);
  });

  test("cancelling, a retired release, dying or an opened panel never fires", () => {
    const cancelled = harness();
    cancelled.input.press(AIMED_SHOT_SLOT, AIMED_SHOT, ranger(), false);
    expect(cancelled.input.cancel()).toBe(true);
    cancelled.input.release(AIMED_SHOT_SLOT, false);
    expect(cancelled.events).toEqual([`draw ${AIMED_SHOT}`, "abandon"]);

    const retired = harness();
    retired.input.press(AIMED_SHOT_SLOT, AIMED_SHOT, ranger(), false);
    retired.input.release(AIMED_SHOT_SLOT, true);
    expect(retired.events).toEqual([`draw ${AIMED_SHOT}`, "abandon"]);

    const dead = harness();
    dead.input.press(AIMED_SHOT_SLOT, AIMED_SHOT, ranger(), false);
    dead.input.observe(ranger(), false);
    dead.input.observe(ranger({ dead: true }), false);
    dead.input.release(AIMED_SHOT_SLOT, false);
    expect(dead.events).toEqual([`draw ${AIMED_SHOT}`, "abandon"]);

    const panel = harness();
    panel.input.press(AIMED_SHOT_SLOT, AIMED_SHOT, ranger(), false);
    panel.input.observe(ranger(), true);
    panel.input.release(AIMED_SHOT_SLOT, false);
    expect(panel.events).toEqual([`draw ${AIMED_SHOT}`, "abandon"]);
  });

  test("another ability, or a press that cannot draw, is an ordinary use", () => {
    const { input, events } = harness();
    expect(input.press(2, 6, ranger(), false)).toBe(false);
    expect(input.press(AIMED_SHOT_SLOT, AIMED_SHOT, ranger({ dead: true }), false)).toBe(false);
    expect(input.press(AIMED_SHOT_SLOT, AIMED_SHOT, ranger(), true)).toBe(false);
    expect(events).toEqual([]);
  });

  test("key-up reaches the draw through the bindings, also on the Shift row and a gamepad", () => {
    const released: [ReleasedAction, Exclude<HoldPhase, "press">][] = [];
    const pressed: GameAction[] = [];
    const controls = new GameControls({
      screen: () => ({ phase: "world", panelOpen: false, weaponDrawn: true }),
      onAction: (action) => pressed.push(action),
      onRelease: (action, phase) => released.push([action, phase]),
    });
    const listeners = new Map<string, (event: unknown) => void>();
    const target = {
      addEventListener: (type: string, listener: (event: unknown) => void) => listeners.set(type, listener),
      removeEventListener: () => {},
    };
    const emit = (type: "keydown" | "keyup", code: string, shiftKey: boolean) =>
      listeners.get(type)?.({ key: code, code, ctrlKey: false, altKey: false, shiftKey, metaKey: false, repeat: false, preventDefault() {} });
    let pads: { index: number; axes: number[]; buttons: { pressed: boolean; value: number }[] }[] = [];
    let frame: (() => void) | undefined;
    const button = (index: number, down: boolean) => {
      pads = [{ index: 0, axes: [0, 0, 0, 0], buttons: Array.from({ length: 16 }, (_, at) => ({ pressed: down && at === index, value: down && at === index ? 1 : 0 })) }];
      frame?.();
    };
    controls.attach({
      window: target,
      document: { ...target, hidden: false, visibilityState: "visible" } as never,
      getGamepads: () => pads,
      frameScheduler: { requestFrame: (callback: () => void) => ((frame = callback), 1), cancelFrame: () => {} },
    });

    emit("keydown", "Digit1", true);
    // Shift let go first: the release still belongs to the Shift-row stroke that pressed it.
    emit("keyup", "ShiftLeft", false);
    emit("keyup", "Digit1", false);
    expect(pressed).toEqual(["ability.slot11"]);
    expect(released).toEqual([["ability.slot11", "release"]]);

    button(12, true);
    button(12, false);
    expect(pressed).toEqual(["ability.slot11", "ability.slot1"]);
    expect(released).toEqual([["ability.slot11", "release"], ["ability.slot1", "release"]]);
  });
});
