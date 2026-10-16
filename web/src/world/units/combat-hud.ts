import type { EntityRef } from "../../entity-ref";
import { findEntity, type ErrorCode, type ZoneEvent, type ZoneSnapshot } from "../../replication";
import type { ContentCatalog } from "../catalog";
import { objectiveLabel } from "./quest-state";

/**
 * Plain-text combat HUD lines beside the class HUD's frames, cast bars and
 * combat text (`class-hud.ts`); nameplates arrive with step 7b.
 */
const ERROR_TEXT: Record<ErrorCode, string> = {
  "no-target": "You have no target.",
  "out-of-range": "Out of range.",
  "target-dead": "Your target is dead.",
  "not-attackable": "You cannot attack that.",
  "you-are-dead": "You are dead.",
  "not-dead": "You are not dead.",
  "invalid-target": "That is not a valid target for that ability.",
  "too-many-intents": "Too many actions at once.",
  "invalid-inventory-move": "Bag move refused.",
  "inventory-full": "Your bags cannot hold those items.",
  "invalid-loot": "That corpse is no longer available.",
  "not-loot-owner": "That loot belongs to another player.",
  "empty-loot": "No loot remains.",
  "money-overflow": "You cannot hold any more copper.",
  "no-class": "You have no class yet.",
  "not-learned": "You have not learned that ability yet.",
  "not-ready": "That ability is not ready yet.",
  "not-enough-resource": "Not enough resource.",
  "stunned": "You are stunned and cannot act.",
  "already-casting": "You are already casting. Press Esc to cancel.",
  "invalid-class": "That class choice is not available.",
  "not-equippable": "That item cannot be equipped.",
  "invalid-vendor": "That vendor does not sell that.",
  "not-enough-money": "You do not have enough copper.",
  "chat-throttled": "You can speak or emote once per second.",
  "invalid-quest": "That quest is not available here.",
  "quest-log-full": "Your quest log is full.",
  "quest-incomplete": "That quest is not complete yet.",
};

const RESOURCE_NAMES = { rage: "Rage", focus: "Focus", mana: "Mana" } as const;

/** An ability's catalog name. */
export function abilityName(ability: number, catalog: ContentCatalog): string {
  return catalog.abilities.get(ability)?.name ?? `ability ${ability}`;
}

/** A unit's display name: catalog names for creatures and NPCs, "you" for the viewer. */
export function unitName(entity: EntityRef, snapshot: ZoneSnapshot, catalog: ContentCatalog): string {
  switch (entity.kind) {
    case "player":
      return entity.id === snapshot.viewerId ? "you" : `Player ${entity.id}`;
    case "npc":
      return catalog.npcs.get(entity.id)?.name ?? `NPC ${entity.id}`;
    case "creature": {
      const template = findEntity(snapshot, entity)?.appearance;
      return (template === undefined ? undefined : catalog.creatureTemplates.get(template)?.name) ?? "a creature";
    }
  }
}

/** Health, level, combat state and target, or how to release a dead spirit. */
export function combatStatus(snapshot: ZoneSnapshot, catalog: ContentCatalog): string {
  const viewer = snapshot.viewer;
  if (viewer.dead) {
    return "You are dead. Press R to release your spirit to the graveyard.";
  }
  const parts = [`Health ${viewer.health}/${viewer.maxHealth}`];
  if (viewer.resource) {
    parts.push(`${RESOURCE_NAMES[viewer.resource.kind]} ${viewer.resource.value}/${viewer.resource.max}`);
  }
  parts.push(`Level ${viewer.level}`);
  if (viewer.cast) {
    const verb = viewer.cast.channel ? "Channelling" : "Casting";
    parts.push(`${verb} ${abilityName(viewer.cast.ability, catalog)} ${viewer.cast.elapsed}/${viewer.cast.total}`);
  }
  if (viewer.inCombat) {
    parts.push("In combat");
  }
  if (viewer.target !== null) {
    const record = findEntity(snapshot, viewer.target);
    const name = unitName(viewer.target, snapshot, catalog);
    const detail = record === undefined
      ? "out of sight"
      : record.flags.dead ? `level ${record.level}, dead` : `level ${record.level}, ${record.healthPercent}%`;
    const cast = snapshot.targetDetail.cast;
    const casting = cast ? `, casting ${abilityName(cast.ability, catalog)} ${cast.elapsed}/${cast.total}` : "";
    parts.push(`Target: ${name} (${detail}${casting})`);
  }
  if (viewer.autoAttacking) {
    parts.push("Attacking");
  }
  return parts.join(" · ");
}

