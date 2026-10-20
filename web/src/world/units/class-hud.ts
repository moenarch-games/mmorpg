import { findEntity, STRIKE_HEIGHTS, STRIKE_SIDES, type AuraState, type ResourceKind, type StrikeAim, type ZoneSnapshot } from "../../replication";
import type { ContentCatalog } from "../catalog";
import { abilitySlots } from "./abilities";
import { artworkOf, auraPolarity, glyphOf } from "./ability-presentation";
import { cooldownSeconds, resourceName, slotState, sweepDegrees, targetDistanceUnits, tooltipText, type SlotState } from "./ability-state";
import { auraLabel, auraTimer, sortAuras } from "./aura-display";
import { castBarView } from "./cast-bar";
import { HEROIC_STRIKE_ABILITY } from "./directed-strike";
import { guardDisplay } from "./shield-guard";
import { COMBAT_TEXT_MS, INTERRUPT_FLASH_MS, combatTexts, viewerInterrupted } from "./combat-text";
import { unitName } from "./combat-hud";

/**
 * The class kit's DOM: action bar, player and target frames with auras, both cast bars and the
 * floating combat text. Every value comes from the decoded projection and the exported catalog;
 * the only output is `onUse` / `onCancel` intent, which the zone validates. Elements are written
 * only when their value changes.
 */
export type ClassHudCallbacks = { onUse: (slot: number) => void };

const SVG = "http://www.w3.org/2000/svg";
const LONG_PRESS_MS = 450;
const TOOLTIP_MARGIN_PX = 8;
const MAX_FLOATING_TEXTS = 12;
const KEYBIND_LABELS = ["1", "2", "3", "4", "5", "6", "7", "8"] as const;

function part<T extends HTMLElement>(root: HTMLElement, name: string): T {
  const element = root.querySelector<T>(`[data-part="${name}"]`);
  if (!element) {
    throw new Error(`The class HUD is missing ${name}`);
  }
  return element;
}

function setText(element: HTMLElement, text: string): void {
  if (element.textContent !== text) {
    element.textContent = text;
  }
}

function setStyle(element: HTMLElement, name: string, value: string): void {
  if (element.style.getPropertyValue(name) !== value) {
    element.style.setProperty(name, value);
  }
}

function setData(element: HTMLElement, name: string, value: string | null): void {
  if (value === null) {
    if (element.hasAttribute(`data-${name}`)) {
      element.removeAttribute(`data-${name}`);
    }
  } else if (element.getAttribute(`data-${name}`) !== value) {
    element.setAttribute(`data-${name}`, value);
  }
}

function glyphElement(glyph: string, accent: string): SVGSVGElement {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  const path = document.createElementNS(SVG, "path");
  path.setAttribute("d", glyph);
  path.setAttribute("fill", accent);
  path.setAttribute("fill-rule", "evenodd");
  svg.append(path);
  return svg;
}

type SlotElements = {
  button: HTMLButtonElement;
  keybind: HTMLElement;
  /** The directed-strike binding hint on the Heroic Strike slot (Atlas 5.2.57). */
  alt: HTMLElement | null;
  timer: HTMLElement;
  unlock: HTMLElement;
};

const RESOURCE_CLASS: Record<ResourceKind, string> = { rage: "rage", focus: "focus", mana: "mana" };

export class ClassHud {
  readonly #callbacks: ClassHudCallbacks;
  readonly #bar: HTMLElement;
  readonly #tooltip: HTMLElement;
  readonly #cast: { root: HTMLElement; fill: HTMLElement; name: HTMLElement; time: HTMLElement };
  readonly #player: { name: HTMLElement; health: HTMLElement; healthText: HTMLElement; resource: HTMLElement; resourceText: HTMLElement; buffs: HTMLElement; debuffs: HTMLElement };
  readonly #target: { root: HTMLElement; name: HTMLElement; health: HTMLElement; healthText: HTMLElement; buffs: HTMLElement; debuffs: HTMLElement; cast: HTMLElement; castFill: HTMLElement; castName: HTMLElement };
  readonly #text: HTMLElement;
  readonly #floating: { element: HTMLElement; until: number }[] = [];
  readonly #auraSignatures = new Map<HTMLElement, string>();
  #slots: SlotElements[] = [];
  #slotKey = "";
  #longPress: { slot: number; timer: ReturnType<typeof setTimeout> } | null = null;
  #suppressClick = false;
  #lastPointerType = "";
  #interruptUntil = 0;
  #tick = -1n;
  #catalog: ContentCatalog | null = null;
  #latest: ZoneSnapshot | null = null;
  /** Labels of the directed-strike action's working bindings; empty when unbound. */
  #directedLabels: readonly string[] = [];
  #tooltipIndex: number | null = null;
  readonly #aim: { root: HTMLElement; cells: HTMLElement[] };
  readonly #guard: { root: HTMLElement; cells: HTMLElement[]; label: HTMLElement };

