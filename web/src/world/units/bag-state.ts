import type { WorldCommand } from "../../command-wire";
import type { InventorySlot, StatTotals, ZoneSnapshot } from "../../replication";
import { panelRefusal } from "./hud-texts";

/** The kind of item intent awaiting the zone's answer. */
type PendingItemIntent = "move" | "equip" | "unequip";

const EQUIPMENT_SLOT_COUNT = 6;

/** Retains received sheets; intents never modify this presentation cache. */
export class BagState {
  #identity: string | null = null;
  #tick = -1n;
  #revision = 0n;
  #sheetRevision = 0n;
  #slots: readonly InventorySlot[] | null = null;
  #equipment: readonly (number | null)[] | null = null;
  #stats: StatTotals | null = null;
  #dead = false;
  #feedback = "";
  #sentRevision: bigint | null = null;
  #pending: PendingItemIntent | null = null;
  /** The sequence the pending intent was sent under, once the source assigned it. */
  #sentSequence: number | null = null;
  /** Counts every change the panels draw from (#1060): a reset, an accepted sheet, a sent or answered intent. */
  #changes = 0;

  get slots(): readonly InventorySlot[] | null { return this.#slots; }
  /** Item IDs by equipment slot, received with the bag under the same revision. */
  get equipment(): readonly (number | null)[] | null { return this.#equipment; }
  get stats(): StatTotals | null { return this.#stats; }
  get ready(): boolean { return this.#slots !== null && this.#sheetRevision === this.#revision; }
  get canMove(): boolean { return this.ready && !this.#dead; }
  get feedback(): string { return this.#feedback; }
  get revision(): bigint { return this.#revision; }
  /** Moves on every change a panel may draw; equal means nothing a panel shows changed. */
  get changes(): number { return this.#changes; }
  /** Whether an item intent was sent and its sheet or refusal has not arrived yet. */
  get pending(): boolean { return this.#pending !== null; }
  /**
   * Equipment changes wait for the answer to the previous item intent: a second change built from the
   * same sheet would only be refused and misreport the first one's result.
   */
  get canChangeEquipment(): boolean { return this.canMove && this.#pending === null; }
  /** Bag moves may follow each other, but not an equipment change awaiting its answer. */
  get canMoveItems(): boolean { return this.canMove && (this.#pending === null || this.#pending === "move"); }
  get status(): string {
    if (!this.ready) {
      return "Waiting for your bag…";
    }
    if (this.#dead) {
      return "You cannot move items while dead.";
    }
    return "Select a stack, then its destination.";
  }

  reset(snapshot: ZoneSnapshot | null = null): void {
    this.#identity = snapshot ? `${snapshot.zoneId}:${snapshot.contentRevision}:${snapshot.viewerId}` : null;
    this.#tick = -1n;
    this.#revision = 0n;
    this.#sheetRevision = 0n;
    this.#slots = null;
    this.#equipment = null;
    this.#stats = null;
    this.#dead = false;
    this.#feedback = "";
    this.#sentRevision = null;
    this.#pending = null;
    this.#sentSequence = null;
    this.#changes += 1;
  }

  update(snapshot: ZoneSnapshot): boolean {
    const identity = `${snapshot.zoneId}:${snapshot.contentRevision}:${snapshot.viewerId}`;
    if ((this.#identity !== null && this.#identity !== identity) || snapshot.tick <= this.#tick ||
        snapshot.inventoryRevision < this.#revision) {
      return false;
    }
    this.#changes += 1;
    this.#identity = identity;
    this.#tick = snapshot.tick;
    this.#revision = snapshot.inventoryRevision;
    this.#dead = snapshot.viewer.dead;
    if (snapshot.inventory !== null) {
      this.#slots = snapshot.inventory.map((stack) => stack ? { ...stack } : null);
      this.#equipment = snapshot.equipment ? [...snapshot.equipment] : null;
      this.#stats = snapshot.stats ? { ...snapshot.stats } : null;
      this.#sheetRevision = snapshot.inventoryRevision;
      if (this.#sentRevision !== null && this.#sheetRevision > this.#sentRevision) {
        this.#feedback = this.#pending === "move" ? "Bag updated." : "Equipment updated.";
        this.#sentRevision = null;
        this.#pending = null;
      }
    }
    for (const event of snapshot.events) {
      // Bag and equipment refusals concern no unit; a targeted error answers another intent (a loot
      // claim, a trade), and nothing reinterprets errors once the pending intent was answered.
      if (event.kind !== "error" || event.target !== null || this.#pending === null) {
        continue;
      }
      const message = panelRefusal(event.code, this.#pending === "equip" || this.#pending === "unequip" ? this.#pending : "bag");
      if (message !== null) {
        this.#feedback = message;
        this.#sentRevision = null;
        this.#pending = null;
      }
    }
    // Events are lossy: once a projection acknowledges the intent's own sequence without changing
    // the bag, it was refused even if its feedback was lost, so the controls unlock.
    if (this.#pending !== null && this.#sentSequence !== null && snapshot.acknowledgedSequence >= this.#sentSequence &&
        snapshot.inventoryRevision === this.#sentRevision) {
      this.#feedback = "The zone did not change your items.";
      this.#sentRevision = null;
      this.#pending = null;
    }
    return true;
  }

  /**
   * Records the sequence the pending intent was sent under; `null` means the source dropped it, so
   * nothing will answer and the controls unlock at once.
   */
  sent(sequence: number | null): void {
    if (this.#pending === null) {
      return;
    }
    this.#changes += 1;
    if (sequence === null) {
      this.#feedback = "That item action was not sent. Your items are unchanged.";
      this.#sentRevision = null;
      this.#pending = null;
      return;
    }
    this.#sentSequence = sequence;
  }

  move(source: number, destination: number, quantity: number): WorldCommand | null {
    const stack = this.#slots?.[source];
    if (!this.canMoveItems || !Number.isInteger(source) || source < 0 || source >= 16 ||
        !Number.isInteger(destination) || destination < 0 || destination >= 16 ||
        !stack || !Number.isInteger(quantity) || quantity < 1 || quantity > stack.quantity) {
      return null;
    }
    this.#send("move", "Move sent. Waiting for the zone.");
    return { kind: "move-item", source, destination, quantity };
  }

  /**
   * Equips the item in an occupied bag slot. Whether the catalog item fits an equipment slot is
   * the caller's offer to make; the zone decides and refuses anything else.
   */
  equip(bagSlot: number): WorldCommand | null {
    if (!this.canChangeEquipment || !Number.isInteger(bagSlot) || bagSlot < 0 || bagSlot >= 16 || !this.#slots?.[bagSlot]) {
      return null;
    }
    this.#send("equip", "Equip sent. Waiting for the zone.");
    return { kind: "equip-item", bagSlot };
  }

  /** Moves an occupied equipment slot's item into the bag; the zone picks the bag slot. */
  unequip(equipmentSlot: number): WorldCommand | null {
    if (!this.canChangeEquipment || !Number.isInteger(equipmentSlot) || equipmentSlot < 0 || equipmentSlot >= EQUIPMENT_SLOT_COUNT ||
        this.#equipment?.[equipmentSlot] == null) {
      return null;
    }
    this.#send("unequip", "Unequip sent. Waiting for the zone.");
    return { kind: "unequip-item", equipmentSlot };
  }

  #send(kind: PendingItemIntent, feedback: string): void {
    this.#changes += 1;
    this.#feedback = feedback;
    this.#sentRevision = this.#revision;
    this.#pending = kind;
    this.#sentSequence = null;
  }
}
