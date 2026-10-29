import type { WorldCommand } from "../../command-wire";
import type { ZoneSnapshot } from "../../replication";

export type WaystoneElements = {
  panel: HTMLElement;
  title: HTMLElement;
  destinations: HTMLElement;
  status: HTMLElement;
};

/** A stone of the network: its 1-based ID and name, in ID order (`mmorpg_core::WAYSTONES`). */
export type WaystoneSite = { id: number; name: string };

/** What the panel offers in `snapshot`: the stone in reach and the unlocked destinations. */
export function waystoneOffer(
  snapshot: ZoneSnapshot,
  waystoneAt: (x: number, z: number) => number,
  sites: readonly WaystoneSite[],
): { here: WaystoneSite; destinations: WaystoneSite[]; undiscovered: number } | null {
  const viewer = snapshot.entities[0];
  if (snapshot.viewer.dead || !viewer) {
    return null;
  }
  const here = sites.find((site) => site.id === waystoneAt(viewer.position[0], viewer.position[2]));
  if (!here) {
    return null;
  }
  const unlocked = new Set(snapshot.viewer.waystones);
  return {
    here,
    destinations: sites.filter((site) => site.id !== here.id && unlocked.has(site.id)),
    undiscovered: sites.filter((site) => !unlocked.has(site.id)).length,
  };
}

/**
 * The waystone panel (#145): shown while the living viewer stands within reach of a stone,
 * one button per unlocked destination. The zone checks reach, unlocks and combat.
 */
export class WaystonePanel {
  readonly #elements: WaystoneElements;
  readonly #send: (command: WorldCommand) => void;
  #waystoneAt: (x: number, z: number) => number = () => 0;
  #sites: readonly WaystoneSite[] = [];
  #shown = "";

  constructor(elements: WaystoneElements, send: (command: WorldCommand) => void) {
    this.#elements = elements;
    this.#send = send;
  }

  load(waystoneAt: (x: number, z: number) => number, sites: readonly WaystoneSite[]): void {
    this.#waystoneAt = waystoneAt;
    this.#sites = sites;
  }

  update(snapshot: ZoneSnapshot): void {
    const offer = waystoneOffer(snapshot, this.#waystoneAt, this.#sites);
    const key = offer === null
      ? ""
      : `${offer.here.id}:${offer.destinations.map((site) => site.id).join(",")}:${snapshot.viewer.inCombat}`;
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
    title.textContent = `Waystone · ${offer.here.name}`;
    for (const site of offer.destinations) {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = site.name;
      button.disabled = snapshot.viewer.inCombat;
      button.addEventListener("click", () => this.#send({ kind: "use-waystone", waystone: site.id }));
      destinations.append(button);
    }
    status.textContent = offer.destinations.length === 0
      ? "Explore to discover more waystones."
      : offer.undiscovered > 0 ? `${offer.undiscovered} undiscovered` : "";
  }
}
