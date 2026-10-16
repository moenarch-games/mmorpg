import type { ActionRegistry, Binding, ContextLayer, InputStroke } from "@moritzbrantner/input-bindings";
import { InputRuntimeController, SemanticControlState } from "@moritzbrantner/input-bindings-runtime";
import {
  attachGamepadRuntime,
  attachKeyboardRuntime,
  type FrameScheduler,
  type GamepadLike,
  type RuntimeEventTargetLike,
  type VisibilityEventTargetLike,
} from "@moritzbrantner/input-bindings-web";
import type { HeldIntent } from "../world/orbit-camera";

// MMORPG owns this catalog, its defaults and its contexts. `input-bindings` only normalizes
// keyboard/gamepad input, resolves it through the context stack and retires held actions;
// what an action does (and the per-tick command stream) stays in the client and the zone.

export const GAME_ACTIONS = [
  "move.forward",
  "move.back",
  "move.strafeLeft",
  "move.strafeRight",
  "move.jump",
  "target.next",
  "combat.toggleAutoAttack",
  "ability.slot1",
  "ability.slot2",
  "ability.slot3",
  "ability.slot4",
  "combat.cancelCast",
  "player.releaseSpirit",
  "ui.toggleBags",
  "ui.toggleCharacter",
  "ui.toggleVendor",
  "ui.talk",
  "ui.toggleQuestLog",
  "ui.openChat",
  "ui.closePanel",
  "ui.leaveWorld",
  "ui.toggleDebug",
  "ui.enterWorld",
  "ui.cancelCreation",
] as const;
export type GameAction = (typeof GAME_ACTIONS)[number];

/**
 * - `selection` / `creation`: character selection, with creation as a modal layer;
 * - `world` < `gameplay` < `casting` < `panels`: while the viewer casts, Escape cancels the cast
 *   instead of leaving; bags, the character pane, loot, the vendor or the quest dialog and log open as a non-blocking overlay
 *   that claims Escape first but
 *   lets movement fall through to gameplay.
 */
export type GameContext = "selection" | "creation" | "world" | "gameplay" | "casting" | "panels";

export type ControlScreen =
  | { phase: "selection"; creating: boolean }
  | { phase: "world"; panelOpen: boolean; casting?: boolean };

export function contextStack(screen: ControlScreen): ContextLayer[] {
  if (screen.phase === "selection") {
    return screen.creating
      ? [{ id: "selection" }, { id: "creation", blocksLower: true }]
      : [{ id: "selection" }];
  }
  return [
    { id: "world" },
    { id: "gameplay" },
    ...(screen.casting ? [{ id: "casting" }] : []),
    ...(screen.panelOpen ? [{ id: "panels" }] : []),
  ];
}

const key = (code: string): InputStroke => ({ key: { kind: "physical", value: code } });
const stick = (axis: number, direction: "positive" | "negative"): InputStroke => ({
  device: "gamepadAxis",
  axis,
  direction,
  threshold: 50,
  deadzone: 20,
});
const button = (index: number): InputStroke => ({ device: "gamepadButton", button: index, threshold: 50 });

function action(
  id: GameAction,
  title: string,
  context: GameContext,
  strokes: readonly InputStroke[],
): ActionRegistry["actions"][number] {
  return {
    id,
    title,
    repeatPolicy: "never",
    allowedDevices: ["keyboard", "gamepad"],
    defaults: strokes.map((stroke, index): Binding => ({
      id: `${id}.${index}`,
      action: id,
      sequence: [stroke],
      when: { op: "context", id: context },
    })),
    provenance: { source: "mmorpg", version: "1" },
  };
}

/**
 * Standard-mapping gamepad: left stick moves, A jumps, B backs out (or cancels a cast), X attacks,
 * Y releases, and the d-pad (up, right, down, left) uses ability slots 1–4.
 */