/** The readable message for an error code; a resource shortage names the viewer's resource. */
export function errorText(code: ErrorCode, snapshot: ZoneSnapshot): string {
  const resource = snapshot.viewer.resource;
  return code === "not-enough-resource" && resource ? `Not enough ${RESOURCE_NAMES[resource.kind].toLowerCase()}.` : ERROR_TEXT[code];
}

/** One feedback line per event, from the viewer's point of view. */
export function eventText(event: ZoneEvent, snapshot: ZoneSnapshot, catalog: ContentCatalog): string {
  const name = (entity: EntityRef) => unitName(entity, snapshot, catalog);
  const isViewer = (entity: EntityRef) => entity.kind === "player" && entity.id === snapshot.viewerId;
  const critical = (isCritical: boolean) => (isCritical ? " (critical)" : "");
  switch (event.kind) {
    case "damage-dealt":
      return `You hit ${name(event.target)} for ${event.amount}${critical(event.critical)}.`;
    case "damage-taken":
      return `${capitalise(name(event.source))} hits you for ${event.amount}${critical(event.critical)}.`;
    case "miss":
      return event.source.kind === "player" && event.source.id === snapshot.viewerId
        ? `You miss ${name(event.target)}.`
        : `${capitalise(name(event.source))} misses you.`;
    case "evade":
      return `${capitalise(name(event.target))} evades.`;
    case "died":
      return event.entity.kind === "player" && event.entity.id === snapshot.viewerId
        ? "You die."
        : `${capitalise(name(event.entity))} dies.`;
    case "error":
      return errorText(event.code, snapshot);
    case "cast-started":
      return `${capitalise(name(event.source))} ${isViewer(event.source) ? "begin" : "begins"} ${abilityName(event.ability, catalog)}.`;
    case "ability-used":
      return `${capitalise(name(event.source))} ${isViewer(event.source) ? "use" : "uses"} ${abilityName(event.ability, catalog)}.`;
    case "healed":
      return `${abilityName(event.ability, catalog)} heals ${name(event.target)} for ${event.amount}.`;
    case "aura-applied":
      return `${capitalise(name(event.target))} ${isViewer(event.target) ? "gain" : "gains"} ${abilityName(event.ability, catalog)}.`;
    case "aura-removed":
      return `${abilityName(event.ability, catalog)} fades from ${name(event.target)}.`;
    case "interrupted":
      return `${capitalise(name(event.target))} ${isViewer(event.target) ? "are" : "is"} interrupted (${abilityName(event.ability, catalog)}).`;
    case "absorbed":
      return `A shield absorbs ${event.amount} damage.`;
    case "quest-progress": {
      const quest = catalog.quests.get(event.quest);
      const objective = quest?.objectives[event.objective];
      return objective
        ? `${quest!.name}: ${objectiveLabel(objective, catalog)} ${objective.kind === "kill" || objective.kind === "collect" ? `${event.count}/${objective.count}` : "done"}.`
        : "Quest progress.";
    }
    case "quest-completed":
      return `${catalog.quests.get(event.quest)?.name ?? "Quest"} completed.`;
  }
}

function capitalise(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** How long a feedback line stays up, in milliseconds. */
export const FEEDBACK_MS = 3_000;

/** A HUD line the combat HUD writes; an `HTMLElement` in the page. */
export type HudLine = { textContent: string | null };

/**
 * The two combat HUD lines: the viewer's status, and the latest feedback
 * event, which fades after `FEEDBACK_MS`. Lines are written only when their
 * text changes.
 */
export class CombatHud {
  readonly #status: HudLine;
  readonly #feedback: HudLine;
  #latest: { text: string; until: number } | null = null;
  #tick = -1n;

  constructor(status: HudLine, feedback: HudLine) {
    this.#status = status;
    this.#feedback = feedback;
  }

  /** Clears both lines for a new session. */
  reset(): void {
    this.#latest = null;
    this.#tick = -1n;
    this.#status.textContent = "";
    this.#feedback.textContent = "";
  }

  /** Reads one projection; its events are read once, however many frames show it. */
  update(snapshot: ZoneSnapshot, catalog: ContentCatalog, now: number): void {
    write(this.#status, combatStatus(snapshot, catalog));
    if (snapshot.tick !== this.#tick) {
      this.#tick = snapshot.tick;
      const latest = snapshot.events.at(-1);
      if (latest) {
        this.#latest = { text: eventText(latest, snapshot, catalog), until: now + FEEDBACK_MS };
      }
    }
    write(this.#feedback, this.#latest && this.#latest.until > now ? this.#latest.text : "");
  }
}

function write(line: HudLine, text: string): void {
  if (line.textContent !== text) {
    line.textContent = text;
  }
}
