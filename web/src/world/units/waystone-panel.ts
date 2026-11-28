import type { WorldCommand } from "../../command-wire";
import type { ZoneSnapshot } from "../../replication";
import { panelViews } from "./panel-views";

export type WaystoneElements = {
  panel: HTMLElement;
  title: HTMLElement;
  destinations: HTMLElement;
  status: HTMLElement;
};

/**
 * The waystone panel (#145): shown while the living viewer stands within reach of a stone,
 * one button per unlocked destination. What it offers is `mmorpg-client-model`'s
 * `panels::waystone_offer` (#1280), read with the projection's panel views. The zone checks
 * reach, unlocks and combat.
 */
export class WaystonePanel {
  readonly #elements: WaystoneElements;
  readonly #send: (command: WorldCommand) => void;
  #shown = "";

  constructor(elements: WaystoneElements, send: (command: WorldCommand) => void) {
    this.#elements = elements;
    this.#send = send;
  }

  update(snapshot: ZoneSnapshot): void {
    const offer = panelViews(snapshot).waystone;
    const key = offer === null
      ? ""
      : `${offer.id}:${offer.destinations.map((site) => site.id).join(",")}:${offer.status}:${snapshot.viewer.inCombat}`;
    if (key === this.#shown) {
      return;
    }
    this.#shown = key;
    const { panel, title, destinations, status } = this.#elements;
    panel.hidden = offer === null;
    destinations.replaceChildren();
    if (offer === null) {
      return;
    }
    title.textContent = `Waystone · ${offer.name}`;
    for (const site of offer.destinations) {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = site.name;
      button.disabled = snapshot.viewer.inCombat;
      button.addEventListener("click", () => this.#send({ kind: "use-waystone", waystone: site.id }));
      destinations.append(button);
    }
    status.textContent = offer.status;
  }
}
