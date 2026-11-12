import { describe, expect, test } from "bun:test";
import type { Profile } from "@moritzbrantner/input-bindings";
import { actionBindingLabels } from "../src/input/binding-labels";
import { GameControls, type HoldPhase } from "../src/input/game-controls";
import { MouseOwnerInput } from "../src/world/units/mouse-owner";
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
  test("the shared mouse owner aims from motion since the press and fires only on its own release (#636)", () => {
    const mouse = new MouseOwnerInput();
    expect(mouse.owner()).toBe("free-look");
    expect(mouse.motion(10, 0)).toBe(true);
    expect(mouse.beginHold("strike")).toEqual({ started: true, preempted: "none" });
    expect(mouse.motion(-40, 30)).toBe(false); // the locked camera holds still
    expect(mouse.aim()).toEqual({ strike: { side: "left", height: "low" }, guard: null });
    expect(mouse.leftClick()).toEqual({ outcome: "nothing", ended: "none", ability: null, action: null });
    // The guard's press takes the mouse; the preempted strike never fires (5.2.37).
    expect(mouse.beginHold("guard")).toEqual({ started: true, preempted: "strike" });
    expect(mouse.releaseHold("strike")).toBe(false);
    expect(mouse.rightClick()).toEqual({ outcome: "cancel", ended: "guard", ability: null, action: null });
    expect(mouse.releaseHold("guard")).toBe(false);
    expect(mouse.rightClick()).toEqual({ outcome: "cancel-cast", ended: "none", ability: null, action: null });
    mouse.setHoldCamera("free");
    mouse.beginHold("strike");
    expect(mouse.motion(30, -30)).toBe(true);
    expect(mouse.releaseHold("strike")).toBe(true);
    mouse.setFocus(true);
    expect(mouse.owner()).toBe("focus");
    expect(mouse.leftClick()).toEqual({ outcome: "select", ended: "none", ability: null, action: null });
    mouse.beginHold("strike");
    expect(mouse.interrupt()).toBe("strike");
    expect(mouse.releaseHold("strike")).toBe(false);
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
      { op: "add", binding: { id: "custom.strike", action: "combat.directedStrike", sequence: [{ key: { kind: "physical", value: "KeyJ" } }], when: { op: "context", id: "gameplay" } } },
    ]);
    expect(actionBindingLabels("combat.directedStrike", remapped)).toEqual(["J"]);
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

describe("ability slot releases (#414)", () => {
  test("a slot acts on its press and reports a deliberate release or a retirement", () => {
    const actions: string[] = [];
    const releases: string[] = [];
    const window = new FakeTarget();
    const document = new FakeTarget();
    const controls = new GameControls({
      screen: () => ({ phase: "world", panelOpen: false }),
      onAction: (action) => actions.push(action),
      onRelease: (action, phase) => releases.push(`${action} ${phase}`),
    });
    controls.attach({ window, document, getGamepads: () => [] });
    window.emit("keydown", key("Digit1"));
    expect(actions).toEqual(["ability.slot1"]);
    window.emit("keyup", key("Digit1"));
    window.emit("keydown", key("Digit1"));
    window.emit("blur");
    expect(actions).toEqual(["ability.slot1", "ability.slot1"]);
    expect(releases).toEqual(["ability.slot1 release", "ability.slot1 cancel"]);
  });
});
