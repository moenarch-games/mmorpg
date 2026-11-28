import type { DepthsLootRow, WorldCommand, ZoneSnapshot } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { panelViews } from "./panel-views";

export type DepthsLootElements = {
  panel: HTMLElement;
  list: HTMLElement;
};

/**
 * Captain Thorn's shared loot (#404): a Need/Greed/Pass prompt for each open roll and a claim
 * button for items everyone passed on. The rows are `mmorpg-client-model`'s
 * `panels::depths_loot_rows` (#1280), read with the projection's panel views; every button only
 * names a command the zone validates.
 */
export class DepthsLootPanel {
  readonly #elements: DepthsLootElements;

  #rendered = "";

  constructor(elements: DepthsLootElements, send: (command: WorldCommand) => void) {
    this.#elements = elements;
    elements.list.addEventListener("click", (event) => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-action]");
      if (!button) { return; }
      const run = BigInt(button.dataset.run ?? "0");
      const drop = Number(button.dataset.drop ?? 0);
      const action = button.dataset.action;
      if (action === "claim") {
        send({ kind: "claim-depths-drop", run, drop });
      } else if (action === "need" || action === "greed" || action === "pass") {
        send({ kind: "roll-depths-drop", run, drop, choice: action });
      }
    });
  }

  reset(): void {
    this.#rendered = "";
    this.#elements.panel.hidden = true;
    this.#elements.list.replaceChildren();
  }

  update(snapshot: ZoneSnapshot): void {
    const loot = panelViews(snapshot).depthsLoot;
    if (loot === null) {
      if (!this.#elements.panel.hidden) { this.reset(); }
      return;
    }
    // The rows carry the countdown, so an unchanged second leaves the list alone.
    const key = JSON.stringify(loot, (_key, value) => typeof value === "bigint" ? value.toString() : value);
    if (key === this.#rendered) { return; }
    this.#rendered = key;
    this.#elements.panel.hidden = false;
    this.#elements.list.replaceChildren(...loot.rows.map((row) => this.#row(loot.runId, row)));
  }

  #row(run: bigint, drop: DepthsLootRow): HTMLElement {
    const row = document.createElement("li");
    row.className = "depths-loot-row";
    row.append(text(drop.itemName), text(drop.line));
    for (const { action, label, enabled } of drop.buttons) {
      row.append(button(run, drop.drop, action, label, enabled));
    }
    return row;
  }
}

function text(content: string): HTMLElement {
  const span = document.createElement("span");
  span.textContent = content;
  return span;
}

function button(run: bigint, drop: number, action: string, label: string, enabled: boolean): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button";
  element.textContent = label;
  element.dataset.run = String(run);
  element.dataset.drop = String(drop);
  element.dataset.action = action;
  element.disabled = !enabled;
  return element;
}