export const GAME_ACTION_REGISTRY: ActionRegistry = {
  actions: [
    action("move.forward", "Run forward", "gameplay", [key("KeyW"), key("ArrowUp"), stick(1, "negative")]),
    action("move.back", "Backpedal", "gameplay", [key("KeyS"), key("ArrowDown"), stick(1, "positive")]),
    action("move.strafeLeft", "Strafe left", "gameplay", [
      key("KeyA"),
      key("KeyQ"),
      key("ArrowLeft"),
      stick(0, "negative"),
    ]),
    action("move.strafeRight", "Strafe right", "gameplay", [
      key("KeyD"),
      key("KeyE"),
      key("ArrowRight"),
      stick(0, "positive"),
    ]),
    action("move.jump", "Jump", "gameplay", [key("Space"), button(0)]),
    action("target.next", "Target next", "gameplay", [key("Tab"), button(5)]),
    action("combat.toggleAutoAttack", "Toggle auto-attack", "gameplay", [key("KeyF"), button(2)]),
    action("ability.slot1", "Ability 1", "gameplay", [key("Digit1"), button(12)]),
    action("ability.slot2", "Ability 2", "gameplay", [key("Digit2"), button(15)]),
    action("ability.slot3", "Ability 3", "gameplay", [key("Digit3"), button(13)]),
    action("ability.slot4", "Ability 4", "gameplay", [key("Digit4"), button(14)]),
    action("combat.cancelCast", "Cancel cast", "casting", [key("Escape"), button(1)]),
    action("player.releaseSpirit", "Release spirit", "gameplay", [key("KeyR"), button(3)]),
    action("ui.toggleBags", "Toggle bags", "gameplay", [key("KeyB"), button(8)]),
    action("ui.toggleCharacter", "Toggle character pane", "gameplay", [key("KeyC")]),
    action("ui.toggleVendor", "Toggle vendor", "gameplay", [key("KeyV")]),
    action("ui.talk", "Talk to a quest giver", "gameplay", [key("KeyT")]),
    action("ui.toggleQuestLog", "Toggle quest log", "gameplay", [key("KeyL")]),
    action("ui.openChat", "Chat", "gameplay", [key("Enter"), key("NumpadEnter")]),
    action("ui.closePanel", "Close panel", "panels", [key("Escape"), button(1)]),
    action("ui.leaveWorld", "Return to characters", "gameplay", [key("Escape"), button(1)]),
    action("ui.toggleDebug", "Toggle debug overlay", "world", [key("F3")]),
    action("ui.enterWorld", "Enter world", "selection", [key("Enter"), button(9)]),
    action("ui.cancelCreation", "Cancel character creation", "creation", [key("Escape"), button(1)]),
  ],
};

/** Text-like controls and the character turntable own every key. */
const TEXT_LIKE_CONTROL_SELECTOR = "input, select, textarea, [contenteditable=true], [role=slider]";
/**
 * Buttons and links own only their activation, focus and dismissal keys (panels close themselves
 * on Escape); other keys such as movement still reach gameplay while an overlay button has focus.
 */
const BUTTON_LIKE_CONTROL_SELECTOR = "button, summary, a";
const BUTTON_OWNED_CODES = new Set(["Enter", "NumpadEnter", "Space", "Tab", "Escape"]);

export type GameControlsTargets = {
  window: RuntimeEventTargetLike;
  document: VisibilityEventTargetLike;
  getGamepads?: (() => readonly (GamepadLike | null)[]) | undefined;
  frameScheduler?: FrameScheduler | undefined;
};

export type GameControlsOptions = {
  screen: () => ControlScreen;
  onAction: (action: GameAction) => void;
};

/**
 * The client's one input path: keyboard and gamepad resolve to semantic actions through the
 * shared runtime; held movement is sampled per frame and one-shot actions dispatch once.
 */
export class GameControls {
  readonly #controller: InputRuntimeController;
  readonly #state = new SemanticControlState();

