import { frontEndModes, gameMenu, type FrontEndMode, type ModeCard } from "./generated/mmorpg-wasm/mmorpg_wasm.js";

/**
 * The selection screen's mode tabs (`mmorpg_client_model::front_end`, drawn the same by the native
 * client): Greyhaven Vale opens the Characters screen, Battle Royale the Loadout screen. The tabs
 * drive the page's activity choice, which owns the rest of the switch.
 */
export class ModeTabs {
  readonly #nav: HTMLElement;
  readonly #activity: HTMLSelectElement;
  readonly #title: HTMLElement;
  readonly #tagline: HTMLElement;
  #cards: ModeCard[] = [];

  constructor(nav: HTMLElement, activity: HTMLSelectElement, title: HTMLElement, tagline: HTMLElement) {
    this.#nav = nav; this.#activity = activity; this.#title = title; this.#tagline = tagline;
    activity.addEventListener("change", () => this.sync());
  }

  /** Draws the tabs once the zone simulation's module has loaded. */
  ready(): void {
    this.#cards = frontEndModes();
    this.#nav.replaceChildren(...this.#cards.map((card) => {
      const tab = document.createElement("button");
      tab.type = "button";
      tab.role = "tab";
      tab.className = "mode-tab";
      tab.dataset.mode = card.mode;
      tab.textContent = card.label;
      tab.addEventListener("click", () => this.#choose(card.mode));
      return tab;
    }));
    this.#nav.addEventListener("keydown", (event) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") { return; }
      event.preventDefault();
      const index = this.#cards.findIndex((card) => card.mode === this.#activity.value);
      const next = this.#cards[(index + (event.key === "ArrowRight" ? 1 : this.#cards.length - 1)) % this.#cards.length];
      if (next) { this.#choose(next.mode); this.#nav.querySelector<HTMLElement>(`[data-mode="${next.mode}"]`)?.focus(); }
    });
    this.sync();
  }

  /** The current mode's card, once loaded. */
  get current(): ModeCard | undefined {
    return this.#cards.find((card) => card.mode === this.#activity.value);
  }

  /** Follows the activity choice: the selected tab, the heading and its tagline. */
  sync(): void {
    const current = this.current;
    for (const tab of this.#nav.querySelectorAll<HTMLButtonElement>(".mode-tab")) {
      const selected = tab.dataset.mode === current?.mode;
      tab.setAttribute("aria-selected", String(selected));
      tab.tabIndex = selected ? 0 : -1;
      tab.disabled = this.#activity.disabled;
    }
    if (current) {
      this.#title.textContent = current.heading;
      this.#tagline.textContent = current.tagline;
    }
  }

  #choose(mode: FrontEndMode): void {
    if (this.#activity.disabled || this.#activity.value === mode) { return; }
    this.#activity.value = mode;
    this.#activity.dispatchEvent(new Event("change"));
  }
}

/** Labels the game menu's buttons (#636) for `mode` from the shared front end. */
export function labelGameMenu(mode: FrontEndMode, buttons: Partial<Record<string, HTMLButtonElement>>): void {
  for (const entry of gameMenu(mode)) {
    const button = buttons[entry.action];
    if (button) { button.textContent = entry.label; }
  }
}
