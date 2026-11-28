import type { ActionRegistry, Binding, ContextLayer, InputStroke, Profile } from "@moritzbrantner/input-bindings";
import { InputRuntimeController, SemanticControlState } from "@moritzbrantner/input-bindings-runtime";
import {
  attachGamepadRuntime,
  attachKeyboardRuntime,
  type FrameScheduler,
  type GamepadLike,
  type RuntimeEventTargetLike,
  type VisibilityEventTargetLike,
} from "@moritzbrantner/input-bindings-web";
import sharedCatalog from "../../../content/settings/input-actions.json";
import type { HeldIntent } from "../world/orbit-camera";

// MMORPG owns this catalog, its defaults and its contexts. `input-bindings` only normalizes
// keyboard/gamepad input, resolves it through the context stack and retires held actions;
// what an action does (and the per-tick command stream) stays in the client and the zone.
// The actions both clients bind (ids, titles, contexts, keys, pad buttons and stick thresholds)
// are `content/settings/input-actions.json`, in `input-bindings`' own registry format, which the
// native client reads too (#1284); this file adds only the browser's screen and menu actions.

/** Every action id the browser binds: the shared catalog's, then the browser-only ones. */
export const GAME_ACTIONS = [
  "move.forward",
  "move.back",
  "move.strafeLeft",
  "move.strafeRight",
  "move.jump",
  "move.roll",
  "move.descend",
  "target.next",
  "combat.toggleWeapon",
  "combat.swing",
  "ability.slot1",
  "ability.slot2",
  "ability.slot3",
  "ability.slot4",
  "ability.slot5",
  "ability.slot6",
  "ability.slot7",
  "ability.slot8",
  "ability.slot9",
  "ability.slot10",
  "ability.slot11",
  "ability.slot12",
  "ability.slot13",
  "ability.slot14",
  "ability.slot15",
  "ability.slot16",
  "ability.slot17",
  "ability.slot18",
  "ability.slot19",
  "ability.slot20",
  "combat.directedStrike",
  "combat.guard",
  "wheel.fixed",
  "wheel.context",
  "combat.cancelCast",
  "minions.attack",
  "minions.defend",
  "minions.follow",
  "minions.retreat",
  "player.releaseSpirit",
  "ui.toggleBags",
  "ui.toggleCharacter",
  "ui.toggleVendor",
  "ui.talk",
  "ui.toggleQuestLog",
  "ui.toggleWorldMap",
  "ui.openChat",
  "ui.closePanel",
  "ui.leaveWorld",
  "ui.gameMenu",
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
 * - `settings`: the Settings modal blocks every lower context on either screen, so no game key
 *   resolves (or suppresses a browser default) while it is open; the dialog owns its keys.
 */
export type GameContext = "selection" | "creation" | "world" | "gameplay" | "exploration" | "combat" | "casting" | "panels" | "settings";

export type ControlScreen =
  | { phase: "selection"; creating: boolean; settingsOpen?: boolean }
  | { phase: "world"; panelOpen: boolean; weaponDrawn?: boolean; casting?: boolean; settingsOpen?: boolean };

export function contextStack(screen: ControlScreen): ContextLayer[] {
  const settings: ContextLayer[] = screen.settingsOpen ? [{ id: "settings", blocksLower: true }] : [];
  if (screen.phase === "selection") {
    return screen.creating
      ? [{ id: "selection" }, { id: "creation", blocksLower: true }, ...settings]
      : [{ id: "selection" }, ...settings];
  }
  return [
    { id: "world" },
    { id: "gameplay" },
    { id: screen.weaponDrawn ? "combat" : "exploration" },
    ...(screen.casting ? [{ id: "casting" }] : []),
    ...(screen.panelOpen ? [{ id: "panels" }] : []),
    ...settings,
  ];
}

/** The catalog version stored with each portable binding profile, shared with the native client. */
export const GAME_ACTION_REGISTRY_VERSION: number = sharedCatalog.registryVersion;

/**
 * The shared pad guard rule (#156, owner answer #156.Q1 A): a pad holding `triggerButton` past
 * `triggerThreshold` % aims the guard with `stickAxis`, high or low past `deflection` %.
 */
export const GUARD_LANE_RULE: {
  readonly triggerButton: number;
  readonly triggerThreshold: number;
  readonly stickAxis: number;
  readonly deflection: number;
} = sharedCatalog.guardLane;

/** The actions both clients bind; the JSON import widens its string literals, the runtime validates it. */
export const SHARED_ACTIONS = sharedCatalog.actions as unknown as ActionRegistry["actions"];

const key = (code: string): InputStroke => ({ key: { kind: "physical", value: code } });
const button = (index: number): InputStroke => ({ device: "gamepadButton", button: index, threshold: 50 });

/** A browser-only screen or menu action; every one is an interface action. */
function action(
  id: GameAction,
  title: string,
  context: GameContext,
  strokes: readonly InputStroke[],
): ActionRegistry["actions"][number] {
  return {
    id,
    title,
    categoryPath: ["Interface"],
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
 * The shared catalog (standard-mapping gamepad: left stick moves, A jumps, B rolls, #516, and
 * cancels a cast while one runs, X swings, Y releases, the d-pad uses slots 1–4, L3 and RT open the
 * character pane and quest log), then the browser's own screen and menu actions.
 */
export const GAME_ACTION_REGISTRY: ActionRegistry = {
  actions: [
    ...SHARED_ACTIONS,
    action("ui.toggleVendor", "Toggle vendor", "gameplay", [key("KeyV")]),
    // Interact (#158; E since #515): talk to the selected NPC, or open or close a door in reach.
    // E remains an explicit interaction attempt in combat so the drawn-weapon hint is actionable.
    // The native client interacts through its own `world.interact`.
    action("ui.talk", "Interact: talk, or open or close a door", "gameplay", [key("KeyE"), button(9)]),
    action("ui.closePanel", "Close panel", "panels", [key("Escape"), button(1)]),
    // Escape opens the game menu (#636): an open panel claims it first, and the menu holds the
    // return to characters. The touch menu still returns directly.
    action("ui.gameMenu", "Game menu", "gameplay", [key("Escape")]),
    action("ui.leaveWorld", "Return to characters", "gameplay", []),
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

/** Held actions report their whole lifecycle instead of a one-shot press. */
export const HELD_ACTIONS = ["combat.directedStrike", "combat.guard", "wheel.fixed", "wheel.context"] as const;
export type HeldAction = (typeof HELD_ACTIONS)[number];
/**
 * `release` is a deliberate key release; `cancel` is any retirement (blur, hidden page, Settings,
 * a profile or screen change) and must never act.
 */
export type HoldPhase = "press" | "release" | "cancel";

/** Ability slots act on their press and also report its release (#414): hold-to-charge and chords. */
export const RELEASED_ACTIONS = [
  "ability.slot1",
  "ability.slot2",
  "ability.slot3",
  "ability.slot4",
  "ability.slot5",
  "ability.slot6",
  "ability.slot7",
  "ability.slot8",
  "ability.slot9",
  "ability.slot10",
  "ability.slot11",
  "ability.slot12",
  "ability.slot13",
  "ability.slot14",
  "ability.slot15",
  "ability.slot16",
  "ability.slot17",
  "ability.slot18",
  "ability.slot19",
  "ability.slot20",
] as const;
export type ReleasedAction = (typeof RELEASED_ACTIONS)[number];

export type GameControlsOptions = {
  screen: () => ControlScreen;
  onAction: (action: GameAction) => void;
  onHold?: (action: HeldAction, phase: HoldPhase) => void;
  /** A pressed ability slot's key went up (`release`) or its hold was retired (`cancel`). */
  onRelease?: (action: ReleasedAction, phase: Exclude<HoldPhase, "press">) => void;
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
        if (isHeldAction(dispatch.action)) {
          if (dispatch.phase === "press") {
            options.onHold?.(dispatch.action, "press");
          } else if (dispatch.phase === "release") {
            options.onHold?.(dispatch.action, dispatch.reason === "keyUp" ? "release" : "cancel");
          }
          return;
        }
        if (dispatch.phase === "press" && isGameAction(dispatch.action)) {
          options.onAction(dispatch.action);
        } else if (dispatch.phase === "release" && isReleasedAction(dispatch.action)) {
          options.onRelease?.(dispatch.action, dispatch.reason === "keyUp" ? "release" : "cancel");
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
      // Pad connect/disconnect events arrive on the same window as the keys.
      eventTarget: targets.window,
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

  /**
   * Installs a validated binding profile (e.g. the selected character's) for the next input;
   * held actions are retired so a swap never strands movement under the old mapping.
   */
  setProfile(profile: Profile | undefined): void {
    this.#controller.updateProfile(profile);
    this.retire("profileChanged");
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
      // Held Jump ascends while swimming, held Descend descends (#274).
      rise: state.axis("move.descend", "move.jump"),
    };
  }
}

function isHeldAction(action: string): action is HeldAction {
  return (HELD_ACTIONS as readonly string[]).includes(action);
}

export function isReleasedAction(action: string): action is ReleasedAction {
  return (RELEASED_ACTIONS as readonly string[]).includes(action);
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
