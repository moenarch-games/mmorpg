import { describe, expect, test } from "bun:test";
import { contextStack, GAME_ACTION_REGISTRY, GameControls, type ControlScreen, type GameAction } from "../src/input/game-controls";
import { bindingProfileKey, loadBindingProfile, saveBindingProfile } from "../src/settings/binding-profiles";
import { Preferences, SETTING } from "../src/settings/preferences";
import { SettingsPanel, type SettingsPanelElements } from "../src/settings/settings-panel";

class MemoryStorage {
  readonly items = new Map<string, string>();
  getItem(key: string): string | null {
    return this.items.get(key) ?? null;
  }
  setItem(key: string, value: string): void {
    this.items.set(key, value);
  }
}

class FakeTarget {
  readonly #listeners = new Map<string, Set<(event: unknown) => void>>();
  addEventListener(type: string, listener: (event: unknown) => void): void {
    const listeners = this.#listeners.get(type) ?? new Set();
    listeners.add(listener);
    this.#listeners.set(type, listeners);
  }
  removeEventListener(type: string, listener: (event: unknown) => void): void {
    this.#listeners.get(type)?.delete(listener);
  }
  emit(type: string, event: unknown): void {
    for (const listener of this.#listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

function keyEvent(code: string, modifiers: Partial<Record<"ctrlKey" | "shiftKey" | "altKey" | "metaKey", boolean>> = {}) {
  const event = {
    key: code, code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, repeat: false,
    ...modifiers,
    defaultPrevented: false,
    preventDefault() { event.defaultPrevented = true; },
  };
  return event;
}

function harness(screen: () => ControlScreen) {
  const actions: GameAction[] = [];
  const window = new FakeTarget();
  const controls = new GameControls({ screen, onAction: (action) => actions.push(action) });
  controls.attach({ window, document: new FakeTarget(), getGamepads: () => [], frameScheduler: { requestFrame: () => 1, cancelFrame() {} } });
  return { controls, actions, window };
}

describe("browser key ownership (Atlas 6.4.2)", () => {
  test("mapped gameplay keys suppress cancelable browser defaults; unmapped and modified keys do not", () => {
    const app = harness(() => ({ phase: "world", panelOpen: false }));
    const prevented = (code: string, modifiers = {}) => {
      const event = keyEvent(code, modifiers);
      app.window.emit("keydown", event);
      app.window.emit("keyup", keyEvent(code, modifiers));
      return event.defaultPrevented;
    };
    expect(["Space", "Tab", "ArrowUp", "ArrowDown", "F3"].map((code) => prevented(code))).toEqual([true, true, true, true, true]);
    // Shift+Tab focus navigation and browser shortcuts such as Ctrl+F or Ctrl+W stay the browser's.
    expect(prevented("Tab", { shiftKey: true })).toBe(false);
    expect(prevented("KeyF", { ctrlKey: true })).toBe(false);
    expect(prevented("KeyW", { ctrlKey: true })).toBe(false);
  });

  test("the Settings modal blocks every game key and opening it releases held movement", () => {
    let settingsOpen = false;
    const app = harness(() => ({ phase: "world", panelOpen: false, settingsOpen }));
    app.window.emit("keydown", keyEvent("KeyW"));
    expect(app.controls.heldIntent().forward).toBe(1);
    settingsOpen = true;
    app.controls.retire("settingsOpened");
    expect(app.controls.heldIntent().steering).toBe(false);
    const space = keyEvent("Space");
    app.window.emit("keydown", space);
    expect(space.defaultPrevented).toBe(false);
    expect(app.actions).toEqual(["move.forward"]);
    expect(contextStack({ phase: "selection", creating: false, settingsOpen: true }).at(-1)).toEqual({ id: "settings", blocksLower: true });
  });
});

test("all game actions have useful categories in the bindings editor", () => {
  for (const action of GAME_ACTION_REGISTRY.actions) {
    expect(action.categoryPath?.length).toBeGreaterThan(0);
    expect(action.categoryPath).not.toContain("Other");
  }
});

test("the binding editor starts loading at initialization, before the Controls menu is opened", () => {
  const names = ["dialog", "close", "categories", "categorySelect", "search", "resetCategory", "content", "status", "resetAll", "exportUser", "importUser"];
  const elements = Object.fromEntries(names.map((name) => [name, { addEventListener() {}, disabled: false }])) as unknown as SettingsPanelElements;
  let editorLoads = 0;
  let characterReads = 0;
  new SettingsPanel(elements, {
    preferences: new Promise<Preferences>(() => {}),
    bindings: () => { characterReads += 1; throw new Error("The profile is only needed when the editor mounts"); },
    onOpenChange() {},
    loadBindingsEditor: () => {
      editorLoads += 1;
      return Promise.resolve({ mountBindingsEditor: () => () => {} });
    },
  });
  expect(editorLoads).toBe(1);
  expect(characterReads).toBe(0);
});

describe("per-character binding profiles (#41 Q21)", () => {
  test("a rebinding saved for one character applies to it alone and survives a reload", () => {
    const storage = new MemoryStorage();
    const jumpOnJ = {
      id: "character:a",
      patches: [{ op: "replace" as const, bindingId: "move.jump.0", binding: { id: "move.jump.0", action: "move.jump", sequence: [{ key: { kind: "physical" as const, value: "KeyJ" } }], when: { op: "context" as const, id: "gameplay" } } }],
    };
    saveBindingProfile(storage, "a", jumpOnJ);
    const reloaded = loadBindingProfile(storage, "a");
    expect(reloaded.problem).toBeNull();
    expect(loadBindingProfile(storage, "b")).toEqual({ profile: { id: "character:b", patches: [] }, problem: null });

    const app = harness(() => ({ phase: "world", panelOpen: false }));
    app.controls.setProfile(reloaded.profile);
    app.window.emit("keydown", keyEvent("KeyJ"));
    app.window.emit("keydown", keyEvent("Space"));
    expect(app.actions).toEqual(["move.jump"]);
  });

  test("an unreadable profile fails closed to the defaults with a reason", () => {
    const storage = new MemoryStorage();
    storage.setItem(bindingProfileKey("a"), "{not json");
    const loaded = loadBindingProfile(storage, "a");
    expect(loaded.profile.patches).toEqual([]);
    expect(loaded.problem).toContain("using defaults");
  });
});

describe("client preferences over the shared settings session", () => {
  test("a changed field of view persists, reloads and resets", async () => {
    const storage = new MemoryStorage();
    const first = await Preferences.open(storage);
    expect(first.values()).toEqual({ fieldOfViewDegrees: 50, lookSensitivity: 1, invertVerticalLook: false, reduceMotion: "system" });
    first.set(SETTING.fieldOfView, { type: "integer", value: 70 });
    first.set(SETTING.reduceMotion, { type: "choice", value: "reduce" });
    expect(() => first.set(SETTING.fieldOfView, { type: "integer", value: 400 })).toThrow();

    const second = await Preferences.open(storage);
    expect(second.problem).toBeNull();
    expect(second.values()).toEqual({ fieldOfViewDegrees: 70, lookSensitivity: 1, invertVerticalLook: false, reduceMotion: "reduce" });
    second.resetAll();
    expect((await Preferences.open(storage)).values().fieldOfViewDegrees).toBe(50);
  });
});

test("look inversion persists through the shared catalog and reverses only vertical orbit", async () => {
  const { OrbitCamera } = await import("../src/world/orbit-camera");
  const storage = new MemoryStorage();
  const preferences = await Preferences.open(storage);
  preferences.set(SETTING.invertVerticalLook, { type: "bool", value: true });
  expect((await Preferences.open(storage)).values().invertVerticalLook).toBe(true);
  const normal = new OrbitCamera();
  const inverted = new OrbitCamera();
  inverted.setLookInversion(true);
  normal.orbit(8, 5);
  inverted.orbit(8, -5);
  expect(inverted.view([0, 0, 0])).toEqual(normal.view([0, 0, 0]));
});

test("User profile preview is isolated and confirmed import replaces destination-only overrides", async () => {
  const storage = new MemoryStorage();
  const destination = await Preferences.open(storage);
  destination.set(SETTING.fieldOfView, { type: "integer", value: 65 });
  const source = await Preferences.open(new MemoryStorage());
  source.set(SETTING.lookSensitivity, { type: "number", value: 2 });
  const preview = await destination.previewUserImport(source.exportUserProfile());
  expect(preview.changes.some((change) => change.id === SETTING.fieldOfView && change.operation === "Reset to default")).toBe(true);
  expect(destination.values().fieldOfViewDegrees).toBe(65);
  expect((await Preferences.open(storage)).values().fieldOfViewDegrees).toBe(65);
  destination.confirmUserImport(preview);
  expect(destination.values().fieldOfViewDegrees).toBe(50);
  expect((await Preferences.open(storage)).values().lookSensitivity).toBe(2);
});

test("cancelled imports, invalid documents and failed durable writes do not change live preferences", async () => {
  const storage = new MemoryStorage();
  const destination = await Preferences.open(storage);
  const source = await Preferences.open(new MemoryStorage());
  source.set(SETTING.fieldOfView, { type: "integer", value: 70 });
  const cancelled = await destination.previewUserImport(source.exportUserProfile());
  destination.cancelUserImport();
  expect(() => destination.confirmUserImport(cancelled)).toThrow();
  expect(destination.values().fieldOfViewDegrees).toBe(50);
  await expect(destination.previewUserImport("broken")).rejects.toThrow();
  const failing = await Preferences.open({ getItem: () => null, setItem: () => { throw new Error("storage unavailable"); } });
  const pending = await failing.previewUserImport(source.exportUserProfile());
  expect(() => failing.confirmUserImport(pending)).toThrow("storage unavailable");
  expect(failing.values().fieldOfViewDegrees).toBe(50);
});

test("the shared portable profile round-trips across clients and dormant entries survive resets", async () => {
  const { default: portable } = await import("../../fixtures/settings/user-profile-v2.json");
  const preferences = await Preferences.open(new MemoryStorage());
  const preview = await preferences.previewUserImport(JSON.stringify(portable));
  expect(preview.diagnostics.length).toBeGreaterThan(0);
  preferences.confirmUserImport(preview);
  expect(JSON.parse(preferences.exportUserProfile())).toEqual(portable);
  preferences.resetAll();
  expect(JSON.parse(preferences.exportUserProfile()).overrides).toEqual({ "other_platform.preference": portable.overrides["other_platform.preference"] });
});

test("a confirmed import with unchanged effective preferences does not reconfigure the client", async () => {
  const preferences = await Preferences.open(new MemoryStorage());
  let applied = 0;
  preferences.subscribe(() => { applied += 1; });
  const preview = await preferences.previewUserImport(preferences.exportUserProfile());
  preferences.confirmUserImport(preview);
  expect(applied).toBe(1);
});