  constructor(options: GameControlsOptions) {
    this.#controller = new InputRuntimeController({
      registry: GAME_ACTION_REGISTRY,
      getActiveContexts: () => new Set(),
      getContextStack: () => contextStack(options.screen()),
      consumePolicy: "matched",
      onDispatch: (dispatch) => {
        this.#state.apply(dispatch);
        if (dispatch.phase === "press" && isGameAction(dispatch.action)) {
          options.onAction(dispatch.action);
        }
      },
    });
    const report = this.#controller.validationReport;
    if (!report.valid) {
      throw new Error(`Invalid MMORPG control catalog: ${JSON.stringify(report.diagnostics)}`);
    }
  }

  /** Attaches keyboard and gamepad input; returns a detach function that releases everything. */
  attach(targets: GameControlsTargets): () => void {
    const detachKeyboard = attachKeyboardRuntime(this.#controller, {
      keyTarget: ignoringNativeControls(targets.window),
      focusTarget: targets.window,
      visibilityTarget: targets.document,
      mode: "physical",
      ignoreTextEntry: true,
    });
    const detachGamepad = attachGamepadRuntime(this.#controller, {
      ...(targets.getGamepads ? { getGamepads: targets.getGamepads } : {}),
      ...(targets.frameScheduler ? { scheduler: targets.frameScheduler } : {}),
    });
    // Typing into a text-like control must not leave movement held; buttons do not take
    // movement keys, so focusing an overlay button keeps running.
    const onFocusIn = (event: unknown) => {
      if (ownsEveryKey((event as { target?: unknown } | null)?.target)) {
        this.retire("focusChanged");
      }
    };
    targets.document.addEventListener("focusin", onFocusIn);
    return () => {
      targets.document.removeEventListener("focusin", onFocusIn);
      detachGamepad();
      detachKeyboard();
      this.#state.clear();
    };
  }

  /** Releases every held action, e.g. when the screen or overlay context changes. */
  retire(reason: string): void {
    this.#controller.reset(reason);
  }

  /** Camera-relative movement from the held semantic actions; any held movement steers. */
  heldIntent(): HeldIntent {
    const state = this.#state;
    return {
      forward: state.axis("move.back", "move.forward"),
      strafe: state.axis("move.strafeLeft", "move.strafeRight"),
      steering: ["move.forward", "move.back", "move.strafeLeft", "move.strafeRight"].some((id) =>
        state.isHeld(id),
      ),
    };
  }
}

function isGameAction(action: string): action is GameAction {
  return (GAME_ACTIONS as readonly string[]).includes(action);
}

/** Drops keydowns a focused native control owns; keyups always pass so held keys still release. */
function ignoringNativeControls(target: RuntimeEventTargetLike): RuntimeEventTargetLike {
  const wrapped = new Map<(event: any) => void, (event: any) => void>();
  return {
    addEventListener(type, listener, options) {
      const filtered =
        type === "keydown"
          ? (event: any) => {
              if (!ownedByNativeControl(event?.target, event?.code)) {
                listener(event);
              }
            }
          : listener;
      wrapped.set(listener, filtered);
      target.addEventListener(type, filtered, options);
    },
    removeEventListener(type, listener, options) {
      target.removeEventListener(type, wrapped.get(listener) ?? listener, options);
      wrapped.delete(listener);
    },
  };
}

function ownedByNativeControl(target: unknown, code: unknown): boolean {
  return (
    ownsEveryKey(target) ||
    (matches(target, BUTTON_LIKE_CONTROL_SELECTOR) && typeof code === "string" && BUTTON_OWNED_CODES.has(code))
  );
}

function ownsEveryKey(target: unknown): boolean {
  return matches(target, TEXT_LIKE_CONTROL_SELECTOR);
}

function matches(target: unknown, selector: string): boolean {
  const closest = (target as { closest?: (selector: string) => unknown } | null)?.closest;
  return typeof closest === "function" && Boolean(closest.call(target, selector));
}
