import type { WorldCommand } from "../../command-wire";
import { questCompleted, type ErrorCode, type QuestMarkerKind, type QuestSheet, type ZoneSnapshot } from "../../replication";
import type { ContentCatalog, QuestObjectiveRecord, QuestRecord } from "../catalog";

/** The zone's inclusive XZ reach to a quest NPC's feet (5 m); a presentation hint, the zone decides. */
export const QUEST_REACH_UNITS = 500;

export type ObjectiveView = { text: string; current: number; required: number; done: boolean };
/** An active quest with each objective's received progress. */
export type QuestView = { quest: QuestRecord; objectives: readonly ObjectiveView[]; complete: boolean };
/** A projected quest giver or ender and whether it looks within reach. */
export type QuestNpc = { npc: number; name: string; inReach: boolean };
/** What one NPC offers the viewer: quests to accept, quests to turn in and quests still in progress. */
export type DialogView = {
  npc: QuestNpc;
  offered: readonly QuestRecord[];
  ready: readonly QuestView[];
  inProgress: readonly QuestView[];
};

/** "Timber Wolf slain", "Wolf Pelt", "Speak with Farmer Osric Mill" or "Explore Stillwater Lake". */
export function objectiveLabel(objective: QuestObjectiveRecord, catalog: ContentCatalog): string {
  switch (objective.kind) {
    case "kill":
      return `${catalog.creatureTemplates.get(objective.target)?.name ?? "Creature"} slain`;
    case "collect":
      return catalog.items.get(objective.target)?.name ?? "Item";
    case "talk":
      return `Speak with ${catalog.npcs.get(objective.target)?.name ?? "someone"}`;
    case "explore":
      return `Explore ${catalog.areas.get(objective.target) ?? "the area"}`;
    default: {
      const unknown: never = objective;
      throw new Error(`Unknown objective ${String(unknown)}`);
    }
  }
}

function objectiveView(objective: QuestObjectiveRecord, current: number, catalog: ContentCatalog): ObjectiveView {
  const required = objective.count;
  const label = objectiveLabel(objective, catalog);
  const done = current >= required;
  const text = objective.kind === "kill" || objective.kind === "collect"
    ? `${label}: ${Math.min(current, required)}/${required}`
    : `${label}${done ? " (done)" : ""}`;
  return { text, current, required, done };
}

/** The sheet's active quests in quest-ID order; quests the catalog does not know fail closed. */
export function questViews(sheet: QuestSheet, catalog: ContentCatalog): QuestView[] {
  return sheet.entries.map((entry) => {
    const quest = catalog.quests.get(entry.quest);
    if (!quest) {
      throw new Error("Projected quest is missing from the content catalog.");
    }
    const objectives = quest.objectives.map((objective, index) => objectiveView(objective, entry.progress[index] ?? 0, catalog));
    return { quest, objectives, complete: objectives.every((objective) => objective.done) };
  });
}

/** Whether the viewer may accept `quest`: not active, not turned in, and its prerequisite turned in. */
function offered(sheet: QuestSheet, quest: QuestRecord): boolean {
  return !sheet.entries.some((entry) => entry.quest === quest.id)
    && !questCompleted(sheet.completed, quest.id)
    && (quest.prerequisite === null || questCompleted(sheet.completed, quest.prerequisite));
}

function isQuestNpc(catalog: ContentCatalog, npc: number): boolean {
  for (const quest of catalog.quests.values()) {
    if (quest.giver === npc || quest.ender === npc) {
      return true;
    }
  }
  return false;
}

/**
 * An explicitly selected NPC is the only possible addressee, even if it has no quests.
 * Without an NPC selection, Talk retains nearest-quest-giver access. The zone checks reach.
 */
export function dialogNpc(snapshot: ZoneSnapshot, catalog: ContentCatalog): QuestNpc | null {
  const viewer = snapshot.entities[0];
  if (!viewer) {
    return null;
  }
  let chosen: { npc: number; distance: number } | null = null;
  const selected = snapshot.viewer.target;
  for (const entity of snapshot.entities) {
    if (entity.kind !== "npc" || !isQuestNpc(catalog, entity.entityId)) {
      continue;
    }
    if (selected?.kind === "npc" && selected.id !== entity.entityId) {
      continue;
    }
    const distance = Math.hypot(entity.position[0] - viewer.position[0], entity.position[2] - viewer.position[2]);
    const target = snapshot.viewer.target;
    if (target?.kind === "npc" && target.id === entity.entityId) {
      chosen = { npc: entity.entityId, distance };
      break;
    }
    if (chosen === null || distance < chosen.distance || (distance === chosen.distance && entity.entityId < chosen.npc)) {
      chosen = { npc: entity.entityId, distance };
    }
  }
  if (chosen === null) {
    return null;
  }
  return {
    npc: chosen.npc,
    name: catalog.npcs.get(chosen.npc)?.name ?? "Quest giver",
    inReach: chosen.distance <= QUEST_REACH_UNITS,
  };
}

export function dialogView(sheet: QuestSheet, catalog: ContentCatalog, npc: QuestNpc): DialogView {
  const active = questViews(sheet, catalog).filter((view) => view.quest.ender === npc.npc);
  return {
    npc,
    offered: [...catalog.quests.values()].filter((quest) => quest.giver === npc.npc && offered(sheet, quest)),
    ready: active.filter((view) => view.complete),
    inProgress: active.filter((view) => !view.complete),
  };
}

/** One line per active quest objective for the HUD tracker. */
export function trackerLines(sheet: QuestSheet, catalog: ContentCatalog): string[] {
  return questViews(sheet, catalog).flatMap((view) => [
    view.complete ? `${view.quest.name} — return to ${catalog.npcs.get(view.quest.ender)?.name ?? "the quest giver"}` : view.quest.name,
    ...view.objectives.map((objective) => `  ${objective.done ? "✓" : "·"} ${objective.text}`),
  ]);
}

/** Ticks (3 s at 30 Hz) after which an unanswered quest request stops blocking new ones. */
const PENDING_TICKS = 90n;

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
        const quest = catalog.quests.get(event.quest);
        const objective = quest?.objectives[event.objective];
        if (objective) {
          this.#feedback = objectiveView(objective, event.count, catalog).text;
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
