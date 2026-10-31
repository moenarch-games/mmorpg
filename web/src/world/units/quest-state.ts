import type { WorldCommand } from "../../command-wire";
import type { ObjectiveView, QuestView as ModelQuestView, TrackerRow } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { questCompleted, ticksFromMs, type ErrorCode, type QuestMarkerKind, type QuestSheet, type ZoneSnapshot } from "../../replication";
import { wasm } from "../../wasm";
import type { ContentCatalog, QuestRecord } from "../catalog";

// Quest views are the shared client model's (`mmorpg-client-model`, #543), the same the native
// client shows; this module names their quests from the catalog and keeps the request state.

export type { ObjectiveView, TrackerRow } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
/** An active quest with each objective's received progress. */
export type QuestView = { quest: QuestRecord; objectives: readonly ObjectiveView[]; complete: boolean };
/** A projected quest giver or ender and whether it looks within reach. */
export type QuestNpc = { npc: number; name: string; inReach: boolean };
/** What one NPC offers the viewer: quests to accept, quests to turn in and quests still in progress. */
export type DialogView = {
  npc: QuestNpc;
  /** A woken private-home resident's line before its turn-ins (#158), else null. */
  greeting: string | null;
  offered: readonly QuestRecord[];
  ready: readonly QuestView[];
  inProgress: readonly QuestView[];
};

/** The catalog record of a projected quest; a quest the catalog does not know fails closed. */
function record(catalog: ContentCatalog, quest: number): QuestRecord {
  const found = catalog.quests.get(quest);
  if (!found) {
    throw new Error("Projected quest is missing from the content catalog.");
  }
  return found;
}

function named(view: ModelQuestView, catalog: ContentCatalog): QuestView {
  return { quest: record(catalog, view.quest), objectives: view.objectives, complete: view.complete };
}

/** The sheet's active quests in quest-ID order. */
export function questViews(sheet: QuestSheet, catalog: ContentCatalog): QuestView[] {
  return wasm.questViews(sheet).map((view) => named(view, catalog));
}

/** `quest`'s objectives at `progress`, one count per objective (missing counts are 0). */
export function objectiveViews(quest: number, progress: readonly number[]): readonly ObjectiveView[] {
  return wasm.questView(quest, Uint8Array.from(progress))?.objectives ?? [];
}

/** One objective of `quest` at `count`, as a quest-progress event reports it. */
export function objectiveAt(quest: number, objective: number, count: number): ObjectiveView | undefined {
  const progress = [0, 0, 0];
  progress[objective] = count;
  return objectiveViews(quest, progress)[objective];
}

/**
 * The quest NPC Talk addresses: an explicitly selected NPC only, even without quests; otherwise
 * the nearest projected quest NPC. The zone checks reach.
 */
export function dialogNpc(snapshot: ZoneSnapshot, catalog: ContentCatalog): QuestNpc | null {
  const npc = wasm.dialogNpc(snapshot);
  return npc && { npc: npc.npc, name: catalog.npcs.get(npc.npc)?.name ?? "Quest giver", inReach: npc.inReach };
}

/**
 * At `night` a private-home resident (an NPC with a woken line, #158) greets with that line and
 * offers no new quests, only turn-ins; the zone enforces both.
 */
export function dialogView(sheet: QuestSheet, catalog: ContentCatalog, npc: QuestNpc, night = false): DialogView {
  const view = wasm.dialogView(sheet, npc.npc, night);
  return {
    npc,
    greeting: view.greeting,
    offered: view.offered.map((quest) => record(catalog, quest)),
    ready: view.ready.map((quest) => named(quest, catalog)),
    inProgress: view.inProgress.map((quest) => named(quest, catalog)),
  };
}

/** The HUD tracker's rows: each active quest followed by its objectives. */
export function trackerRows(sheet: QuestSheet): TrackerRow[] {
  return wasm.trackerRows(sheet);
}

