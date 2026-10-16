import type { WorldCommand } from "../../command-wire";
import type { StatTotals, ViewerState, ZoneSnapshot } from "../../replication";
import type { ContentCatalog, ItemStats } from "../catalog";
import type { BagState } from "./bag-state";

/** Equipment slot labels by wire index (0 main hand … 5 feet). */
export const EQUIPMENT_SLOT_LABELS: readonly string[] = ["Main hand", "Off hand", "Head", "Chest", "Legs", "Feet"];

const STAT_LABELS: readonly [keyof ItemStats, string][] = [
  ["stamina", "Stamina"],
  ["strength", "Strength"],
  ["agility", "Agility"],
  ["intellect", "Intellect"],
];

/** Catalog attributes as text, for example `+2 Strength, +2 Agility`; empty when the item adds none. */
export function itemStatsText(stats: ItemStats): string {
  return STAT_LABELS.filter(([stat]) => stats[stat] > 0).map(([stat, label]) => `+${stats[stat]} ${label}`).join(", ");
}

export type CharacterSlotView = {
  slot: number;
  label: string;
  /** The equipped item's catalog name, or null when the slot is empty or not received yet. */
  itemName: string | null;
  text: string;
  /** Whether the pane offers Unequip for this slot now. */
  canUnequip: boolean;
};

export type CharacterPaneModel = {
  status: string;
  slots: readonly CharacterSlotView[];
  /** Stamina, Strength, Agility, Intellect, Health and Damage rows. */
  rows: readonly { label: string; value: string }[];
};

/** The parts of the received bag cache the pane reads. */
export type EquipmentView = Pick<BagState, "equipment" | "stats" | "ready" | "canMove" | "canChangeEquipment">;

/**
 * Builds the pane from received facts only: the equipment and stat totals of the last self sheet,
 * the viewer's projected health and damage range, and catalog names and stats.
 */
export function characterPaneModel(
  equipment: EquipmentView,
  viewer: ViewerState | null,
  catalog: ContentCatalog | null,
): CharacterPaneModel {
  let status = "Choose Unequip to move an item into your bag.";
  if (!equipment.ready) {
    status = "Waiting for your equipment…";
  } else if (!equipment.canMove) {
    status = "You cannot change equipment while dead.";
  }
  const slots = EQUIPMENT_SLOT_LABELS.map((label, slot): CharacterSlotView => {
    const itemId = equipment.equipment?.[slot] ?? null;
    if (equipment.equipment === null) {
      return { slot, label, itemName: null, text: `${label} · Waiting`, canUnequip: false };
    }
    if (itemId === null) {
      return { slot, label, itemName: null, text: `${label} · Empty`, canUnequip: false };
    }
    const item = catalog?.items.get(itemId);
    if (!item) {
      throw new Error("Equipped item is missing from the content catalog.");
    }
    const stats = itemStatsText(item.stats);
    return {
      slot,
      label,
      itemName: item.name,
      text: stats ? `${label} · ${item.name} (${stats})` : `${label} · ${item.name}`,
      canUnequip: equipment.canChangeEquipment,
    };
  });
  const totals: StatTotals | null = equipment.stats;
  const rows = [
    ...STAT_LABELS.map(([stat, label]) => ({ label, value: totals ? String(totals[stat]) : "—" })),
    { label: "Health", value: viewer ? `${viewer.health} / ${viewer.maxHealth}` : "—" },
    { label: "Damage", value: viewer ? `${viewer.damage.min}–${viewer.damage.max}` : "—" },
  ];
  return { status, slots, rows };
}

export type CharacterElements = {
  panel: HTMLElement;
  toggle: HTMLButtonElement;
  close: HTMLButtonElement;
  status: HTMLElement;
  slots: HTMLElement;
  stats: HTMLElement;
  feedback: HTMLElement;
};

/**
 * The character pane over the shared received bag cache: it draws equipment and totals from the
 * last self sheet and sends only `UnequipItem`; the zone decides every swap, health change and refusal.
 */
export class CharacterPane {
  readonly #elements: CharacterElements;
  readonly #state: BagState;
  readonly #send: (command: WorldCommand, onSent: (sequence: number | null) => void) => void;
  readonly #slotText: HTMLElement[];
  readonly #unequip: HTMLButtonElement[];
  readonly #values: HTMLElement[];
  #catalog: ContentCatalog | null = null;
  #viewer: ViewerState | null = null;

  constructor(elements: CharacterElements, state: BagState, send: (command: WorldCommand, onSent: (sequence: number | null) => void) => void) {
    this.#elements = elements;
    this.#state = state;
    this.#send = send;
    this.#slotText = [];
    this.#unequip = [];
    for (const [slot, label] of EQUIPMENT_SLOT_LABELS.entries()) {
      const row = document.createElement("li");
      row.className = "equipment-slot";
      row.dataset.slot = String(slot);
      const text = document.createElement("span");
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = "Unequip";
      button.setAttribute("aria-label", `Unequip ${label.toLowerCase()}`);
      button.addEventListener("click", () => {
        const command = this.#state.unequip(slot);
        if (command) {
          this.#send(command, (sequence) => this.#state.sent(sequence));
        }
        this.render();
      });
      row.append(text, button);
      elements.slots.append(row);
      this.#slotText.push(text);
      this.#unequip.push(button);
    }
    this.#values = characterPaneModel(state, null, null).rows.map(({ label }) => {
      const term = document.createElement("dt");
      term.textContent = label;
      const value = document.createElement("dd");
      value.dataset.stat = label.toLowerCase();
      elements.stats.append(term, value);
      return value;
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

  /** Closes the pane and forgets the viewer; the shared bag cache is reset by its owner. */
  reset(snapshot: ZoneSnapshot | null = null): void {
    this.#viewer = snapshot?.viewer ?? null;
    this.#elements.panel.hidden = true;
    this.#elements.toggle.setAttribute("aria-expanded", "false");
    this.render();
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

  close(returnFocus = true): boolean {
    if (this.#elements.panel.hidden) {
      return false;
    }
    this.#elements.panel.hidden = true;
    this.#elements.toggle.setAttribute("aria-expanded", "false");
    if (returnFocus) {
      this.#elements.toggle.focus();
    }
    return true;
  }

  /** Takes the viewer of a projection the shared bag cache already accepted. */
  update(snapshot: ZoneSnapshot): void {
    this.#viewer = snapshot.viewer;
    this.render();
  }

  render(): void {
    const model = characterPaneModel(this.#state, this.#viewer, this.#catalog);
    setText(this.#elements.status, model.status);
    setText(this.#elements.feedback, this.#state.feedback);
    for (const view of model.slots) {
      setText(this.#slotText[view.slot]!, view.text);
      const button = this.#unequip[view.slot]!;
      button.hidden = view.itemName === null;
      button.disabled = !view.canUnequip;
      button.setAttribute("aria-label", view.itemName ? `Unequip ${view.itemName}` : `Unequip ${view.label.toLowerCase()}`);
    }
    for (const [index, row] of model.rows.entries()) {
      setText(this.#values[index]!, row.value);
    }
  }
}

function setText(element: HTMLElement, text: string): void {
  if (element.textContent !== text) {
    element.textContent = text;
  }
}
