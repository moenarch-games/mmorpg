import { ArenaPresentation, classCard } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { CHARACTER_CLASSES, isCharacterClassId, isCharacterSex, type CharacterClassId, type CharacterPreview } from "../../character-selection";
import { parseCertificateHash, parseSessionRoute } from "../../session/route";
import { EQUIPMENT_SLOT_NAMES, type ContentCatalog } from "../catalog";
import type { ZoneServer } from "../online-config";
import { classChoiceCodes, type JoinCharacter } from "../world-source";
import { equipmentSlots } from "./world";
import type { ArenaEquipment, MatchProjection } from "./protocol";

const SLOT_LABELS = ["Main hand", "Off hand", "Head", "Chest", "Legs", "Feet"];
function element<T extends HTMLElement>(root: ParentNode, selector: string): T {
  const found = root.querySelector<T>(selector);
  if (!found) { throw new Error(`Missing Battle Royale control: ${selector}`); }
  return found;
}

/** Where the Loadout screen draws: the class column, the equipment and look column, and the arena route in the bottom bar. */
export type LoadoutRoots = { classes: HTMLElement; loadout: HTMLElement; route: HTMLElement };

/** The roster characters whose look a match build may wear; choosing one selects it, it never changes it. */
export type LookSource = {
  characters(): readonly CharacterPreview[];
  selected(): string;
  choose(id: string): void;
};

/**
 * The Battle Royale **Loadout** screen (`mmorpg_client_model::front_end`): pick only what one match
 * needs, a class, equipment and a look. Match-local setup is held in these controls only; it never
 * writes the character roster or storage.
 */
export class BattleRoyaleEntry {
  readonly #activity: HTMLSelectElement;
  readonly #roots: LoadoutRoots;
  readonly #classGrid: HTMLElement;
  readonly #classDescription: HTMLElement;
  readonly #sex: HTMLSelectElement;
  readonly #look: HTMLSelectElement;
  readonly #hosted: HTMLElement;
  readonly #server: HTMLInputElement;
  readonly #hash: HTMLInputElement;
  readonly #equipment: HTMLSelectElement[] = [];
  readonly #note: HTMLElement;
  readonly #looks: LookSource;
  readonly #changed: () => void;
  #classId: CharacterClassId = "warden";
  #catalog: ContentCatalog | null = null;
  #presentation: ArenaPresentation | null = null;

