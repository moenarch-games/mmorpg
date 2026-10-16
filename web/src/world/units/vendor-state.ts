import type { WorldCommand } from "../../command-wire";
import type { ErrorCode, ZoneSnapshot } from "../../replication";
import type { ContentCatalog, VendorOffer } from "../catalog";
import type { BagState } from "./bag-state";

/** The zone's inclusive XZ reach to a vendor's feet (5 m); a presentation hint, the zone decides. */
export const VENDOR_REACH_UNITS = 500;

/** The nearest projected vendor NPC and whether it looks within reach. */
export type NearbyVendor = { npc: number; name: string; inReach: boolean; offers: readonly VendorOffer[] };

/** Honors explicit NPC selection; otherwise chooses the nearest vendor. The zone checks reach. */
export function nearestVendor(snapshot: ZoneSnapshot, catalog: ContentCatalog): NearbyVendor | null {
  const viewer = snapshot.entities[0];
  if (!viewer) {
    return null;
  }
  let nearest: { npc: number; distance: number } | null = null;
  const selected = snapshot.viewer.target;
  for (const entity of snapshot.entities) {
    if (entity.kind !== "npc" || !catalog.vendors.has(entity.entityId)) {
      continue;
    }
    if (selected?.kind === "npc" && selected.id !== entity.entityId) {
      continue;
    }
    const distance = Math.hypot(entity.position[0] - viewer.position[0], entity.position[2] - viewer.position[2]);
    if (nearest === null || distance < nearest.distance || (distance === nearest.distance && entity.entityId < nearest.npc)) {
      nearest = { npc: entity.entityId, distance };
    }
  }
  if (nearest === null) {
    return null;
  }
  const vendor = catalog.vendors.get(nearest.npc);
  return {
    npc: nearest.npc,
    name: catalog.npcs.get(nearest.npc)?.name ?? "Vendor",
    // Projected NPC feet stand on y = 0 while the viewer's record is its body centre, so compare XZ only.
    inReach: nearest.distance <= VENDOR_REACH_UNITS,
    offers: vendor?.offers ?? [],
  };
}

const REFUSALS: Partial<Record<ErrorCode, string>> = {
  "not-enough-money": "You do not have enough copper.",
  "inventory-full": "Your bags cannot hold that.",
  "invalid-vendor": "That vendor does not trade that.",
  "out-of-range": "Move closer to the vendor.",
  "money-overflow": "You cannot hold more copper.",
  "invalid-inventory-move": "That trade was refused. Your items are unchanged.",
};

/** The trade, its vendor, the bag revision when it was sent and the sequence the source assigned it. */
type PendingTrade = { kind: "buy" | "sell"; npc: number; revision: bigint; sequence: number | null };

/**
 * Trade intents over the shared received bag cache. Nothing is predicted: bag and copper change only
 * with the zone's next sheet, and one trade waits for its answer before the next is offered.
 */
export class VendorState {
  #tick = -1n;
  #copper = 0;
  #dead = false;
  #feedback = "";
  #pending: PendingTrade | null = null;

  get copper(): number { return this.#copper; }
  get feedback(): string { return this.#feedback; }
  get pending(): boolean { return this.#pending !== null; }

  reset(): void {
    this.#tick = -1n;
    this.#copper = 0;
    this.#dead = false;
    this.#feedback = "";
    this.#pending = null;
  }

  /** Takes a projection the bag cache already accepted (same identity, newer tick). */
  update(snapshot: ZoneSnapshot, bag: BagState): void {
    if (snapshot.tick <= this.#tick) {
      return;
    }
    this.#tick = snapshot.tick;
    this.#copper = snapshot.viewer.copper;
    this.#dead = snapshot.viewer.dead;
    const pending = this.#pending;
    if (pending === null) {
      return;
    }
    // A sheet past the sent revision confirms the trade, whatever uncorrelated feedback arrives with it.
    if (bag.ready && bag.revision > pending.revision) {
      this.#feedback = pending.kind === "buy" ? "Purchased." : "Sold.";
      this.#pending = null;
      return;
    }
    for (const event of snapshot.events) {
      if (event.kind !== "error") {
        continue;
      }
      const answers = event.target === null
        ? event.code === "too-many-intents" || event.code === "you-are-dead"
        : event.target.kind === "npc" && event.target.id === pending.npc;
      if (!answers) {
        continue;
      }
      this.#feedback = event.code === "too-many-intents"
        ? "Too many actions. Try the trade again."
        : event.code === "you-are-dead"
          ? "You cannot trade while dead."
          : REFUSALS[event.code] ?? "That trade was refused.";
      this.#pending = null;
      return;
    }
    if (pending.sequence !== null && snapshot.acknowledgedSequence >= pending.sequence &&
        snapshot.inventoryRevision === pending.revision) {
      // Events are lossy: a trade whose own sequence was acknowledged with the bag unchanged was refused.
      this.#feedback = "The vendor did not trade. Your items are unchanged.";
      this.#pending = null;
    }
  }

  /** Whether a trade with `vendor` may be offered now. */
  canTrade(vendor: NearbyVendor | null, bag: BagState): boolean {
    // Any pending bag intent could change the slots a trade names, so trades wait for its answer.
    return vendor !== null && vendor.inReach && !this.#dead && this.#pending === null && bag.canMove && !bag.pending;
  }

  buy(vendor: NearbyVendor | null, offer: number, bag: BagState): WorldCommand | null {
    const price = vendor?.offers[offer]?.price;
    if (!vendor || price === undefined || !this.canTrade(vendor, bag) || price > this.#copper) {
      return null;
    }
    this.#send("buy", vendor.npc, bag, "Buying…");
    return { kind: "buy-item", npc: vendor.npc, offer, quantity: 1 };
  }

  /** Sells the whole stack in `bagSlot`. */
  sell(vendor: NearbyVendor | null, bagSlot: number, bag: BagState): WorldCommand | null {
    const stack = bag.slots?.[bagSlot];
    if (!vendor || !stack || !this.canTrade(vendor, bag)) {
      return null;
    }
    this.#send("sell", vendor.npc, bag, "Selling…");
    return { kind: "sell-item", npc: vendor.npc, bagSlot, quantity: stack.quantity };
  }

  /** Records the sequence the pending trade was sent under; `null` means the source dropped it. */
  sent(sequence: number | null): void {
    const pending = this.#pending;
    if (pending === null) {
      return;
    }
    if (sequence === null) {
      this.#feedback = "That trade was not sent. Your items are unchanged.";
      this.#pending = null;
      return;
    }
    pending.sequence = sequence;
  }

  #send(kind: PendingTrade["kind"], npc: number, bag: BagState, feedback: string): void {
    this.#pending = { kind, npc, revision: bag.revision, sequence: null };
    this.#feedback = feedback;
  }
}
