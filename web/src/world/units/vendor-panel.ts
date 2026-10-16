import type { WorldCommand } from "../../command-wire";
import type { ZoneSnapshot } from "../../replication";
import type { ContentCatalog, ItemStats } from "../catalog";
import type { BagState } from "./bag-state";
import { nearestVendor, VendorState, type NearbyVendor } from "./vendor-state";

export type VendorElements = {
  panel: HTMLElement;
  toggle: HTMLButtonElement;
  close: HTMLButtonElement;
  title: HTMLElement;
  status: HTMLElement;
  offers: HTMLElement;
  sales: HTMLElement;
  feedback: HTMLElement;
};

const STATS: readonly [keyof ItemStats, string][] = [
  ["stamina", "Stamina"], ["strength", "Strength"], ["agility", "Agility"], ["intellect", "Intellect"],
];

function statsText(stats: ItemStats): string {
  return STATS.filter(([stat]) => stats[stat] > 0).map(([stat, label]) => `+${stats[stat]} ${label}`).join(", ");
}

function setText(element: HTMLElement, text: string): void {
  if (element.textContent !== text) {
    element.textContent = text;
  }
}

/** A list of rows, each a label and one action button, rebuilt only when its shape changes. */
class ActionList {
  readonly #root: HTMLElement;
  readonly #onAction: (index: number) => void;
  #rows: { text: HTMLElement; button: HTMLButtonElement }[] = [];

  constructor(root: HTMLElement, onAction: (index: number) => void) {
    this.#root = root;
    this.#onAction = onAction;
  }

  render(rows: readonly { key: number; text: string; action: string; label: string; enabled: boolean }[]): void {
    while (this.#rows.length > rows.length) {
      this.#rows.pop();
      this.#root.lastElementChild?.remove();
    }
    while (this.#rows.length < rows.length) {
      const row = document.createElement("li");
      const text = document.createElement("span");
      const button = document.createElement("button");
      button.type = "button";
      button.addEventListener("click", () => {
        const key = Number(button.dataset.key);
        if (Number.isInteger(key)) {
          this.#onAction(key);
        }
      });
      row.append(text, button);
      this.#root.append(row);
      this.#rows.push({ text, button });
    }
    for (const [index, row] of rows.entries()) {
      const element = this.#rows[index]!;
      setText(element.text, row.text);
      setText(element.button, row.action);
      element.button.dataset.key = String(row.key);
      element.button.setAttribute("aria-label", row.label);
      element.button.disabled = !row.enabled;
    }
  }
}

/** The vendor window: received stock, copper and bag only; the zone performs every trade. */
export class VendorPanel {
  readonly #state = new VendorState();
  readonly #elements: VendorElements;
  readonly #bag: BagState;
  readonly #send: (command: WorldCommand, onSent: (sequence: number | null) => void) => void;
  readonly #onOpen: () => void;
  readonly #offers: ActionList;
  readonly #sales: ActionList;
  #catalog: ContentCatalog | null = null;
  #vendor: NearbyVendor | null = null;

  constructor(elements: VendorElements, bag: BagState, send: (command: WorldCommand, onSent: (sequence: number | null) => void) => void, onOpen: () => void) {
    this.#elements = elements;
    this.#bag = bag;
    this.#send = send;
    this.#onOpen = onOpen;
    this.#offers = new ActionList(elements.offers, (offer) => this.#trade(this.#state.buy(this.#vendor, offer, this.#bag)));
    this.#sales = new ActionList(elements.sales, (slot) => this.#trade(this.#state.sell(this.#vendor, slot, this.#bag)));
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

  reset(snapshot: ZoneSnapshot | null = null): void {
    this.#state.reset();
    this.#vendor = null;
    this.#elements.panel.hidden = true;
    this.#elements.toggle.setAttribute("aria-expanded", "false");
    if (snapshot) {
      this.update(snapshot);
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
    this.#onOpen();
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

  /** Takes a projection the shared bag cache already accepted. */
  update(snapshot: ZoneSnapshot): void {
    this.#state.update(snapshot, this.#bag);
    this.#vendor = this.#catalog ? nearestVendor(snapshot, this.#catalog) : null;
    this.#render();
  }

  #trade(command: WorldCommand | null): void {
    if (command) {
      this.#send(command, (sequence) => this.#state.sent(sequence));
    }
    this.#render();
  }

  #render(): void {
    const vendor = this.#vendor;
    const catalog = this.#catalog;
    setText(this.#elements.title, vendor?.name ?? "Vendor");
    let status = `Copper: ${this.#state.copper}`;
    if (!vendor) {
      status = `No vendor nearby. ${status}`;
    } else if (!vendor.inReach) {
      status = `Move within 5 m of ${vendor.name} to trade. ${status}`;
    } else if (!this.#bag.ready) {
      status = `Waiting for your bag… ${status}`;
    }
    setText(this.#elements.status, status);
    setText(this.#elements.feedback, this.#state.feedback);
    const canTrade = this.#state.canTrade(vendor, this.#bag);
    this.#offers.render((vendor?.offers ?? []).map((offer, index) => {
      const item = catalog?.items.get(offer.item);
      if (!item) {
        throw new Error("Vendor offer item is missing from the content catalog.");
      }
      const stats = statsText(item.stats);
      return {
        key: index,
        text: `${item.name} · ${offer.price} copper${stats ? ` (${stats})` : ""}`,
        action: "Buy",
        label: `Buy ${item.name} for ${offer.price} copper`,
        enabled: canTrade && offer.price <= this.#state.copper,
      };
    }));
    const slots = vendor ? this.#bag.slots ?? [] : [];
    this.#sales.render(slots.flatMap((stack, slot) => {
      if (!stack) {
        return [];
      }
      const item = catalog?.items.get(stack.itemId);
      if (!item) {
        throw new Error("Projected item is missing from the content catalog.");
      }
      const value = item.sellPrice * stack.quantity;
      return [{
        key: slot,
        text: `${item.name} × ${stack.quantity} · ${value} copper`,
        action: "Sell",
        label: `Sell ${item.name} × ${stack.quantity} for ${value} copper`,
        enabled: canTrade,
      }];
    }));
  }
}