/** Ticks (3 s) a confirmed objective change stays highlighted in the tracker. */
export const HIGHLIGHT_TICKS = BigInt(ticksFromMs(3_000));
/** Ticks (4 s) a completion notice and a newly ready turn-in marker stay shown. */
export const NOTICE_TICKS = BigInt(ticksFromMs(4_000));

/** Ticks (3 s) after which an unanswered quest request stops blocking new ones. */
const PENDING_TICKS = BigInt(ticksFromMs(3_000));

const REFUSALS: Partial<Record<ErrorCode, string>> = {
  "invalid-quest": "That quest is not available here.",
  "quest-log-full": "Your quest log is full. Abandon a quest first.",
  "quest-incomplete": "That quest is not complete yet.",
  "out-of-range": "Move closer to talk.",
  "inventory-full": "Your bags cannot hold the reward.",
  "money-overflow": "You cannot hold more copper.",
  "you-are-dead": "You cannot do that while dead.",
  "too-many-intents": "Too many actions. Try again.",
  "invalid-inventory-move": "The reward was refused. Your items are unchanged.",
};

/**
 * The viewer's received quest sheet and quest intents. Nothing is predicted: the log, markers and
 * progress change only with the zone's next sheet, and refusals answer the latest request.
 */
export class QuestState {
  #tick = -1n;
  #sheet: QuestSheet | null = null;
  #dead = false;
  #feedback = "";
  /**
   * The NPC the latest request addressed (`null` for an abandon) and the tick it was sent at,
   * until an answer arrives. Further requests wait, so repeated clicks cannot fill the zone's
   * intent queue; an unanswered request lapses after `PENDING_TICKS`.
   */
  #pending: { npc: number | null; tick: bigint } | null = null;
  /**
   * Cues for progress the zone confirmed between two received sheets, keyed to the tick they
   * expire. The first sheet after a reset only synchronizes state, so hydration, refetch and
   * reconnect never announce progress again.
   */
  #highlights = new Map<string, bigint>();
  #notice: { text: string; until: bigint } | null = null;
  #ready = new Map<number, bigint>();

