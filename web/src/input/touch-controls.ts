import type { HeldIntent } from "../world/orbit-camera";
import type { GameAction } from "./game-controls";
import { ChordState, FACE_BUTTONS, INTERACT_LABELS, stickIntent, TOUCH_CHORD_PAGES, type TouchContext } from "./touch-layout";

/** One hotbar slot as the face buttons show it: its ability name and the action-bar button it mirrors. */
export type SlotFace = { name: string; source: HTMLElement };

/** The action-bar slot states a face button copies each frame (locked, cooldown, resource, range, …). */
const MIRRORED_DATA = ["data-state", "data-resource", "data-range", "data-gcd"] as const;

export type TouchControlsOptions = {
  /** Fires an existing semantic action, exactly as its key or gamepad button would. */
  onAction: (action: GameAction) => void;
  /** The context interact button: loot, trade, talk, open or repair. */
  onInteract: () => void;
  onSettings: () => void;
  onChat: () => void;
  context: () => TouchContext;
  /** A key that changes whenever the hotbar does. */
  facesKey: () => string;
  /** Slots 1–8 with fresh icon copies, `null` for an empty slot. */
  faces: () => readonly (SlotFace | null)[];
};

/** Stick travel in CSS pixels at full deflection. */
const STICK_RADIUS_PX = 48;

const MENU_ITEMS: readonly { label: string; action: GameAction | "settings" | "chat" }[] = [
  { label: "Bags", action: "ui.toggleBags" },
  { label: "Character", action: "ui.toggleCharacter" },
  { label: "Quests", action: "ui.toggleQuestLog" },
  { label: "Chat", action: "chat" },
  { label: "Settings", action: "settings" },
  { label: "Characters", action: "ui.leaveWorld" },
];

/**
 * The touch overlay (#434): a left virtual thumbstick, a jump button, four face buttons with a
 * modifier for 2-tap chords and a hint ring, context buttons (target, attack, interact, release)
 * and a compact menu. It only fires the existing semantic actions and samples held movement;
 * shown by CSS while the page is in touch input mode.
 */
export class TouchControls {
  readonly #options: TouchControlsOptions;
  readonly #chord = new ChordState();
  readonly #stick: { zone: HTMLElement; base: HTMLElement; knob: HTMLElement };
  readonly #faces: HTMLButtonElement[];
  readonly #hints: HTMLElement[];
  /** The action-bar buttons the four faces currently mirror. */
  readonly #sources: (HTMLElement | null)[] = [null, null, null, null];
  readonly #ring: HTMLElement;
  readonly #modifier: HTMLButtonElement;
  readonly #menu: HTMLElement;
  readonly #context: { target: HTMLButtonElement; attack: HTMLButtonElement; interact: HTMLButtonElement; release: HTMLButtonElement };
  #drag: { pointerId: number; x: number; y: number } | null = null;
  #vector = { x: 0, y: 0 };
  #jumpPointer: number | null = null;
  #shownKey = "";

