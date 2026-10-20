import { ArenaPresentation } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { CHARACTER_CLASSES, isCharacterClassId, isCharacterSex, type CharacterPreview } from "../../character-selection";
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

/** Match-local setup is held in this form only. It never writes the character roster or storage. */
export class BattleRoyaleEntry {
  readonly #activity: HTMLSelectElement;
  readonly #root: HTMLElement;
  readonly #class: HTMLSelectElement;
  readonly #sex: HTMLSelectElement;
  readonly #server: HTMLInputElement;
  readonly #hash: HTMLInputElement;
  readonly #equipment: HTMLSelectElement[] = [];
  readonly #note: HTMLElement;
  readonly #changed: () => void;
  #catalog: ContentCatalog | null = null;
  #presentation: ArenaPresentation | null = null;

  constructor(activity: HTMLSelectElement, root: HTMLElement, changed: () => void) {
    this.#activity = activity; this.#root = root; this.#changed = changed;
    root.innerHTML = `<div class="arena-character-fields"><label>Class<select id="arena-class"></select></label>
      <label>Appearance<select id="arena-sex"><option value="male">Male</option><option value="female">Female</option></select></label></div>
      <fieldset id="arena-equipment"><legend>Equipment</legend></fieldset>
      <label>Match server<input id="arena-server" type="url" placeholder="https://host:4433/game/matches/br-0000000000000001" autocomplete="off"></label>
      <details><summary>Development certificate</summary><label>Certificate hash<input id="arena-cert-hash" type="text" autocomplete="off" spellcheck="false"></label></details>
      <p id="arena-setup-note"></p>`;
    this.#class = element(root, "#arena-class"); this.#sex = element(root, "#arena-sex");
    this.#server = element(root, "#arena-server"); this.#hash = element(root, "#arena-cert-hash"); this.#note = element(root, "#arena-setup-note");
    for (const choice of CHARACTER_CLASSES) { this.#class.add(new Option(choice.name, choice.id)); }
    const equipment = element<HTMLFieldSetElement>(root, "#arena-equipment");
    SLOT_LABELS.forEach((name, slot) => {
      const label = document.createElement("label"); label.append(name);
      const select = document.createElement("select"); select.id = `arena-slot-${slot}`;
      label.append(select); equipment.append(label); this.#equipment.push(select);
    });
    const query = new URLSearchParams(window.location.search);
    const server = query.get("arenaServer") ?? "";
    this.#server.value = server;
    this.#hash.value = query.get("certHash") ?? "";
    if (query.get("activity") === "battle-royale") { activity.value = "battle-royale"; }
    this.#class.addEventListener("change", () => { this.#defaults(); changed(); });
    root.addEventListener("change", (event) => { if (event.target !== this.#class) { changed(); } });
    activity.addEventListener("change", () => { root.hidden = !this.selected; changed(); });
    root.hidden = !this.selected;
  }
  get selected(): boolean { return this.#activity.value === "battle-royale"; }
  ready(catalog: ContentCatalog): void {
    this.#catalog = catalog; this.#presentation = new ArenaPresentation();
    this.#equipment.forEach((select, index) => {
      select.add(new Option("Empty", "0"));
      for (const item of catalog.items.values()) {
        if (item.slot === EQUIPMENT_SLOT_NAMES[index]) { select.add(new Option(item.name, String(item.id))); }
      }
    });
    this.#defaults();
    this.#note.textContent = `Level ${this.#presentation.maximumLevel()} sandbox. Your world character progress stays separate. A real match host is required.`;
  }
  #defaults(): void {
    if (!this.#presentation) { return; }
    const character = this.character({ id: "arena-guest", classId: "warden", sex: "male" });
    const defaults = this.#presentation.standardEquipment(classChoiceCodes(character).classId);
    this.#equipment.forEach((select, slot) => { select.value = String(defaults[slot] ?? 0); });
  }
  character(base: JoinCharacter): JoinCharacter {
    const classId = this.#class.value; const sex = this.#sex.value;
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
    if (!this.#server.value.trim()) { return null; }
    const route = parseSessionRoute(this.#server.value.trim(), "battle-royale");
    const hash = this.#hash.value.trim();
    return { route, label: new URL(route.url).host, certificateHash: hash ? parseCertificateHash(hash) : null };
  }
  setLocked(locked: boolean): void {
    this.#activity.disabled = locked;
    for (const input of this.#root.querySelectorAll<HTMLInputElement | HTMLSelectElement>("input, select")) { input.disabled = locked; }
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
