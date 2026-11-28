import type { WorldCommand } from "../../command-wire";
import { sameEntity, type EntityRef } from "../../entity-ref";
import type { LootView, ZoneSnapshot } from "../../replication";
import { panelRefusal } from "./hud-texts";

export type LootIntent = (latest: ZoneSnapshot) => WorldCommand | null;

type PendingClaim = { sheet: LootView; tick: bigint; sequence: number; sent: boolean };

function identity(snapshot: ZoneSnapshot): string {
  return `${snapshot.zoneId}:${snapshot.contentRevision}:${snapshot.viewerId}`;
}

function sameClaim(left: LootView | null, right: LootView | null): boolean {
  return left?.creatureId === right?.creatureId && left?.diedAt === right?.diedAt;
}

function eligibleSheet(snapshot: ZoneSnapshot): LootView | null {
  const sheet = snapshot.loot;
  if (snapshot.viewer.dead || sheet === null || snapshot.viewer.target?.kind !== "creature"
    || snapshot.viewer.target.id !== sheet.creatureId) {
    return null;
  }
  return sheet;
}

/** Received copper/loot only. Complete loot absence clears; intents change no rewards. */
export class LootState {
  #identity: string | null = null;
  #tick = -1n;
  #target: EntityRef | null = null;
  #sequence = 0;
  #sheet: LootView | null = null;
  #copper = 0;
  #feedback = "";
  #pending: PendingClaim | null = null;

  get sheet(): LootView | null { return copySheet(this.#sheet); }
  get copper(): number { return this.#copper; }
  get feedback(): string { return this.#feedback; }
  get canClaim(): boolean { return this.#sheet !== null && this.#pending === null; }

  reset(snapshot: ZoneSnapshot | null = null): void {
    this.#identity = snapshot ? identity(snapshot) : null;
    this.#tick = -1n;
    this.#target = null;
    this.#sequence = 0;
    this.#sheet = null;
    this.#copper = 0;
    this.#feedback = "";
    this.#pending = null;
  }

  update(snapshot: ZoneSnapshot): boolean {
    const joined = identity(snapshot);
    if ((this.#identity !== null && this.#identity !== joined) || snapshot.tick <= this.#tick) {
      return false;
    }
    const sheet = eligibleSheet(snapshot);
    const targetChanged = this.#tick >= 0n && !sameEntity(this.#target, snapshot.viewer.target);
    const pending = this.#pending;
    if (targetChanged || !sameClaim(this.#sheet, sheet)) {
      this.#feedback = "";
      this.#pending = null;
    } else if (pending?.sent && snapshot.tick > pending.tick
      && snapshot.acknowledgedSequence > pending.sequence) {
      this.#pending = null;
      this.#feedback = "Rewards are still available.";
    }
    this.#identity = joined;
    this.#tick = snapshot.tick;
    this.#target = snapshot.viewer.target ? { ...snapshot.viewer.target } : null;
    this.#sequence = snapshot.acknowledgedSequence;
    this.#copper = snapshot.viewer.copper;
    this.#sheet = copySheet(sheet);
    // Only feedback for this pending corpse action belongs in its window.
    if (pending?.sent && !targetChanged && (sheet === null || sameClaim(sheet, pending.sheet))) {
      let specificRefusal = false;
      for (const event of snapshot.events) {
        if (event.kind !== "error") {
          continue;
        }
        const related = event.target === null
          ? (event.code === "too-many-intents" && sameClaim(sheet, pending.sheet)) || (event.code === "you-are-dead" && snapshot.viewer.dead)
          : sameEntity(event.target, { kind: "creature", id: pending.sheet.creatureId });
        if (!related) {
          continue;
        }
        const message = panelRefusal(event.code, "loot");
        if (message !== null && (event.target !== null || !specificRefusal)) {
          this.#feedback = message;
          this.#pending = null;
          if (event.target !== null) {
            specificRefusal = true;
          }
        }
      }
    }
    return true;
  }

  /** One queued action, checked again against the freshest projection when dispatched. */
  claimIntent(): LootIntent | null {
    const sheet = this.#sheet;
    if (!this.canClaim || this.#tick < 0n || sheet === null) {
      return null;
    }
    const joined = this.#identity;
    const pending: PendingClaim = {
      sheet: copySheet(sheet)!,
      tick: this.#tick, sequence: this.#sequence, sent: false,
    };
    this.#pending = pending;
    this.#feedback = "Claim sent. Waiting for the zone.";
    return (current) => {
      if (pending.sent) {
        return null;
      }
      if (this.#pending !== pending || identity(current) !== joined
        || current.tick < this.#tick || current.tick < pending.tick || !sameClaim(eligibleSheet(current), pending.sheet)) {
        if (this.#pending === pending) {
          this.#pending = null;
          this.#feedback = "Loot changed before the claim was sent.";
        }
        return null;
      }
      pending.sent = true;
      pending.tick = current.tick;
      pending.sequence = current.acknowledgedSequence;
      return { kind: "loot", creatureId: pending.sheet.creatureId, diedAt: pending.sheet.diedAt };
    };
  }
}

function copySheet(sheet: LootView | null): LootView | null {
  if (sheet === null) {
    return null;
  }
  return {
    ...sheet,
    item: sheet.item ? { ...sheet.item } : null,
    questItem: sheet.questItem ? { ...sheet.questItem } : null,
  };
}

