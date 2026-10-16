import type { WorldCommand } from "../../command-wire";
import type { QuestMarkerKind, ZoneSnapshot } from "../../replication";
import type { ContentCatalog, QuestRecord } from "../catalog";
import { dialogNpc, dialogView, questViews, QuestState, trackerLines, type QuestNpc, type QuestView } from "./quest-state";

export type QuestElements = {
  /** The NPC dialog: quests offered, ready to turn in and in progress at one quest NPC. */
  dialog: HTMLElement;
  dialogToggle: HTMLButtonElement;
  dialogClose: HTMLButtonElement;
  dialogTitle: HTMLElement;
  dialogStatus: HTMLElement;
  dialogQuests: HTMLElement;
  dialogFeedback: HTMLElement;
  /** The quest log: every active quest with its objectives and Abandon. */
  log: HTMLElement;
  logToggle: HTMLButtonElement;
  logClose: HTMLButtonElement;
  logQuests: HTMLElement;
  logFeedback: HTMLElement;
  /** The always-visible objective tracker. */
  tracker: HTMLElement;
  /** Absolutely positioned `!` and `?` markers over quest NPCs. */
  markers: HTMLElement;
};

/** A quest NPC's marker anchor on screen, in CSS pixels, or not on screen. */
export type MarkerPoint = { npc: number; x: number; y: number; onScreen: boolean };

const MARKER_TEXT: Record<QuestMarkerKind, string> = { available: "!", complete: "?", "in-progress": "?" };
const MARKER_LABEL: Record<QuestMarkerKind, string> = {
  available: "Quest available",
  complete: "Quest ready to turn in",
  "in-progress": "Quest in progress",
};

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string, className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (text !== undefined) {
    node.textContent = text;
  }
  if (className) {
    node.className = className;
  }
  return node;
}

function setText(node: HTMLElement, text: string): void {
  if (node.textContent !== text) {
    node.textContent = text;
  }
}

function rewardsText(quest: QuestRecord): string {
  const parts = [`${quest.experience} XP`];
  if (quest.copper > 0) {
    parts.push(`${quest.copper} copper`);
  }
  return `Rewards: ${parts.join(" · ")}`;
}

function objectiveList(view: QuestView | null, quest: QuestRecord, catalog: ContentCatalog): HTMLUListElement {
  const list = element("ul", undefined, "quest-objectives");
  const objectives = view?.objectives ?? questViews({ completed: 0, entries: [{ quest: quest.id, progress: [0, 0, 0] }], markers: [] }, catalog)[0]!.objectives;
  for (const objective of objectives) {
    const item = element("li", objective.text);
    item.dataset.done = String(objective.done);
    list.append(item);
  }
  return list;
}

/**
 * The NPC dialog, quest log, objective tracker and NPC markers over the received quest sheet. The
 * zone accepts, completes and abandons; nothing here changes quest state.
 */
export class QuestPanel {
  readonly #state = new QuestState();
  readonly #elements: QuestElements;
  readonly #send: (command: WorldCommand) => void;
  readonly #onOpen: () => void;
  readonly #choices = new Map<number, number>();
  readonly #markerNodes = new Map<number, HTMLElement>();
  #catalog: ContentCatalog | null = null;
  #npc: QuestNpc | null = null;
  #dialogKey = "";
  /** What had focus when the dialog opened, to return to on close if still visible. */
  #dialogReturn: HTMLElement | null = null;
  #logKey = "";