  constructor(root: HTMLElement, options: TouchControlsOptions) {
    this.#options = options;
    const zone = element("div", "touch-stick-zone");
    const base = element("div", "touch-stick");
    const knob = element("span", "touch-stick-knob");
    base.append(knob);
    base.hidden = true;
    zone.append(base);
    this.#stick = { zone, base, knob };
    zone.addEventListener("pointerdown", (event) => this.#stickDown(event));
    zone.addEventListener("pointermove", (event) => this.#stickMove(event));
    for (const name of ["pointerup", "pointercancel", "lostpointercapture"] as const) {
      zone.addEventListener(name, (event) => {
        if (this.#drag?.pointerId === event.pointerId) {
          this.#releaseStick();
        }
      });
    }

    const top = element("div", "touch-top");
    const chat = button("touch-pill", "Chat", () => this.#options.onChat());
    const menuToggle = button("touch-pill", "Menu", () => this.#toggleMenu());
    menuToggle.setAttribute("aria-haspopup", "menu");
    this.#menu = element("div", "touch-menu");
    this.#menu.setAttribute("role", "menu");
    this.#menu.hidden = true;
    for (const item of MENU_ITEMS) {
      const entry = button("touch-menu-item", item.label, () => {
        this.#menu.hidden = true;
        if (item.action === "settings") {
          this.#options.onSettings();
        } else if (item.action === "chat") {
          this.#options.onChat();
        } else {
          this.#options.onAction(item.action);
        }
      });
      entry.setAttribute("role", "menuitem");
      this.#menu.append(entry);
    }
    top.append(chat, menuToggle, this.#menu);

    const context = element("div", "touch-context");
    this.#context = {
      target: button("touch-pill", "Target", () => this.#options.onAction("target.next")),
      attack: button("touch-pill", "Attack", () => this.#options.onAction("combat.toggleAutoAttack")),
      interact: button("touch-pill touch-interact", "Interact", () => this.#options.onInteract()),
      release: button("touch-pill", "Release spirit", () => this.#options.onAction("player.releaseSpirit")),
    };
    for (const entry of Object.values(this.#context)) {
      entry.hidden = true;
    }
    context.append(this.#context.target, this.#context.attack, this.#context.interact, this.#context.release);

    const cluster = element("div", "touch-faces");
    this.#ring = element("div", "touch-hint-ring");
    this.#ring.setAttribute("aria-hidden", "true");
    this.#hints = FACE_BUTTONS.map((face) => {
      const hint = element("span", "touch-hint");
      hint.dataset.face = face;
      this.#ring.append(hint);
      return hint;
    });
    this.#faces = FACE_BUTTONS.map((face, index) => {
      const entry = button("touch-face action-slot", "", () => {
        const action = this.#chord.press(index, performance.now());
        if (action) {
          this.#options.onAction(action);
        }
        this.update();
      });
      entry.dataset.face = face;
      return entry;
    });
    this.#modifier = button("touch-modifier", "⇧", () => {
      this.#chord.toggleModifier(performance.now());
      this.update();
    });
    this.#modifier.setAttribute("aria-label", "Next ability page");
    cluster.append(this.#ring, ...this.#faces, this.#modifier);

    const jump = element("button", "touch-jump") as HTMLButtonElement;
    jump.type = "button";
    jump.tabIndex = -1;
    jump.textContent = "Jump";
    // Held like Space: a press jumps (or swims up while held); the release stops rising.
    jump.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      this.#jumpPointer = event.pointerId;
      this.#options.onAction("move.jump");
    });
    for (const name of ["pointerup", "pointercancel", "pointerleave"] as const) {
      jump.addEventListener(name, (event) => {
        if (this.#jumpPointer === event.pointerId) {
          this.#jumpPointer = null;
        }
      });
    }

    root.replaceChildren(zone, top, context, cluster, jump);
  }

  /** Held movement from the thumbstick and the jump button. */
  heldIntent(): HeldIntent {
    return stickIntent(this.#vector.x, this.#vector.y, this.#jumpPointer !== null);
  }

  /** Releases the stick, jump, chord and menu, e.g. on blur or leaving the world. */
  reset(): void {
    this.#releaseStick();
    this.#jumpPointer = null;
    this.#chord.reset();
    this.#menu.hidden = true;
    this.#shownKey = "";
  }

  /** Per frame: context buttons and the faces of the page the next tap fires. */
  update(): void {
    const now = performance.now();
    const context = this.#options.context();
    this.#context.target.hidden = !context.combat;
    this.#context.attack.hidden = !context.combat;
    this.#context.release.hidden = !context.dead;
    this.#context.interact.hidden = context.interact === null;
    if (context.interact !== null && this.#context.interact.textContent !== INTERACT_LABELS[context.interact]) {
      this.#context.interact.textContent = INTERACT_LABELS[context.interact];
    }
    const page = this.#chord.page(now);
    const key = `${page}:${this.#options.facesKey()}`;
    if (key !== this.#shownKey) {
      this.#shownKey = key;
      this.#showPage(page);
    }
    this.#mirrorSlots();
  }

  /** Shows the slots the next tap fires: the action bar's own icon, cooldown sweep and timer, and the slot number. */
  #showPage(page: number): void {
    const faces = this.#options.faces();
    this.#ring.dataset.page = String(page);
    this.#modifier.setAttribute("aria-pressed", String(page === 1));
    TOUCH_CHORD_PAGES[page]?.forEach((action, index) => {
      const slot = Number(action.slice(-1));
      const face = faces[slot - 1] ?? null;
      const entry = this.#faces[index];
      const hint = this.#hints[index];
      if (!entry || !hint) {
        return;
      }
      this.#sources[index] = face?.source ?? null;
      const number = element("kbd", "slot-key");
      number.textContent = String(slot);
      const parts = face
        ? [".slot-icon", ".slot-sweep", ".slot-timer"].flatMap((selector) => {
          const part = face.source.querySelector(selector);
          return part ? [part.cloneNode(true)] : [];
        })
        : [];
      entry.replaceChildren(...parts, number);
      entry.disabled = face === null;
      entry.dataset.empty = String(face === null);
      for (const name of MIRRORED_DATA) {
        entry.removeAttribute(name);
      }
      entry.style.removeProperty("--sweep");
      entry.setAttribute("aria-label", face ? `${face.name} (ability ${slot})` : `Ability ${slot} (empty)`);
      // The hint ring names what the next tap on each face fires.
      hint.textContent = face?.name ?? "";
      hint.hidden = face === null;
    });
  }

  /** Copies each shown slot's live state (locked, cooldown sweep and seconds, resource, range). */
  #mirrorSlots(): void {
    this.#sources.forEach((source, index) => {
      const entry = this.#faces[index];
      if (!source || !entry) {
        return;
      }
      for (const name of MIRRORED_DATA) {
        const value = source.getAttribute(name);
        if (value === null) {
          entry.removeAttribute(name);
        } else if (entry.getAttribute(name) !== value) {
          entry.setAttribute(name, value);
        }
      }
      const sweep = source.style.getPropertyValue("--sweep");
      if (entry.style.getPropertyValue("--sweep") !== sweep) {
        entry.style.setProperty("--sweep", sweep);
      }
      const timer = source.querySelector(".slot-timer")?.textContent ?? "";
      const shown = entry.querySelector(".slot-timer");
      if (shown && shown.textContent !== timer) {
        shown.textContent = timer;
      }
    });
  }