  get sheet(): QuestSheet | null { return this.#sheet; }
  get feedback(): string { return this.#feedback; }
  /** Whether a request still waits for its answer. */
  get busy(): boolean { return this.#pending !== null; }

  reset(): void {
    this.#tick = -1n;
    this.#sheet = null;
    this.#dead = false;
    this.#feedback = "";
    this.#pending = null;
    this.#highlights.clear();
    this.#notice = null;
    this.#ready.clear();
  }

  /** Whether the zone just confirmed progress on `quest`'s `objective`. */
  highlighted(quest: number, objective: number): boolean {
    return (this.#highlights.get(`${quest}:${objective}`) ?? -1n) >= this.#tick;
  }

  /** The current non-blocking completion notice, or "" once it has expired. */
  get notice(): string {
    return this.#notice !== null && this.#notice.until >= this.#tick ? this.#notice.text : "";
  }

  /** Whether `npc` just became ready to take a completed quest back. */
  freshlyReady(npc: number): boolean {
    return (this.#ready.get(npc) ?? -1n) >= this.#tick && this.marker(npc) === "complete";
  }

  /** Records cues for objectives that advanced between two received sheets. */
  #confirm(before: QuestSheet, after: QuestSheet, catalog: ContentCatalog): void {
    const previous = new Map(questViews(before, catalog).map((view) => [view.quest.id, view]));
    for (const view of questViews(after, catalog)) {
      const old = previous.get(view.quest.id);
      if (!old) {
        continue;
      }
      view.objectives.forEach((objective, index) => {
        const was = old.objectives[index];
        if (was && objective.current > was.current) {
          this.#highlights.set(`${view.quest.id}:${index}`, this.#tick + HIGHLIGHT_TICKS);
          if (objective.done && !was.done && !view.complete) {
            this.#notice = { text: `Objective complete: ${objective.text.replace(/ \(done\)$/, "")}`, until: this.#tick + NOTICE_TICKS };
          }
        }
      });
      if (view.complete && !old.complete) {
        const ender = catalog.npcs.get(view.quest.ender)?.name ?? "the quest giver";
        this.#notice = { text: `${view.quest.name} ready — return to ${ender}`, until: this.#tick + NOTICE_TICKS };
        this.#ready.set(view.quest.ender, this.#tick + NOTICE_TICKS);
      }
    }
    for (const [key, until] of this.#highlights) {
      if (until < this.#tick) {
        this.#highlights.delete(key);
      }
    }
  }

  /** The viewer's marker over `npc`, if any. */
  marker(npc: number): QuestMarkerKind | null {
    return this.#sheet?.markers.find((marker) => marker.npc === npc)?.marker ?? null;
  }

  /** Takes a newer projection: its sheet, if any, and this tick's quest feedback. */
  update(snapshot: ZoneSnapshot, catalog: ContentCatalog): void {
    if (snapshot.tick <= this.#tick) {
      return;
    }
    this.#tick = snapshot.tick;
    this.#dead = snapshot.viewer.dead;
    if (this.#pending !== null && snapshot.tick - this.#pending.tick > PENDING_TICKS) {
      this.#pending = null;
    }
    if (snapshot.quests !== null) {
      const before = this.#sheet;
      const after = snapshot.quests;
      this.#sheet = after;
      if (before !== null) {
        this.#confirm(before, after, catalog);
        const name = (quest: number) => catalog.quests.get(quest)?.name ?? "Quest";
        const active = (sheet: QuestSheet, quest: number) => sheet.entries.some((entry) => entry.quest === quest);
        const accepted = after.entries.find((entry) => !active(before, entry.quest));
        const dropped = before.entries.find((entry) => !active(after, entry.quest) && !questCompleted(after.completed, entry.quest));
        // The quest-completed event is cosmetic and may be lost; the sheet's completion bit is not.
        const completed = before.entries.find(
          (entry) => !questCompleted(before.completed, entry.quest) && questCompleted(after.completed, entry.quest),
        );
        if (accepted) {
          this.#feedback = `Accepted: ${name(accepted.quest)}.`;
          this.#pending = null;
        } else if (dropped) {
          this.#feedback = `Abandoned: ${name(dropped.quest)}.`;
          this.#pending = null;
        } else if (completed) {
          this.#feedback = `${name(completed.quest)} completed.`;
          this.#pending = null;
        }
      }
    }
    for (const event of snapshot.events) {
      if (event.kind === "quest-completed") {
        this.#feedback = `${catalog.quests.get(event.quest)?.name ?? "Quest"} completed.`;
        this.#pending = null;
      } else if (event.kind === "quest-progress") {
        const objective = objectiveAt(event.quest, event.objective, event.count);
        if (objective) {
          this.#feedback = objective.text;
        }
      } else if (event.kind === "error" && this.#pending !== null) {
        const answers = event.target === null
          ? this.#pending.npc === null || event.code === "too-many-intents" || event.code === "you-are-dead"
          : event.target.kind === "npc" && event.target.id === this.#pending.npc;
        const text = REFUSALS[event.code];
        if (answers && text !== undefined) {
          this.#feedback = text;
          this.#pending = null;
        }
      }
    }
  }

  accept(npc: QuestNpc | null, quest: number): WorldCommand | null {
    if (!npc || !npc.inReach || this.#dead || this.busy) {
      return null;
    }
    this.#request(npc.npc, "Accepting…");
    return { kind: "accept-quest", npc: npc.npc, quest };
  }

  complete(npc: QuestNpc | null, quest: number, choice: number): WorldCommand | null {
    if (!npc || !npc.inReach || this.#dead || this.busy) {
      return null;
    }
    this.#request(npc.npc, "Turning in…");
    return { kind: "complete-quest", npc: npc.npc, quest, choice };
  }

  abandon(quest: number): WorldCommand | null {
    if (this.busy || !this.#sheet?.entries.some((entry) => entry.quest === quest)) {
      return null;
    }
    this.#request(null, "Abandoning…");
    return { kind: "abandon-quest", quest };
  }

  #request(npc: number | null, feedback: string): void {
    this.#pending = { npc, tick: this.#tick };
    this.#feedback = feedback;
  }
}