  constructor(elements: QuestElements, send: (command: WorldCommand) => void, onOpen: () => void) {
    this.#elements = elements;
    this.#send = send;
    this.#onOpen = onOpen;
    elements.dialogToggle.addEventListener("click", () => this.toggleDialog());
    elements.dialogClose.addEventListener("click", () => this.closeDialog());
    elements.logToggle.addEventListener("click", () => this.toggleLog());
    elements.logClose.addEventListener("click", () => this.closeLog());
    for (const [panel, close] of [[elements.dialog, () => this.closeDialog()], [elements.log, () => this.closeLog()]] as const) {
      panel.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          close();
        }
      });
    }
    this.reset();
  }

  get state(): QuestState { return this.#state; }
  get dialogOpen(): boolean { return !this.#elements.dialog.hidden; }
  get logOpen(): boolean { return !this.#elements.log.hidden; }

  load(catalog: ContentCatalog): void { this.#catalog = catalog; }

  reset(snapshot: ZoneSnapshot | null = null): void {
    this.#state.reset();
    this.#npc = null;
    this.#choices.clear();
    this.#dialogKey = "";
    this.#logKey = "";
    for (const [panel, toggle] of [[this.#elements.dialog, this.#elements.dialogToggle], [this.#elements.log, this.#elements.logToggle]] as const) {
      panel.hidden = true;
      toggle.setAttribute("aria-expanded", "false");
    }
    this.#elements.dialogQuests.replaceChildren();
    this.#elements.logQuests.replaceChildren();
    this.#elements.markers.replaceChildren();
    this.#markerNodes.clear();
    if (snapshot) {
      this.update(snapshot);
    }
    this.#render();
  }

  toggleDialog(): void {
    if (this.dialogOpen) {
      this.closeDialog();
      return;
    }
    const active = document.activeElement;
    this.#dialogReturn = active instanceof HTMLElement && active !== document.body ? active : null;
    this.#onOpen();
    // The dialog takes the log's place; its Talk button opens it.
    this.closeLog(false);
    this.#elements.dialog.hidden = false;
    this.#elements.dialogToggle.setAttribute("aria-expanded", "true");
    this.#dialogKey = "";
    this.#render();
    this.#elements.dialogClose.focus();
  }

  closeDialog(returnFocus = true): boolean {
    const restore = this.#dialogReturn;
    this.#dialogReturn = null;
    // The Talk button lives in the log, which the dialog hid; return focus to the control that
    // opened the dialog when it is still shown, and otherwise release it to the world.
    const closed = this.#close(this.#elements.dialog, this.#elements.dialogToggle, false);
    if (closed && returnFocus) {
      if (restore?.isConnected && restore.getClientRects().length > 0) {
        restore.focus();
      } else if (this.#elements.dialog.contains(document.activeElement)) {
        (document.activeElement as HTMLElement).blur();
      }
    }
    return closed;
  }

  toggleLog(): void {
    if (this.logOpen) {
      this.closeLog();
      return;
    }
    this.#onOpen();
    // The log takes the dialog's place, as the dialog takes the log's.
    this.closeDialog(false);
    this.#elements.log.hidden = false;
    this.#elements.logToggle.setAttribute("aria-expanded", "true");
    this.#logKey = "";
    this.#render();
    this.#elements.logClose.focus();
  }

  closeLog(returnFocus = true): boolean {
    return this.#close(this.#elements.log, this.#elements.logToggle, returnFocus);
  }

  #close(panel: HTMLElement, toggle: HTMLButtonElement, returnFocus: boolean): boolean {
    if (panel.hidden) {
      return false;
    }
    panel.hidden = true;
    toggle.setAttribute("aria-expanded", "false");
    if (returnFocus) {
      toggle.focus();
    }
    return true;
  }

  /** Takes a newer projection: sheet, feedback and the NPC the dialog addresses. */
  update(snapshot: ZoneSnapshot): void {
    const catalog = this.#catalog;
    if (!catalog) {
      return;
    }
    this.#state.update(snapshot, catalog);
    this.#npc = dialogNpc(snapshot, catalog);
    this.#render();
  }

  /** Places one marker per marked quest NPC on screen; NPCs off screen or out of view hide theirs. */
  placeMarkers(points: readonly MarkerPoint[]): void {
    const seen = new Set<number>();
    for (const point of points) {
      const marker = this.#state.marker(point.npc);
      if (marker === null || !point.onScreen) {
        continue;
      }
      seen.add(point.npc);
      let node = this.#markerNodes.get(point.npc);
      if (!node) {
        node = element("span", undefined, "quest-marker");
        node.setAttribute("role", "img");
        this.#markerNodes.set(point.npc, node);
        this.#elements.markers.append(node);
      }
      setText(node, MARKER_TEXT[marker]);
      node.dataset.marker = marker;
      node.dataset.npc = String(point.npc);
      const name = this.#catalog?.npcs.get(point.npc)?.name ?? "NPC";
      node.setAttribute("aria-label", `${MARKER_LABEL[marker]}: ${name}`);
      node.style.transform = `translate(${point.x.toFixed(1)}px, ${point.y.toFixed(1)}px) translate(-50%, -100%)`;
    }
    for (const [npc, node] of this.#markerNodes) {
      if (!seen.has(npc)) {
        node.remove();
        this.#markerNodes.delete(npc);
      }
    }
  }

  #render(): void {
    const catalog = this.#catalog;
    const sheet = this.#state.sheet;
    const lines = catalog && sheet ? trackerLines(sheet, catalog) : [];
    setText(this.#elements.tracker, lines.length > 0 ? lines.join("\n") : "No active quests. Look for ! over quest givers and press T to talk.");
    setText(this.#elements.dialogFeedback, this.#state.feedback);
    setText(this.#elements.logFeedback, this.#state.feedback);
    if (!catalog || !sheet) {
      setText(this.#elements.dialogTitle, "Quests");
      setText(this.#elements.dialogStatus, "Waiting for your quest log…");
      return;
    }
    if (this.dialogOpen) {
      this.#renderDialog(catalog, sheet);
    }
    if (this.logOpen) {
      this.#renderLog(catalog, sheet);
    }
  }

  #renderDialog(catalog: ContentCatalog, sheet: NonNullable<QuestState["sheet"]>): void {
    const npc = this.#npc;
    setText(this.#elements.dialogTitle, npc?.name ?? "Quests");
    if (!npc) {
      setText(this.#elements.dialogStatus, "No quest giver nearby.");
      if (this.#dialogKey !== "none") {
        this.#dialogKey = "none";
        this.#elements.dialogQuests.replaceChildren();
      }
      return;
    }
    const view = dialogView(sheet, catalog, npc);
    const empty = view.offered.length + view.ready.length + view.inProgress.length === 0;
    setText(this.#elements.dialogStatus, !npc.inReach
      ? `Move within 5 m of ${npc.name} to talk.`
      : empty ? `${npc.name} has nothing for you right now.` : "");
    const key = JSON.stringify([npc, this.#state.busy, view.offered.map((quest) => quest.id),
      view.ready.map((entry) => entry.quest.id), view.inProgress.map((entry) => [entry.quest.id, entry.objectives.map((o) => o.current)])]);
    if (key === this.#dialogKey) {
      return;
    }
    this.#dialogKey = key;
    const cards: HTMLElement[] = [];
    for (const quest of view.offered) {
      cards.push(this.#card(quest, null, catalog, "Accept", () => this.#state.accept(this.#npc, quest.id),
        npc.inReach && !this.#state.busy));
    }
    for (const entry of view.ready) {
      const card = this.#card(entry.quest, entry, catalog, "Complete quest", () =>
        this.#state.complete(this.#npc, entry.quest.id, this.#choices.get(entry.quest.id) ?? 0),
        npc.inReach && !this.#state.busy);
      card.dataset.state = "complete";
      cards.push(card);
    }
    for (const entry of view.inProgress) {
      const card = this.#card(entry.quest, entry, catalog, null, null, false);
      card.dataset.state = "in-progress";
      cards.push(card);
    }
    this.#elements.dialogQuests.replaceChildren(...cards);
  }

  #renderLog(catalog: ContentCatalog, sheet: NonNullable<QuestState["sheet"]>): void {
    const views = questViews(sheet, catalog);
    const key = JSON.stringify([this.#state.busy, views.map((view) => [view.quest.id, view.objectives.map((objective) => objective.current)])]);
    if (key === this.#logKey) {
      return;
    }
    this.#logKey = key;
    if (views.length === 0) {
      this.#elements.logQuests.replaceChildren(element("li", "Your quest log is empty.", "quest-empty"));
      return;
    }
    this.#elements.logQuests.replaceChildren(...views.map((view) =>
      this.#card(view.quest, view, catalog, "Abandon", () => this.#state.abandon(view.quest.id), !this.#state.busy)));
  }

  /** One quest: name, text, objectives, rewards with any item choice, and an optional action. */
  #card(
    quest: QuestRecord,
    view: QuestView | null,
    catalog: ContentCatalog,
    action: string | null,
    command: (() => WorldCommand | null) | null,
    enabled: boolean,
  ): HTMLElement {
    const card = element("li", undefined, "quest-card");
    card.dataset.quest = String(quest.id);
    const heading = element("h3", view?.complete ? `${quest.name} (complete)` : quest.name);
    card.append(heading, element("p", quest.text, "quest-text"), objectiveList(view, quest, catalog));
    card.append(element("p", rewardsText(quest), "quest-rewards"));
    if (quest.choices.length > 0) {
      const group = element("fieldset", undefined, "quest-choices");
      group.append(element("legend", "Choose one reward"));
      const chosen = this.#choices.get(quest.id) ?? 0;
      quest.choices.forEach((itemId, index) => {
        const label = element("label");
        const input = element("input");
        input.type = "radio";
        input.name = `quest-${quest.id}-choice`;
        input.value = String(index);
        input.checked = index === chosen;
        input.disabled = view === null || !view.complete;
        input.addEventListener("change", () => this.#choices.set(quest.id, index));
        label.append(input, ` ${catalog.items.get(itemId)?.name ?? "Item"}`);
        group.append(label);
      });
      card.append(group);
    }
    if (action && command) {
      const button = element("button", action);
      button.type = "button";
      button.disabled = !enabled;
      button.setAttribute("aria-label", `${action}: ${quest.name}`);
      button.addEventListener("click", () => {
        const next = command();
        if (next) {
          this.#send(next);
        }
        setText(this.#elements.dialogFeedback, this.#state.feedback);
        setText(this.#elements.logFeedback, this.#state.feedback);
      });
      card.append(button);
    }
    return card;
  }
}