  #toggleMenu(): void {
    this.#menu.hidden = !this.#menu.hidden;
  }

  #stickDown(event: PointerEvent): void {
    if (this.#drag) {
      return;
    }
    event.preventDefault();
    try {
      this.#stick.zone.setPointerCapture(event.pointerId);
    } catch {
      // A synthetic or already-ended pointer still steers while it stays over the zone.
    }
    const rect = this.#stick.zone.getBoundingClientRect();
    this.#drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY };
    this.#stick.base.hidden = false;
    this.#stick.base.style.left = `${event.clientX - rect.left}px`;
    this.#stick.base.style.top = `${event.clientY - rect.top}px`;
    this.#placeKnob(0, 0);
  }

  #stickMove(event: PointerEvent): void {
    const drag = this.#drag;
    if (drag?.pointerId !== event.pointerId) {
      return;
    }
    let dx = event.clientX - drag.x;
    let dy = event.clientY - drag.y;
    const length = Math.hypot(dx, dy);
    if (length > STICK_RADIUS_PX) {
      dx *= STICK_RADIUS_PX / length;
      dy *= STICK_RADIUS_PX / length;
    }
    this.#placeKnob(dx, dy);
  }

  #placeKnob(dx: number, dy: number): void {
    this.#vector = { x: dx / STICK_RADIUS_PX, y: dy / STICK_RADIUS_PX };
    this.#stick.knob.style.transform = `translate(calc(-50% + ${dx}px), calc(-50% + ${dy}px))`;
  }

  #releaseStick(): void {
    const drag = this.#drag;
    this.#drag = null;
    if (drag && this.#stick.zone.hasPointerCapture?.(drag.pointerId)) {
      this.#stick.zone.releasePointerCapture(drag.pointerId);
    }
    this.#vector = { x: 0, y: 0 };
    this.#stick.base.hidden = true;
  }
}

/**
 * Touch input mode: on while the last pointer was a finger (or the primary pointer is coarse),
 * off after a mouse press, so hybrid laptops keep the desktop layout with a mouse.
 */
export function trackInputMode(root: HTMLElement, target: Window): void {
  const coarse = typeof target.matchMedia === "function" && target.matchMedia("(pointer: coarse)").matches;
  root.dataset.input = coarse ? "touch" : "pointer";
  target.addEventListener("pointerdown", (event) => {
    if (event.pointerType === "touch") {
      root.dataset.input = "touch";
    } else if (event.pointerType === "mouse") {
      root.dataset.input = "pointer";
    }
  }, { capture: true });
}

function element(tag: string, className: string): HTMLElement {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}

/** Touch buttons act on press, never take focus and ignore the later synthetic click. */
function button(className: string, label: string, onPress: () => void): HTMLButtonElement {
  const node = element("button", className) as HTMLButtonElement;
  node.type = "button";
  node.tabIndex = -1;
  node.textContent = label;
  node.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    if (!node.disabled) {
      onPress();
    }
  });
  // Keyboard or assistive activation (no pointer) still works.
  node.addEventListener("click", (event) => {
    if (event.detail === 0) {
      onPress();
    }
  });
  return node;
}