  constructor(activity: HTMLSelectElement, roots: LoadoutRoots, looks: LookSource, changed: () => void) {
    this.#activity = activity; this.#roots = roots; this.#looks = looks; this.#changed = changed;
    roots.classes.innerHTML = `<h2 class="loadout-heading">Class</h2>
      <div id="arena-classes-grid" class="arena-class-grid" role="radiogroup" aria-label="Class"></div>
      <p id="arena-class-description" class="arena-class-description"></p>`;
    roots.loadout.innerHTML = `<h2 class="loadout-heading">Equipment</h2>
      <fieldset id="arena-equipment" class="arena-slots"><legend class="visually-hidden">Equipment</legend></fieldset>
      <h2 class="loadout-heading">Look</h2>
      <div class="arena-look">
        <label>Character<select id="arena-look"></select></label>
        <label>Body<select id="arena-sex"><option value="male">Male</option><option value="female">Female</option></select></label>
      </div>
      <p class="arena-look-note">Wears a roster character's look for this match. Nothing is changed or saved.</p>`;
    roots.route.innerHTML = `<div class="arena-mode" role="radiogroup" aria-label="Arena">
        <label><input type="radio" name="arena-mode" value="practice" checked><span>Practice</span></label>
        <label><input type="radio" name="arena-mode" value="hosted"><span>Hosted match</span></label>
      </div>
      <div id="arena-hosted" class="arena-hosted"><label>Match server<input id="arena-server" type="url" placeholder="https://host:4433/game/matches/br-0000000000000001" autocomplete="off"></label>
      <details><summary>Development certificate</summary><label>Certificate hash<input id="arena-cert-hash" type="text" autocomplete="off" spellcheck="false"></label></details></div>
      <p id="arena-setup-note" class="arena-setup-note"></p>`;
    this.#classGrid = element(roots.classes, "#arena-classes-grid");
    this.#classDescription = element(roots.classes, "#arena-class-description");
    this.#sex = element(roots.loadout, "#arena-sex"); this.#look = element(roots.loadout, "#arena-look");
    this.#hosted = element(roots.route, "#arena-hosted");
    this.#server = element(roots.route, "#arena-server"); this.#hash = element(roots.route, "#arena-cert-hash");
    this.#note = element(roots.route, "#arena-setup-note");
    const equipment = element<HTMLFieldSetElement>(roots.loadout, "#arena-equipment");
    SLOT_LABELS.forEach((name, slot) => {
      const label = document.createElement("label"); label.append(name);
      const select = document.createElement("select"); select.id = `arena-slot-${slot}`;
      label.append(select); equipment.append(label); this.#equipment.push(select);
    });
    const query = new URLSearchParams(window.location.search);
    const server = query.get("arenaServer") ?? "";
    this.#server.value = server;
    this.#hash.value = query.get("certHash") ?? "";
    // A match server in the URL asks for the hosted match; otherwise the arena is practice.
    this.#modeInput(server ? "hosted" : "practice").checked = true;
    this.#hosted.hidden = this.practice;
    for (const input of roots.route.querySelectorAll<HTMLInputElement>('input[name="arena-mode"]')) {
      input.addEventListener("change", () => { this.#hosted.hidden = this.practice; this.#describe(); });
    }
    if (query.get("activity") === "battle-royale") { activity.value = "battle-royale"; }
    this.#look.addEventListener("change", () => {
      const character = looks.characters().find((candidate) => candidate.id === this.#look.value);
      if (character) { this.#sex.value = character.sex; }
      looks.choose(this.#look.value);
    });
    for (const root of [roots.loadout, roots.route]) {
      root.addEventListener("change", (event) => { if (event.target !== this.#look) { changed(); } });
    }
    activity.addEventListener("change", () => { this.#show(); changed(); });
    this.#show();
  }
  get selected(): boolean { return this.#activity.value === "battle-royale"; }
  /** Whether the arena is the local practice arena (#550) rather than a hosted match. */
  get practice(): boolean { return this.#modeInput("practice").checked; }
  #modeInput(mode: "practice" | "hosted"): HTMLInputElement {
    return element(this.#roots.route, `input[name="arena-mode"][value="${mode}"]`);
  }
  #show(): void {
    for (const root of Object.values(this.#roots)) { root.hidden = !this.selected; }
  }
  ready(catalog: ContentCatalog): void {
    this.#catalog = catalog; this.#presentation = new ArenaPresentation();
    for (const choice of CHARACTER_CLASSES) {
      const card = classCard(choice.id);
      const tile = document.createElement("label");
      tile.className = "arena-class-tile";
      tile.innerHTML = `<input type="radio" name="arena-class" id="arena-class-${choice.id}" value="${choice.id}"><strong></strong><small></small>`;
      tile.querySelector("strong")!.textContent = card.name;
      tile.querySelector("small")!.textContent = card.role;
      const input = element<HTMLInputElement>(tile, "input");
      input.checked = choice.id === this.#classId;
      input.addEventListener("change", () => {
        if (!input.checked || !isCharacterClassId(input.value)) { return; }
        this.#classId = input.value;
        this.#defaults();
        this.#describeClass();
        this.#changed();
      });
      this.#classGrid.append(tile);
    }
    this.#equipment.forEach((select, index) => {
      select.add(new Option("Empty", "0"));
      for (const item of catalog.items.values()) {
        if (item.slot === EQUIPMENT_SLOT_NAMES[index]) { select.add(new Option(item.name, String(item.id))); }
      }
    });
    this.refreshLooks();
    const look = this.#looks.characters().find((character) => character.id === this.#looks.selected());
    if (look) { this.#sex.value = look.sex; }
    this.#defaults();
    this.#describeClass();
    this.#describe();
  }
  /** Lists the roster's characters as looks again, after the roster or its selection changed. */
  refreshLooks(): void {
    const selected = this.#looks.selected();
    this.#look.replaceChildren(...this.#looks.characters().map((character) =>
      new Option(`${character.name} · ${character.className}`, character.id, false, character.id === selected)));
  }
  #describeClass(): void {
    this.#classDescription.textContent = classCard(this.#classId).description;
  }
  #describe(): void {
    if (!this.#presentation) { return; }
    const level = this.#presentation.maximumLevel();
    this.#note.textContent = this.practice
      ? `Level ${level} practice against training targets, in this browser. Nothing is saved.`
      : `Level ${level} sandbox. Your world character progress stays separate. A real match host is required.`;
  }
  #defaults(): void {
    if (!this.#presentation) { return; }
    const character = this.character({ id: "arena-guest", classId: "warden", sex: "male" });
    const defaults = this.#presentation.standardEquipment(classChoiceCodes(character).classId);
    this.#equipment.forEach((select, slot) => { select.value = String(defaults[slot] ?? 0); });
  }
  character(base: JoinCharacter): JoinCharacter {
    const classId = this.#classId; const sex = this.#sex.value;
    if (!isCharacterClassId(classId) || !isCharacterSex(sex)) { throw new Error("Choose a supported class and appearance."); }
    return { id: base.id, classId, sex };
  }
  equipment(): ArenaEquipment { return equipmentSlots(this.#equipment.map((select) => Number(select.value))); }
  preview(base: CharacterPreview): CharacterPreview {
    if (!this.selected) { return base; }
    const character = this.character(base);
    const className = CHARACTER_CLASSES.find((candidate) => candidate.id === character.classId)?.name ?? base.className;
    return { ...base, ...character, className, level: this.#presentation?.maximumLevel() ?? base.level, location: "Battle Royale",
      equipment: this.#equipment.flatMap((select, slot) => {
        const item = this.#catalog?.items.get(Number(select.value));
        return item ? [{ slot: SLOT_LABELS[slot]!, name: item.name, accent: "#b7c6bd" }] : [];
      }) };
  }
  server(): ZoneServer | null {
    if (this.practice || !this.#server.value.trim()) { return null; }
    const route = parseSessionRoute(this.#server.value.trim(), "battle-royale");
    const hash = this.#hash.value.trim();
    return { route, label: new URL(route.url).host, certificateHash: hash ? parseCertificateHash(hash) : null };
  }
  setLocked(locked: boolean): void {
    this.#activity.disabled = locked;
    for (const root of Object.values(this.#roots)) {
      for (const input of root.querySelectorAll<HTMLInputElement | HTMLSelectElement>("input, select")) { input.disabled = locked; }
    }
  }
  nextMatch(): void {
    const server = this.server();
    if (!server) { return; }
    const id = BigInt(`0x${server.route.matchId.slice(3)}`);
    if (id < 0xffff_ffff_ffff_ffffn) {
      const next = `br-${(id + 1n).toString(16).padStart(16, "0")}`;
      this.#server.value = `${server.route.origin}${server.route.prefix}/matches/${next}`;
    }
    this.#changed();
  }
}

export function matchStatus(match: MatchProjection, viewerId: number): { result: string; detail: string; canConfigure: boolean } {
  const alive = match.players.filter((player) => player.alive).length;
  const eliminated = match.players.some((player) => player.id === viewerId && !player.alive);
  if (match.phase === "finished") {
    let result = "Eliminated";
    if (match.winner === viewerId) { result = "Victory"; }
    if (match.winner === null) { result = "Match finished"; }
    const detail = match.winner === null ? "No survivor" : `Player ${match.winner} wins · ${alive} remaining`;
    return { result, detail, canConfigure: true };
  }
  if (eliminated) { return { result: "Eliminated", detail: `${alive} remaining`, canConfigure: true }; }
  if (match.phase === "waiting") { return { result: "Waiting for players", detail: `${match.players.length} joined`, canConfigure: false }; }
  if (match.phase === "drop") { return { result: "Get ready", detail: "Your class and equipment are locked for this life", canConfigure: false }; }
  return { result: "Battle Royale", detail: `${alive} remaining`, canConfigure: false };
}
