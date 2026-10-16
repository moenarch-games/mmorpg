import { sameEntity, type EntityRef } from "../../entity-ref";
import type { Vector3, ZoneSnapshot } from "../../replication";
import type { ContentCatalog } from "../catalog";
import { LootState, type LootIntent } from "./loot-state";

export type LootElements = {
  panel: HTMLElement;
  toggle: HTMLButtonElement;
  close: HTMLButtonElement;
  title: HTMLElement;
  rewards: HTMLElement;
  claim: HTMLButtonElement;
  feedback: HTMLElement;
  copper: HTMLElement;
};

/** Chooses from received ownership flags; core decides selection and claim eligibility. */
export function lootTarget(snapshot: ZoneSnapshot): EntityRef | null {
  if (snapshot.viewer.dead) {
    return null;
  }
  const viewer = snapshot.entities[0];
  if (!viewer) {
    return null;
  }
  const corpses = snapshot.entities.filter((entity) => entity.kind === "creature" && entity.flags.lootable);
  const selected = corpses.find((entity) => sameEntity(snapshot.viewer.target, { kind: entity.kind, id: entity.entityId }));
  if (selected) {
    return { kind: "creature", id: selected.entityId };
  }
  corpses.sort((left, right) => {
    const distance = (position: Vector3) => {
      const x = position[0] - viewer.position[0];
      const y = position[1] - viewer.position[1];
      const z = position[2] - viewer.position[2];
      return x * x + y * y + z * z;
    };
    return distance(left.position) - distance(right.position) || left.entityId - right.entityId;
  });
  const nearest = corpses[0];
  return nearest ? { kind: "creature", id: nearest.entityId } : null;
}

/** Accessible received rewards and claim intent. No local settlement or reward generation. */
export class LootPanel {
  readonly #state = new LootState();
  readonly #elements: LootElements;
  readonly #send: (intent: LootIntent) => void;
  readonly #onOpen: () => void;
  #catalog: ContentCatalog | null = null;
  #name = "Corpse loot";
  #empty = "Select an owned corpse and move within reach.";

  constructor(elements: LootElements, send: (intent: LootIntent) => void, onOpen: () => void) {
    this.#elements = elements;
    this.#send = send;
    this.#onOpen = onOpen;
    elements.toggle.addEventListener("click", () => this.toggle());
    elements.close.addEventListener("click", () => this.close());
    elements.claim.addEventListener("click", () => {
      const intent = this.#state.claimIntent();
      if (intent !== null) {
        this.#send(intent);
      }
      this.#render();
    });
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
    this.#state.reset(snapshot);
    this.#name = "Corpse loot";
    this.#empty = "Select an owned corpse and move within reach.";
    this.#elements.panel.hidden = true;
    this.#elements.toggle.setAttribute("aria-expanded", "false");
    if (snapshot !== null) {
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
    this.#send((latest) => {
      const target = lootTarget(latest);
      return target ? { kind: "select-target", target } : null;
    });
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

  update(snapshot: ZoneSnapshot): void {
    if (!this.#state.update(snapshot)) {
      return;
    }
    const corpse = snapshot.entities.find((entity) => entity.kind === "creature"
      && sameEntity(snapshot.viewer.target, { kind: "creature", id: entity.entityId }) && entity.flags.dead);
    this.#name = corpse ? this.#catalog?.creatureTemplates.get(corpse.appearance)?.name ?? "Corpse loot" : "Corpse loot";
    this.#empty = "Select an owned corpse and move within reach.";
    if (snapshot.viewer.dead) {
      this.#empty = "You cannot loot while dead.";
    } else if (corpse?.flags.lootable) {
      this.#empty = "Move closer to this corpse.";
    } else if (corpse) {
      this.#empty = "No rewards remain on this corpse.";
    }
    this.#render();
  }

  #render(): void {
    const { title, rewards, claim, feedback, copper } = this.#elements;
    const sheet = this.#state.sheet;
    let rewardText = this.#empty;
    if (sheet !== null) {
      rewardText = `${sheet.money} copper`;
      for (const [stack, suffix] of [[sheet.item, ""], [sheet.questItem, " (quest)"]] as const) {
        if (stack === null) {
          continue;
        }
        const item = this.#catalog?.items.get(stack.itemId);
        if (!item) {
          throw new Error("Projected loot item is missing from the content catalog.");
        }
        rewardText += ` · ${item.name} × ${stack.quantity}${suffix}`;
      }
    }
    for (const [element, text] of [[title, this.#name], [rewards, rewardText],
      [feedback, this.#state.feedback], [copper, `Copper: ${this.#state.copper}`]] as const) {
      if (element.textContent !== text) {
        element.textContent = text;
      }
    }
    claim.disabled = !this.#state.canClaim;
  }
}