  constructor(root: HTMLElement, callbacks: ClassHudCallbacks) {
    this.#callbacks = callbacks;
    this.#bar = part(root, "action-bar");
    this.#tooltip = part(root, "tooltip");
    this.#cast = { root: part(root, "cast"), fill: part(root, "cast-fill"), name: part(root, "cast-name"), time: part(root, "cast-time") };
    this.#player = {
      name: part(root, "player-name"), health: part(root, "player-health"), healthText: part(root, "player-health-text"),
      resource: part(root, "player-resource"), resourceText: part(root, "player-resource-text"),
      buffs: part(root, "player-buffs"), debuffs: part(root, "player-debuffs"),
    };
    this.#target = {
      root: part(root, "target"), name: part(root, "target-name"), health: part(root, "target-health"),
      healthText: part(root, "target-health-text"), buffs: part(root, "target-buffs"), debuffs: part(root, "target-debuffs"),
      cast: part(root, "target-cast"), castFill: part(root, "target-cast-fill"), castName: part(root, "target-cast-name"),
    };
    this.#text = part(root, "combat-text");
    // The held directed-strike aim: a 3 × 3 grid, high row first, left column first.
    const aimRoot = document.createElement("div");
    aimRoot.className = "strike-aim";
    aimRoot.dataset.part = "strike-aim";
    aimRoot.setAttribute("aria-hidden", "true");
    aimRoot.hidden = true;
    const cells = [...STRIKE_HEIGHTS].reverse().flatMap((height) => STRIKE_SIDES.map((side) => {
      const cell = document.createElement("span");
      cell.dataset.aim = `${height}-${side}`;
      return cell;
    }));
    aimRoot.append(...cells);
    root.append(aimRoot);
    this.#aim = { root: aimRoot, cells };
    // The zone's guard (#156): high, middle and low lanes top to bottom, and its state in words.
    const guardRoot = document.createElement("div");
    guardRoot.className = "shield-guard";
    guardRoot.dataset.part = "shield-guard";
    guardRoot.setAttribute("role", "status");
    guardRoot.hidden = true;
    const guardCells = [...STRIKE_HEIGHTS].reverse().map((height) => {
      const cell = document.createElement("span");
      cell.dataset.lane = height;
      return cell;
    });
    const guardLabel = document.createElement("em");
    guardRoot.append(...guardCells, guardLabel);
    root.append(guardRoot);
    this.#guard = { root: guardRoot, cells: guardCells, label: guardLabel };
    // The HUD itself ignores pointers, so a tap anywhere outside the action bar (the world canvas,
    // other controls) dismisses a touch tooltip through the document.
    root.ownerDocument.addEventListener("pointerdown", (event) => {
      if (!(event.target instanceof Element) || !this.#bar.contains(event.target)) {
        this.#hideTooltip();
      }
    }, true);
  }

  /**
   * The directed-strike action's current working bindings (from `input-bindings`); the Heroic
   * Strike slot's hint and tooltip show them, or say the action is unbound.
   */
  setDirectedBinding(labels: readonly string[]): void {
    if (labels.join("\u0000") === this.#directedLabels.join("\u0000")) {
      return;
    }
    this.#directedLabels = [...labels];
    for (const slot of this.#slots) {
      this.#paintDirectedHint(slot);
    }
    if (this.#tooltipIndex !== null) {
      this.#tooltipFor(this.#tooltipIndex);
    }
  }

  /** Shows the held aim, or hides it with null. */
  showStrikeAim(aim: StrikeAim | null): void {
    const key = aim ? `${aim.height}-${aim.side}` : null;
    this.#aim.root.hidden = key === null;
    for (const cell of this.#aim.cells) {
      setData(cell, "on", cell.dataset.aim === key ? "true" : null);
    }
  }

  #paintDirectedHint(slot: SlotElements): void {
    const index = this.#slots.indexOf(slot);
    const name = this.#catalog?.abilities.get(Number(slot.button.dataset.ability))?.name ?? "Ability";
    const directed = slot.alt === null
      ? ""
      : this.#directedLabels.length > 0
        ? `; hold ${this.#directedLabels[0]} and aim to strike directed`
        : "; directed strike unbound";
    slot.button.setAttribute("aria-label", `${name} (key ${KEYBIND_LABELS[index] ?? ""}${directed})`);
    if (slot.alt) {
      setText(slot.alt, this.#directedLabels[0] ?? "");
      slot.alt.hidden = this.#directedLabels.length === 0;
    }
  }

  /** Clears everything for a new session. */
  reset(): void {
    this.#cancelLongPress();
    this.#hideTooltip();
    this.#bar.replaceChildren();
    this.#slots = [];
    this.#slotKey = "";
    this.#interruptUntil = 0;
    this.#tick = -1n;
    this.#latest = null;
    this.#catalog = null;
    for (const entry of this.#floating.splice(0)) {
      entry.element.remove();
    }
    this.#auraSignatures.clear();
    for (const container of [this.#player.buffs, this.#player.debuffs, this.#target.buffs, this.#target.debuffs]) {
      container.replaceChildren();
    }
    this.#cast.root.hidden = true;
    this.#target.root.hidden = true;
  }

  /** Reads one received projection's events once, however many frames show it. */
  receive(snapshot: ZoneSnapshot, now: number): void {
    if (snapshot.tick <= this.#tick) {
      return;
    }
    this.#tick = snapshot.tick;
    if (viewerInterrupted(snapshot)) {
      this.#interruptUntil = now + INTERRUPT_FLASH_MS;
    }
    for (const text of combatTexts(snapshot)) {
      this.#floatText(text.kind, text.text, now);
    }
  }

  /** Redraws the frames, bars and slots from the newest projection. */
  update(snapshot: ZoneSnapshot, catalog: ContentCatalog, now: number): void {
    this.#catalog = catalog;
    this.#latest = snapshot;
    this.#updateBar(snapshot, catalog);
    this.#updatePlayer(snapshot, catalog);
    this.#updateTarget(snapshot, catalog);
    this.#updateCast(snapshot, catalog, now);
    this.#updateGuard(snapshot);
    this.#expireText(now);
  }

  /** Shows the lane the zone holds and whether it protects, is suspended (airborne, stunned) or broken. */
  #updateGuard(snapshot: ZoneSnapshot): void {
    const shown = guardDisplay(snapshot.viewer.guard);
    this.#guard.root.hidden = shown === null;
    setData(this.#guard.root, "state", shown?.state ?? null);
    for (const cell of this.#guard.cells) {
      setData(cell, "on", cell.dataset.lane === shown?.lane ? "true" : null);
    }
    const words = shown === null ? "" : shown.state === "broken"
      ? `Guard broken ${(snapshot.viewer.guard.broken / 30).toFixed(1)} s`
      : shown.state === "protecting" ? `Guarding ${shown.lane}` : `Guard ${shown.lane} (not set)`;
    setText(this.#guard.label, words);
  }

  #updateBar(snapshot: ZoneSnapshot, catalog: ContentCatalog): void {
    const abilities = abilitySlots(snapshot, catalog);
    const key = abilities.map((ability) => ability.id).join(",");
    if (key !== this.#slotKey) {
      this.#slotKey = key;
      this.#buildSlots(abilities.map((ability) => ability.id), catalog);
    }
    const distance = targetDistanceUnits(snapshot);
    abilities.forEach((ability, index) => {
      const slot = this.#slots[index];
      if (!slot) {
        return;
      }
      const state = slotState(ability, snapshot, distance);
      this.#paintSlot(slot, state);
    });
  }

  #buildSlots(ids: readonly number[], catalog: ContentCatalog): void {
    this.#cancelLongPress();
    this.#hideTooltip();
    this.#slots = ids.map((id, index) => {
      const ability = catalog.abilities.get(id);
      const { glyph, accent } = glyphOf(id);
      const button = document.createElement("button");
      button.type = "button";
      button.className = "action-slot";
      button.tabIndex = -1;
      button.dataset.slot = String(index + 1);
      button.dataset.ability = String(id);
      button.setAttribute("aria-label", `${ability?.name ?? "Ability"} (key ${KEYBIND_LABELS[index]})`);
      const icon = document.createElement("span");
      icon.className = "slot-icon";
      const artwork = artworkOf(id);
      if (artwork) {
        const image = document.createElement("img");
        image.src = artwork.regular;
        image.srcset = `${artwork.regular} 1x, ${artwork.highDpi} 2x`;
        image.alt = "";
        image.setAttribute("aria-hidden", "true");
        image.draggable = false;
        image.decoding = "async";
        image.width = 64;
        image.height = 64;
        icon.append(image);
      } else {
        icon.append(glyphElement(glyph, accent));
      }
      const sweep = document.createElement("span");
      sweep.className = "slot-sweep";
      const keybind = document.createElement("kbd");
      keybind.className = "slot-key";
      keybind.textContent = KEYBIND_LABELS[index] ?? "";
      const timer = document.createElement("span");
      timer.className = "slot-timer";
      const unlock = document.createElement("span");
      unlock.className = "slot-unlock";
      button.append(icon, sweep, keybind, timer, unlock);
      let alt: HTMLElement | null = null;
      if (id === HEROIC_STRIKE_ABILITY) {
        alt = document.createElement("kbd");
        alt.className = "slot-alt";
        alt.title = "Directed Heroic Strike: hold, aim with the pointer, release";
        button.append(alt);
      }
      // A pressed slot must not take focus: Space and Enter would then activate it instead of jumping.
      button.addEventListener("mousedown", (event) => event.preventDefault());
      button.addEventListener("click", () => {
        if (this.#suppressClick) {
          this.#suppressClick = false;
          return;
        }
        this.#callbacks.onUse(index + 1);
      });
      button.addEventListener("contextmenu", (event) => {
        event.preventDefault();
        if (this.#lastPointerType === "touch") {
          // A touch long press: keep (or open) the tooltip the long-press timer shows, and swallow
          // this gesture's click.
          this.#cancelLongPress();
          this.#suppressClick = true;
          this.#showTooltip(index);
          return;
        }
        // A mouse hover has usually opened it already; right-click keeps it open.
        this.#showTooltip(index);
      });
      button.addEventListener("pointerenter", (event) => {
        if (event.pointerType === "mouse") {
          this.#showTooltip(index);
        }
      });
      button.addEventListener("pointerleave", (event) => {
        if (event.pointerType === "mouse") {
          this.#hideTooltip();
        }
      });
      button.addEventListener("pointerdown", (event) => {
        // A new gesture: a click swallowed for an earlier long press (or never delivered) is over.
        this.#lastPointerType = event.pointerType;
        this.#suppressClick = false;
        if (event.pointerType === "touch") {
          this.#cancelLongPress();
          this.#longPress = {
            slot: index,
            timer: setTimeout(() => {
              this.#longPress = null;
              this.#suppressClick = true;
              this.#showTooltip(index);
            }, LONG_PRESS_MS),
          };
        }
      });
      for (const end of ["pointerup", "pointercancel", "pointerleave"] as const) {
        button.addEventListener(end, (event) => {
          if (event.pointerType === "touch") {
            this.#cancelLongPress();
          }
        });
      }
      return { button, keybind, timer, unlock, alt };
    });
    this.#bar.replaceChildren(...this.#slots.map((slot) => slot.button));
    for (const slot of this.#slots) {
      this.#paintDirectedHint(slot);
    }
  }

  #paintSlot(slot: SlotElements, state: SlotState): void {
    const { button } = slot;
    setData(button, "state", !state.learned ? "locked" : state.casting ? "casting" : state.cooldown?.kind === "cooldown" ? "cooldown" : "ready");
    setData(button, "resource", state.learned && state.resourceShort ? "short" : null);
    setData(button, "range", state.learned && state.outOfRange ? "out" : null);
    setData(button, "gcd", state.cooldown?.kind === "gcd" ? "true" : null);
    const degrees = state.cooldown ? sweepDegrees(state.cooldown.remaining, state.cooldown.total) : 0;
    setStyle(button, "--sweep", `${degrees.toFixed(1)}deg`);
    const seconds = state.cooldown?.kind === "cooldown" ? cooldownSeconds(state.cooldown.remaining) : null;
    setText(slot.timer, seconds === null ? "" : String(seconds));
    setText(slot.unlock, state.learned ? "" : `Lv ${state.unlockLevel}`);
  }

  #tooltipFor(index: number): void {
    const snapshot = this.#latest;
    const catalog = this.#catalog;
    const button = this.#slots[index]?.button;
    const id = Number(button?.dataset.ability);
    const ability = catalog?.abilities.get(id);
    if (!snapshot || !catalog || !button || !ability) {
      return;
    }
    const tip = tooltipText(ability, catalog, snapshot.viewer.level);
    const tooltip = this.#tooltip;
    tooltip.replaceChildren();
    const title = document.createElement("strong");
    title.textContent = tip.title;
    tooltip.append(title);
    for (const line of tip.lines) {
      const row = document.createElement("span");
      row.textContent = line;
      tooltip.append(row);
    }
    if (tip.description) {
      const description = document.createElement("p");
      description.textContent = tip.description;
      tooltip.append(description);
    }
    if (id === HEROIC_STRIKE_ABILITY) {
      // One ability, two inputs: the directed mode shares this slot's Rage, cooldowns and unlock.
      const directed = document.createElement("span");
      directed.className = "tooltip-directed";
      const key = this.#directedLabels[0];
      directed.textContent = key
        ? `Directed: hold ${key}, move the pointer left/right and up (high) or down (low), release to strike. Same Rage and cooldowns.`
        : "Directed strike: unbound. Assign it in Settings › Controls.";
      tooltip.append(directed);
    }
    if (tip.note) {
      const note = document.createElement("em");
      note.textContent = tip.note;
      tooltip.append(note);
    }
    tooltip.hidden = false;
    const barBox = this.#bar.getBoundingClientRect();
    const box = button.getBoundingClientRect();
    // Centred over the slot, but kept inside the viewport on narrow screens.
    const half = tooltip.offsetWidth / 2;
    const viewport = tooltip.ownerDocument.documentElement.clientWidth;
    const centre = Math.min(Math.max(box.left + box.width / 2, half + TOOLTIP_MARGIN_PX), viewport - half - TOOLTIP_MARGIN_PX);
    tooltip.style.left = `${centre - barBox.left}px`;
  }

  #showTooltip(index: number): void {
    this.#tooltipIndex = index;
    this.#tooltipFor(index);
  }

  #hideTooltip(): void {
    this.#tooltipIndex = null;
    this.#tooltip.hidden = true;
  }

  #cancelLongPress(): void {
    if (this.#longPress) {
      clearTimeout(this.#longPress.timer);
      this.#longPress = null;
    }
  }

  #updatePlayer(snapshot: ZoneSnapshot, catalog: ContentCatalog): void {
    const viewer = snapshot.viewer;
    const className = viewer.classChoice?.classId;
    setText(this.#player.name, `${className ? className[0]!.toUpperCase() + className.slice(1) : "Adventurer"} · Lv ${viewer.level}`);
    this.#fillBar(this.#player.health, viewer.health, viewer.maxHealth);
    setText(this.#player.healthText, `${viewer.health} / ${viewer.maxHealth}`);
    const resource = viewer.resource;
    this.#player.resource.parentElement!.hidden = resource === null;
    if (resource) {
      setData(this.#player.resource.parentElement!, "resource", RESOURCE_CLASS[resource.kind]);
      this.#fillBar(this.#player.resource, resource.value, resource.max);
      setText(this.#player.resourceText, `${resourceName(resource.kind)} ${resource.value} / ${resource.max}`);
    }
    this.#renderAuras(this.#player.buffs, this.#player.debuffs, snapshot.auras, catalog);
  }

  #fillBar(fill: HTMLElement, value: number, max: number): void {
    const share = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;
    setStyle(fill, "--fill", `${(share * 100).toFixed(1)}%`);
    fill.parentElement?.setAttribute("aria-valuenow", String(value));
    fill.parentElement?.setAttribute("aria-valuemax", String(max));
  }

  #updateTarget(snapshot: ZoneSnapshot, catalog: ContentCatalog): void {
    const target = snapshot.viewer.target;
    const record = target ? findEntity(snapshot, target) : undefined;
    const frame = this.#target;
    frame.root.hidden = target === null;
    if (target === null) {
      return;
    }
    const name = unitName(target, snapshot, catalog);
    setText(frame.name, record ? `${name[0]!.toUpperCase()}${name.slice(1)} · Lv ${record.level}` : `${name[0]!.toUpperCase()}${name.slice(1)}`);
    const percent = record ? (record.flags.dead ? 0 : record.healthPercent) : 0;
    setStyle(frame.health, "--fill", `${percent}%`);
    frame.health.parentElement?.setAttribute("aria-valuenow", String(percent));
    setText(frame.healthText, record ? (record.flags.dead ? "Dead" : `${percent}%`) : "Out of sight");
    this.#renderAuras(frame.buffs, frame.debuffs, snapshot.targetDetail.auras, catalog);
    const cast = snapshot.targetDetail.cast;
    frame.cast.hidden = cast === null;
    if (cast) {
      const view = castBarView(cast, catalog);
      setText(frame.castName, view.name);
      setStyle(frame.castFill, "--fill", `${(view.fill * 100).toFixed(1)}%`);
      setData(frame.cast, "channel", view.channel ? "true" : null);
    }
  }

  #updateCast(snapshot: ZoneSnapshot, catalog: ContentCatalog, now: number): void {
    const cast = snapshot.viewer.cast;
    const flashing = now < this.#interruptUntil;
    const view = this.#cast;
    view.root.hidden = cast === null && !flashing;
    setData(view.root, "interrupted", cast === null && flashing ? "true" : null);
    if (cast) {
      const bar = castBarView(cast, catalog);
      setData(view.root, "channel", bar.channel ? "true" : null);
      setText(view.name, bar.name);
      setText(view.time, `${bar.remaining}s`);
      setStyle(view.fill, "--fill", `${(bar.fill * 100).toFixed(1)}%`);
    } else if (flashing) {
      setText(view.name, "Interrupted");
      setText(view.time, "");
      setStyle(view.fill, "--fill", "100%");
    }
  }

  #renderAuras(buffs: HTMLElement, debuffs: HTMLElement, auras: readonly AuraState[], catalog: ContentCatalog): void {
    const sorted = sortAuras(auras);
    this.#renderAuraRow(buffs, sorted.buffs, catalog);
    this.#renderAuraRow(debuffs, sorted.debuffs, catalog);
  }

  #renderAuraRow(container: HTMLElement, auras: readonly AuraState[], catalog: ContentCatalog): void {
    const signature = auras.map((aura) => `${aura.ability}:${aura.kind}:${auraTimer(aura)}:${aura.amount}`).join("|");
    if (this.#auraSignatures.get(container) === signature) {
      return;
    }
    this.#auraSignatures.set(container, signature);
    container.replaceChildren(...auras.map((aura) => {
      const { glyph, accent } = glyphOf(aura.ability);
      const icon = document.createElement("span");
      icon.className = "aura-icon";
      icon.dataset.polarity = auraPolarity(aura.kind);
      icon.dataset.ability = String(aura.ability);
      icon.title = auraLabel(aura, catalog);
      icon.setAttribute("role", "img");
      icon.setAttribute("aria-label", auraLabel(aura, catalog));
      const timer = document.createElement("span");
      timer.className = "aura-timer";
      timer.textContent = auraTimer(aura);
      icon.append(glyphElement(glyph, accent), timer);
      return icon;
    }));
  }

  #floatText(kind: string, text: string, now: number): void {
    const element = document.createElement("span");
    element.className = `combat-text-entry ct-${kind}`;
    element.textContent = text;
    element.style.setProperty("--drift", `${(this.#floating.length % 5 - 2) * 1.1}rem`);
    this.#text.append(element);
    this.#floating.push({ element, until: now + COMBAT_TEXT_MS });
    while (this.#floating.length > MAX_FLOATING_TEXTS) {
      this.#floating.shift()?.element.remove();
    }
  }

  #expireText(now: number): void {
    while (this.#floating[0] && this.#floating[0].until <= now) {
      this.#floating.shift()?.element.remove();
    }
  }
}
