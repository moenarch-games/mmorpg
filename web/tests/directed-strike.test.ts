import { describe, expect, test } from "bun:test";
import type { Profile } from "@moritzbrantner/input-bindings";
import { actionBindingLabels } from "../src/input/binding-labels";
import { GameControls, type HoldPhase } from "../src/input/game-controls";
import { AIM_THRESHOLD_PX, DirectedStrikeAim, aimFromDelta } from "../src/world/units/directed-strike";
import { eventText } from "../src/world/units/combat-hud";
import { playerEntity, testSnapshot } from "./support/snapshots";

class FakeTarget {
  readonly #listeners = new Map<string, Set<(event: unknown) => void>>();
  hidden = false;
  visibilityState = "visible";
  addEventListener(type: string, listener: (event: unknown) => void): void {
    const listeners = this.#listeners.get(type) ?? new Set();
    listeners.add(listener);
    this.#listeners.set(type, listeners);
  }
  removeEventListener(type: string, listener: (event: unknown) => void): void {
    this.#listeners.get(type)?.delete(listener);
  }
  emit(type: string, event: unknown = {}): void {
    for (const listener of this.#listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

function key(code: string) {
  return { key: code, code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, repeat: false, preventDefault() {} };
}

function holdHarness() {
  const holds: HoldPhase[] = [];
  const actions: string[] = [];
  const window = new FakeTarget();
  const document = new FakeTarget();
  const controls = new GameControls({
    screen: () => ({ phase: "world", panelOpen: false }),
    onAction: (action) => actions.push(action),
    onHold: (action, phase) => {
      expect(action).toBe("combat.directedStrike");
      holds.push(phase);
    },
  });
  controls.attach({ window, document, getGamepads: () => [] });
  return { controls, holds, actions, window };
}

describe("directed Heroic Strike input", () => {
  test("pointer travel since the press picks side and height; up is high", () => {
    const t = AIM_THRESHOLD_PX;
    expect(aimFromDelta(0, 0)).toEqual({ side: "centre", height: "middle" });
    expect(aimFromDelta(-t, -t)).toEqual({ side: "left", height: "high" });
    expect(aimFromDelta(t, t)).toEqual({ side: "right", height: "low" });
    expect(aimFromDelta(t - 1, -(t - 1))).toEqual({ side: "centre", height: "middle" });

    const aim = new DirectedStrikeAim();
    aim.track(100, 100);
    expect(aim.aim()).toBeNull();
    aim.begin();
    aim.track(60, 160);
    expect(aim.aim()).toEqual({ side: "left", height: "low" });
    expect(aim.release()).toEqual({ side: "left", height: "low" });
    expect(aim.active).toBe(false);
    aim.begin();
    aim.cancel();
    expect(aim.release()).toBeNull();
  });

  test("a deliberate key release strikes; retirement cancels; the ordinary slot stays a press", () => {
    const { controls, holds, actions, window } = holdHarness();
    window.emit("keydown", key("KeyG"));
    window.emit("keyup", key("KeyG"));
    expect(holds).toEqual(["press", "release"]);
    window.emit("keydown", key("KeyG"));
    window.emit("blur");
    expect(holds).toEqual(["press", "release", "press", "cancel"]);
    window.emit("keydown", key("KeyG"));
    controls.retire("settingsOpened");
    window.emit("keyup", key("KeyG"));
    expect(holds.slice(4)).toEqual(["press", "cancel"]);
    window.emit("keydown", key("Digit1"));
    expect(actions).toEqual(["ability.slot1"]);
  });

  test("the slot hint shows the working binding, a remap, and nothing when unbound or conflicting", () => {
    const profile = (patches: Profile["patches"]): Profile => ({ id: "character:test", patches });
    expect(actionBindingLabels("combat.directedStrike", profile([]))).toEqual(["G"]);
    const remapped = profile([
      { op: "remove", bindingId: "combat.directedStrike.0" },
      { op: "add", binding: { id: "custom.strike", action: "combat.directedStrike", sequence: [{ key: { kind: "physical", value: "KeyX" } }], when: { op: "context", id: "gameplay" } } },
    ]);
    expect(actionBindingLabels("combat.directedStrike", remapped)).toEqual(["X"]);
    expect(actionBindingLabels("combat.directedStrike", profile([{ op: "remove", bindingId: "combat.directedStrike.0" }]))).toEqual([]);
    // G also bound to jump in the same context: ambiguous, so no shortcut is claimed.
    const conflict = profile([
      { op: "add", binding: { id: "custom.jump", action: "move.jump", sequence: [{ key: { kind: "physical", value: "KeyG" } }], when: { op: "context", id: "gameplay" } } },
    ]);
    expect(actionBindingLabels("combat.directedStrike", conflict)).toEqual([]);
  });
});

describe("directed Heroic Strike presentation", () => {
  test("feedback names the aim and whether the sweep struck", () => {
    const snapshot = testSnapshot({ zoneId: 1, tick: 1n, contentRevision: 1n, acknowledgedSequence: 0, viewerId: 1, entities: [playerEntity(1, [0, 90, 0])] });
    const catalog = { abilities: new Map([[1, { name: "Heroic Strike" }]]), creatureTemplates: new Map(), npcs: new Map(), quests: new Map() } as never;
    const whiff = eventText({ kind: "strike-swept", source: { kind: "player", id: 1 }, target: null, ability: 1, aim: { side: "right", height: "high" } }, snapshot, catalog);
    expect(whiff).toBe("Your high-right Heroic Strike cuts only air.");
  });
});
