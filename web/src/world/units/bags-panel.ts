import type { WorldCommand } from "../../command-wire";
import type { ZoneSnapshot } from "../../replication";
import type { ContentCatalog } from "../catalog";
import { BagState } from "./bag-state";
import { itemStatsText } from "./character-pane";

export type BagsElements = {
  panel: HTMLElement;
  toggle: HTMLButtonElement;
  close: HTMLButtonElement;
  slots: HTMLElement;
  quantity: HTMLInputElement;
  /** Offers EquipItem for a selected stack whose catalog item has an equipment slot. */
  equip: HTMLButtonElement;
  selection: HTMLElement;
  status: HTMLElement;
  feedback: HTMLElement;
};

/** DOM controls send bounded intent; only received self sheets draw stacks. */
export class BagsPanel {
  readonly #state = new BagState();
  readonly #elements: BagsElements;
  readonly #buttons: HTMLButtonElement[];
  readonly #send: (command: WorldCommand, onSent: (sequence: number | null) => void) => void;
  #catalog: ContentCatalog | null = null;
  #selected: number | null = null;

  constructor(elements: BagsElements, send: (command: WorldCommand, onSent: (sequence: number | null) => void) => void) {
    this.#elements = elements;
    this.#send = send;
    this.#buttons = Array.from({ length: 16 }, (_, slot) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "bag-slot";
      button.dataset.slot = String(slot);
      button.addEventListener("click", () => this.#click(slot));
      elements.slots.append(button);
      return button;
    });
    elements.equip.addEventListener("click", () => {
      const command = this.#selected === null ? null : this.#state.equip(this.#selected);
      if (command) {
        this.#send(command, (sequence) => this.#state.sent(sequence));
        this.#selected = null;
      }
      this.#render();
    });
    elements.toggle.addEventListener("click", () => this.toggle());
    elements.close.addEventListener("click", () => this.close());
    elements.panel.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        this.close();
      }
    });
    this.reset();
  }

  load(catalog: ContentCatalog): void { this.#catalog = catalog; }

  /** The received bag, equipment and stat cache this panel keeps current. */
  get state(): BagState { return this.#state; }

  reset(snapshot: ZoneSnapshot | null = null): void {
    this.#state.reset(snapshot);
    this.#selected = null;
    this.#elements.panel.hidden = true;
    this.#elements.toggle.setAttribute("aria-expanded", "false");
    if (snapshot) {
      this.#state.update(snapshot);
    }
    this.#render();
  }

  get open(): boolean {
    return !this.#elements.panel.hidden;
  }

  toggle(): void {
    if (!this.#elements.panel.hidden) {
      this.close();
      return;
    }
    this.#elements.panel.hidden = false;
    this.#elements.toggle.setAttribute("aria-expanded", "true");
    this.#elements.close.focus();
  }

  close(): boolean {
    if (this.#elements.panel.hidden) {
      return false;
    }
    this.#elements.panel.hidden = true;
    this.#elements.toggle.setAttribute("aria-expanded", "false");
    this.#elements.toggle.focus();
    return true;
  }

  update(snapshot: ZoneSnapshot): void {
    const revision = this.#state.revision;
    if (!this.#state.update(snapshot)) {
      return;
    }
    if (revision !== this.#state.revision || !this.#state.canMoveItems) {
      this.#selected = null;
    }
    this.#render();
  }

  #click(slot: number): void {
    if (!this.#state.canMoveItems) {
      return;
    }
    if (this.#selected === slot) {
      this.#selected = null;
    } else if (this.#selected === null) {
      const stack = this.#state.slots?.[slot];
      if (!stack) {
        return;
      }
      this.#selected = slot;
      this.#elements.quantity.value = String(stack.quantity);
      this.#elements.quantity.max = String(stack.quantity);
    } else {
      const command = this.#state.move(this.#selected, slot, this.#elements.quantity.valueAsNumber);
      if (command) {
        this.#send(command, (sequence) => this.#state.sent(sequence));
        this.#selected = null;
      } else {
        this.#elements.quantity.reportValidity();
      }
    }
    this.#render();
  }

  #render(): void {
    const { status, feedback, selection, quantity, equip } = this.#elements;
    if (status.textContent !== this.#state.status) {
      status.textContent = this.#state.status;
    }
    if (feedback.textContent !== this.#state.feedback) {
      feedback.textContent = this.#state.feedback;
    }
    quantity.disabled = this.#selected === null || !this.#state.canMoveItems;
    const hint = this.#selected === null ? "Choose an occupied slot." : `Move from slot ${this.#selected + 1}. Choose a quantity and destination.`;
    if (selection.textContent !== hint) {
      selection.textContent = hint;
    }
    const selectedStack = this.#selected === null ? null : this.#state.slots?.[this.#selected];
    const selectedItem = selectedStack ? this.#catalog?.items.get(selectedStack.itemId) : null;
    equip.hidden = !selectedItem?.slot;
    equip.disabled = !this.#state.canChangeEquipment;
    const equipText = selectedItem?.slot ? `Equip ${selectedItem.name}` : "Equip";
    if (equip.textContent !== equipText) {
      equip.textContent = equipText;
    }
    for (const [slot, button] of this.#buttons.entries()) {
      const stack = this.#state.slots?.[slot];
      const item = stack ? this.#catalog?.items.get(stack.itemId) : null;
      if (stack && !item) {
        throw new Error("Projected item is missing from the content catalog.");
      }
      let content = this.#state.slots ? "Empty" : "Waiting";
      if (stack) {
        content = `${item?.name} × ${stack.quantity}`;
      }
      const text = `Slot ${slot + 1} · ${content}`;
      if (button.textContent !== text) {
        button.textContent = text;
      }
      const stats = item ? itemStatsText(item.stats) : "";
      if (stats) {
        button.title = stats;
      } else {
        button.removeAttribute("title");
      }
      button.disabled = !this.#state.canMoveItems;
      button.setAttribute("aria-pressed", String(this.#selected === slot));
    }
  }
}
