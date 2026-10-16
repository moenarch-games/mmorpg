import { describe, expect, test } from "bun:test";
import type { GamepadLike } from "@moritzbrantner/input-bindings-web";
import {
  contextStack,
  GameControls,
  type ControlScreen,
  type GameAction,
} from "../src/input/game-controls";

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

  get listenerCount(): number {
    return [...this.#listeners.values()].reduce((total, listeners) => total + listeners.size, 0);
  }
}

class Frames {
  #callback: (() => void) | undefined;
  requestFrame(callback: () => void): unknown {
    this.#callback = callback;
    return 1;
  }
  cancelFrame(): void {
    this.#callback = undefined;
  }
  step(): void {
    this.#callback?.();
  }
}

function key(code: string, target?: unknown, repeat = false) {
  return { key: code, code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, repeat, target, preventDefault() {} };
}

/** A focused native control, as `closest` sees it. */
const nativeButton = { closest: (selector: string) => (selector.startsWith("button") ? {} : null) };
const nativeSlider = { closest: (selector: string) => (selector.includes("role=slider") ? {} : null) };

function harness(initial: ControlScreen = { phase: "world", panelOpen: false }) {
  let screen = initial;
  let pads: GamepadLike[] = [];
  const actions: GameAction[] = [];
  const window = new FakeTarget();
  const document = new FakeTarget();
  const frames = new Frames();
  const controls = new GameControls({ screen: () => screen, onAction: (action) => actions.push(action) });
  const detach = controls.attach({ window, document, getGamepads: () => pads, frameScheduler: frames });
  return {
    controls,
    actions,
    window,
    document,
    detach,
    setScreen(next: ControlScreen) {
      screen = next;
    },
    pad(axes: number[], pressed: number[] = []) {
      pads = [{
        index: 0,
        axes,
        buttons: Array.from({ length: 16 }, (_, index) => ({ pressed: pressed.includes(index), value: pressed.includes(index) ? 1 : 0 })),
      }];
      frames.step();
    },
    unplug() {
      pads = [];
      frames.step();
    },
  };
}

describe("MMORPG semantic controls", () => {
  test("the catalog is valid and every screen maps to an ordered context stack", () => {
    expect(() => harness()).not.toThrow();
    expect(contextStack({ phase: "selection", creating: true })).toEqual([
      { id: "selection" },
      { id: "creation", blocksLower: true },
    ]);
    expect(contextStack({ phase: "world", panelOpen: true }).map((layer) => layer.id)).toEqual(["world", "gameplay", "panels"]);
  });

  test("WASD, arrows and Q/E map to forward/backpedal and strafe; opposites cancel", () => {
    const app = harness();
    expect(app.controls.heldIntent()).toEqual({ forward: 0, strafe: 0, steering: false });
    app.window.emit("keydown", key("KeyW"));
    app.window.emit("keydown", key("KeyE"));
    expect(app.controls.heldIntent()).toEqual({ forward: 1, strafe: 1, steering: true });
    app.window.emit("keydown", key("ArrowDown"));
    app.window.emit("keydown", key("KeyQ"));
    expect(app.controls.heldIntent()).toEqual({ forward: 0, strafe: 0, steering: true });
  });

  test("the gamepad drives the same semantic movement and one-shot actions as the keyboard", () => {
    const keyboard = harness();
    keyboard.window.emit("keydown", key("KeyS"));
    keyboard.window.emit("keydown", key("KeyA"));
    keyboard.window.emit("keydown", key("Space"));

    const gamepad = harness();
    gamepad.pad([-0.9, 0.9], [0]);

    expect(gamepad.controls.heldIntent()).toEqual(keyboard.controls.heldIntent());
    expect([...gamepad.actions].sort()).toEqual([...keyboard.actions].sort());
    gamepad.unplug();
    expect(gamepad.controls.heldIntent().steering).toBe(false);
  });

  test("one-shot actions dispatch once per press, never on key repeat", () => {
    const app = harness();
    app.window.emit("keydown", key("Space"));
    app.window.emit("keydown", key("Space", undefined, true));
    app.window.emit("keyup", key("Space"));
    app.window.emit("keydown", key("Tab"));
    app.window.emit("keydown", key("KeyF"));
    app.window.emit("keydown", key("KeyB"));
    app.window.emit("keydown", key("KeyC"));
    app.window.emit("keydown", key("KeyV"));
    app.window.emit("keydown", key("Enter"));
    app.window.emit("keydown", key("F3"));
    expect(app.actions).toEqual(["move.jump", "target.next", "combat.toggleAutoAttack", "ui.toggleBags", "ui.toggleCharacter", "ui.toggleVendor", "ui.openChat", "ui.toggleDebug"]);
  });

  test("an open panel is a non-blocking overlay: it claims Escape while movement falls through", () => {
    const app = harness({ phase: "world", panelOpen: true });
    app.window.emit("keydown", key("KeyW"));
    app.window.emit("keydown", key("Escape"));
    expect(app.controls.heldIntent().forward).toBe(1);
    expect(app.actions).toEqual(["move.forward", "ui.closePanel"]);
    app.setScreen({ phase: "world", panelOpen: false });
    app.window.emit("keyup", key("Escape"));
    app.window.emit("keydown", key("Escape"));
    expect(app.actions.at(-1)).toBe("ui.leaveWorld");
  });

  test("1–4 and the d-pad use ability slots; while casting, Escape cancels the cast before leaving", () => {
    const app = harness();
    for (const code of ["Digit1", "Digit2", "Digit3", "Digit4"]) {
      app.window.emit("keydown", key(code));
    }
    expect(app.actions).toEqual(["ability.slot1", "ability.slot2", "ability.slot3", "ability.slot4"]);
    const pad = harness();
    pad.pad([0, 0], [12, 15, 13, 14]);
    expect([...pad.actions].sort()).toEqual(["ability.slot1", "ability.slot2", "ability.slot3", "ability.slot4"]);
    expect(contextStack({ phase: "world", panelOpen: true, casting: true }).map((layer) => layer.id))
      .toEqual(["world", "gameplay", "casting", "panels"]);
    const casting = harness({ phase: "world", panelOpen: false, casting: true });
    casting.window.emit("keydown", key("Escape"));
    expect(casting.actions).toEqual(["combat.cancelCast"]);
    casting.setScreen({ phase: "world", panelOpen: true, casting: true });
    casting.window.emit("keyup", key("Escape"));
    casting.window.emit("keydown", key("Escape"));
    expect(casting.actions.at(-1)).toBe("ui.closePanel");
  });

  test("character creation is modal over selection; gameplay keys do nothing outside the world", () => {
    const app = harness({ phase: "selection", creating: true });
    app.window.emit("keydown", key("Enter"));
    app.window.emit("keydown", key("KeyW"));
    app.window.emit("keydown", key("Escape"));
    expect(app.actions).toEqual(["ui.cancelCreation"]);
    expect(app.controls.heldIntent().steering).toBe(false);
    app.setScreen({ phase: "selection", creating: false });
    app.window.emit("keyup", key("Enter"));
    app.window.emit("keydown", key("Enter"));
    expect(app.actions.at(-1)).toBe("ui.enterWorld");
  });

  test("native controls keep their own keys; focusing them releases held movement", () => {
    const app = harness();
    app.window.emit("keydown", key("Space", nativeButton));
    app.window.emit("keydown", key("Escape", nativeButton));
    app.window.emit("keydown", key("ArrowLeft", nativeSlider));
    app.window.emit("keydown", key("KeyW", { tagName: "INPUT", type: "text", isContentEditable: false }));
    expect(app.actions).toEqual([]);

    app.window.emit("keydown", key("KeyD"));
    app.document.emit("focusin", { target: nativeButton });
    // An overlay button does not stop running.
    expect(app.controls.heldIntent().strafe).toBe(1);
    app.document.emit("focusin", { target: nativeSlider });
    expect(app.controls.heldIntent().steering).toBe(false);
  });

  test("movement still reaches gameplay while an overlay's button has focus", () => {
    const app = harness({ phase: "world", panelOpen: true });
    app.window.emit("keydown", key("KeyW", nativeButton));
    expect(app.controls.heldIntent().forward).toBe(1);
    expect(app.actions).toEqual(["move.forward"]);
  });

  test("leaving the screen, blur and hidden visibility retire held movement", () => {
    const app = harness();
    app.window.emit("keydown", key("KeyW"));
    app.controls.retire("leftWorld");
    expect(app.controls.heldIntent().steering).toBe(false);
    app.window.emit("keyup", key("KeyW"));

    app.window.emit("keydown", key("KeyA"));
    app.window.emit("blur");
    expect(app.controls.heldIntent().steering).toBe(false);

    app.window.emit("keydown", key("KeyD"));
    app.document.hidden = true;
    app.document.visibilityState = "hidden";
    app.document.emit("visibilitychange");
    expect(app.controls.heldIntent().steering).toBe(false);
  });

  test("detaching removes every listener and forgets held state", () => {
    const app = harness();
    app.window.emit("keydown", key("KeyW"));
    app.detach();
    expect(app.window.listenerCount).toBe(0);
    expect(app.document.listenerCount).toBe(0);
    expect(app.controls.heldIntent().steering).toBe(false);
  });
});
