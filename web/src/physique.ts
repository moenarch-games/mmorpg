/**
 * The picker's physique editor (#634, Atlas 5.1.i633): six presets and eleven sliders in the
 * Appearance panel, with a live preview of the stat modifiers. Every number here comes from
 * `mmorpg-core` through `mmorpg-wasm` (the slider list, presets, ±2 % bound and the modifiers
 * themselves); this module holds only labels, layout and the page's edit/cancel/confirm state.
 */

/** `physiqueModel()`'s JSON. */
export type PhysiqueModel = {
  scale: number;
  neutral: number;
  maxModifierBp: number;
  params: { key: string; affectsStats: boolean }[];
  attributes: string[];
  presets: { name: string; values: number[] }[];
};

const LABELS: Readonly<Record<string, string>> = {
  frame: "Frame",
  muscularity: "Muscularity",
  bodyFat: "Body fat",
  shoulderWidth: "Shoulder width",
  height: "Height",
  bustFullness: "Bust fullness",
  waistWidth: "Waist width",
  hipWidth: "Hip width",
  gluteFullness: "Glute fullness",
  jawWidth: "Jaw width",
  jawProjection: "Jaw projection",
  maxHealth: "Maximum health",
  meleeDamage: "Melee damage",
};

/** The cosmetic groups of the approved layout; the stat sliders form "Build". */
const COSMETIC_GROUPS: readonly { title: string; keys: readonly string[] }[] = [
  { title: "Body proportions", keys: ["bustFullness", "waistWidth", "hipWidth", "gluteFullness"] },
  { title: "Face structure", keys: ["jawWidth", "jawProjection"] },
];

/**
 * The sliders the player body draws (#635), from the audit of the bundles players wear: the male
 * Ranger has `build.lean`/`build.heavy` keys on the body and every outfit, plate and hairstyle
 * (body fat), a declared height scale (height) and `face.broad`/`face.narrow` keys (jaw width).
 * No bundle has frame, muscularity, shoulder, bust, waist, hip, glute or jaw-projection morphs.
 */
export const DRAWN_PHYSIQUE: readonly string[] = ["bodyFat", "height", "jawWidth"];

export const label = (key: string): string => LABELS[key] ?? key;

/** `values` as the renderer's physique look: slider key to its value in [0, 1]. */
export function physiqueLook(model: PhysiqueModel, values: readonly number[]): Record<string, number> {
  return Object.fromEntries(model.params.map((param, index) => [param.key, (values[index] ?? model.neutral) / model.scale]));
}

/** A modifier in basis points as a signed percentage, e.g. `+1.25%`, `−0.50%`, `0%`. */
export function formatModifier(bp: number): string {
  if (bp === 0) return "0%";
  const sign = bp > 0 ? "+" : "−";
  return `${sign}${(Math.abs(bp) / 100).toFixed(2)}%`;
}

/** The preset whose exact vector `values` is, if any (presets never stack: one is just values). */
export function matchingPreset(model: PhysiqueModel, values: readonly number[]): string | null {
  return model.presets.find((preset) => preset.values.every((value, index) => value === values[index]))?.name ?? null;
}

export function neutralPhysique(model: PhysiqueModel): number[] {
  return model.params.map(() => model.neutral);
}

/** Whether `values` is a valid fixed-point physique of `model`. */
export function isPhysique(model: PhysiqueModel, values: unknown): values is number[] {
  return Array.isArray(values) && values.length === model.params.length &&
    values.every((value) => Number.isInteger(value) && value >= 0 && value <= model.scale);
}

const PENDING_PREFIX = "mmorpg.pending-physique.v1.";
export type PendingStorage = Pick<Storage, "getItem" | "setItem">;

/**
 * Storage key of the physique a never-stored local character was confirmed with. The page keeps
 * it only until the first entry creates the character's durable record with it; from then the
 * record is the one source.
 */
export function pendingPhysiqueKey(characterId: string): string {
  if (!characterId) throw new Error("Character ID is required for the pending physique");
  return `${PENDING_PREFIX}${characterId}`;
}

export function savePendingPhysique(storage: PendingStorage, characterId: string, values: readonly number[]): void {
  storage.setItem(pendingPhysiqueKey(characterId), JSON.stringify(values));
}

/** The pending physique, or null when there is none or it does not fit `model`. */
export function loadPendingPhysique(storage: PendingStorage, model: PhysiqueModel, characterId: string): number[] | null {
  const raw = storage.getItem(pendingPhysiqueKey(characterId));
  if (raw === null) return null;
  try {
    const values: unknown = JSON.parse(raw);
    return isPhysique(model, values) ? values : null;
  } catch {
    return null;
  }
}

export type PhysiqueEditorOptions = {
  model: PhysiqueModel;
  /** `mmorpg-core`'s modifiers of `values`, in `model.attributes` order. */
  modifiers(values: readonly number[]): number[];
  /** Saves `values`; resolves the status to show, rejects with the reason it was not saved. */
  confirm(values: readonly number[]): Promise<string>;
};

const percent = (model: PhysiqueModel, value: number) => Math.round((value / model.scale) * 100);

/**
 * The Physique group inside the Appearance panel. Edits are a preview until Confirm saves them;
 * Cancel returns to the saved values. A cosmetic slider is enabled only when the player body
 * draws it (#635, `DRAWN_PHYSIQUE`): the others never pretend to change the preview.
 */
export class PhysiqueEditor {
  readonly #root: HTMLElement;
  readonly #options: PhysiqueEditorOptions;
  readonly #inputs: { index: number; input: HTMLInputElement; output: HTMLOutputElement }[] = [];
  readonly #presetButtons: HTMLButtonElement[] = [];
  readonly #stats: { bp: HTMLElement; attribute: string }[] = [];
  readonly #statNote: HTMLElement;
  readonly #status: HTMLElement;
  readonly #cancel: HTMLButtonElement;
  readonly #confirm: HTMLButtonElement;
  #saved: number[];
  #values: number[];
  #unavailable: string | null = null;
  #saving = false;
  #generation = 0;

  constructor(root: HTMLElement, options: PhysiqueEditorOptions) {
    this.#root = root;
    this.#options = options;
    const { model } = options;
    this.#saved = neutralPhysique(model);
    this.#values = [...this.#saved];
    const doc = root.ownerDocument;
    const element = <K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string) => {
      const node = doc.createElement(tag);
      if (className) node.className = className;
      if (text !== undefined) node.textContent = text;
      return node;
    };

    const drawn = (key: string) => DRAWN_PHYSIQUE.includes(key);
    const disclosure = element("p", "physique-disclosure",
      "The body shows body fat, height and jaw width. Frame, muscularity and shoulder width change stats only: the body has no shapes for them yet. Your values are saved either way, and the stat preview below is exact.");

    const presets = element("div", "physique-presets");
    presets.setAttribute("role", "group");
    presets.setAttribute("aria-label", "Physique presets");
    for (const preset of model.presets) {
      const button = element("button", "physique-preset", preset.name);
      button.type = "button";
      button.dataset.physiquePreset = preset.name;
      button.setAttribute("aria-pressed", "false");
      button.addEventListener("click", () => this.#preview([...preset.values]));
      presets.append(button);
      this.#presetButtons.push(button);
    }

    const slider = (index: number, disabled: boolean) => {
      const key = model.params[index]!.key;
      const row = element("label", "physique-slider");
      const name = element("span", undefined, label(key));
      const input = element("input");
      input.type = "range";
      input.min = "0";
      input.max = "100";
      input.step = "1";
      input.dataset.physique = key;
      input.disabled = disabled;
      const output = element("output");
      input.addEventListener("input", () => {
        const next = [...this.#values];
        next[index] = Math.round((Number(input.value) / 100) * model.scale);
        this.#preview(next);
      });
      row.append(name, input, output);
      this.#inputs.push({ index, input, output });
      return row;
    };

    const build = element("fieldset", "physique-group");
    build.append(element("legend", undefined, "Build"));
    model.params.forEach((param, index) => { if (param.affectsStats) build.append(slider(index, false)); });

    const stats = element("div", "physique-stats");
    stats.setAttribute("aria-live", "polite");
    const list = element("dl");
    for (const attribute of model.attributes) {
      const bp = element("dd");
      list.append(element("dt", undefined, label(attribute)), bp);
      this.#stats.push({ bp, attribute });
    }
    this.#statNote = element("p", "physique-stat-note");
    stats.append(list, this.#statNote);

    // A cosmetic slider is enabled only when the body draws it; the others never fake a shape.
    const cosmetic = COSMETIC_GROUPS.map((group) => {
      const available = group.keys.some(drawn);
      const fieldset = element("fieldset", available ? "physique-group" : "physique-group physique-group--unavailable");
      fieldset.append(element("legend", undefined, group.title));
      for (const key of group.keys) {
        const index = model.params.findIndex((param) => param.key === key);
        if (index >= 0) fieldset.append(slider(index, !drawn(key)));
      }
      return fieldset;
    });
    const cosmeticNote = element("p", "physique-unavailable",
      "Bust, waist, hip, glute and jaw projection are disabled: the body has no shapes for them yet, so they stay off rather than fake one. Saved values are kept, and these never change stats.");

    this.#cancel = element("button", "physique-cancel", "Cancel");
    this.#cancel.type = "button";
    this.#cancel.addEventListener("click", () => {
      this.#preview([...this.#saved]);
      this.#status.textContent = "Changes cancelled; the saved physique is unchanged.";
    });
    this.#confirm = element("button", "physique-confirm", "Confirm physique");
    this.#confirm.type = "button";
    this.#confirm.addEventListener("click", () => void this.#save());
    const actions = element("div", "physique-actions");
    actions.append(this.#cancel, this.#confirm);
    this.#status = element("p", "physique-status");
    this.#status.setAttribute("role", "status");

    root.replaceChildren(disclosure, presets, build, stats, ...cosmetic, cosmeticNote, actions, this.#status);
    this.#render();
  }

  /** The values on screen, saved or not. */
  get values(): number[] {
    return [...this.#values];
  }

  /** The saved values, which the world draws. */
  get savedValues(): number[] {
    return [...this.#saved];
  }

  /** Whether the values on screen differ from the saved ones. */
  get dirty(): boolean {
    return this.#values.some((value, index) => value !== this.#saved[index]);
  }

  /** Shows `values` as the saved physique, discarding unsaved edits. */
  load(values: readonly number[], status: string): void {
    this.#generation += 1;
    this.#saved = [...values];
    this.#values = [...values];
    this.#status.textContent = status;
    this.#render();
  }

  /** Disables saving with `reason` shown, or enables it again with null. */
  setUnavailable(reason: string | null): void {
    if (reason === this.#unavailable) return;
    this.#unavailable = reason;
    if (reason) this.#status.textContent = reason;
    this.#render();
  }

  #preview(values: number[]): void {
    this.#values = values;
    this.#status.textContent = this.dirty ? "Unsaved preview · Confirm to keep it or Cancel." : "Matches the saved physique.";
    this.#render();
  }

  async #save(): Promise<void> {
    if (!this.dirty || this.#saving || this.#unavailable) return;
    this.#saving = true;
    this.#render();
    const values = [...this.#values];
    const generation = this.#generation;
    try {
      const status = await this.#options.confirm(values);
      // Another character loaded meanwhile: its values stay on screen.
      if (generation === this.#generation) {
        this.#saved = values;
        this.#status.textContent = status;
      }
    } catch (error) {
      if (generation === this.#generation) {
        this.#status.textContent = `Physique not saved: ${error instanceof Error ? error.message : String(error)}`;
      }
    } finally {
      this.#saving = false;
      this.#render();
    }
  }

  #render(): void {
    const { model } = this.#options;
    for (const { index, input, output } of this.#inputs) {
      const shown = percent(model, this.#values[index]!);
      input.value = String(shown);
      input.setAttribute("aria-valuetext", `${shown}%`);
      output.textContent = `${shown}%`;
    }
    const preset = matchingPreset(model, this.#values);
    for (const button of this.#presetButtons) {
      const selected = button.dataset.physiquePreset === preset;
      button.classList.toggle("is-selected", selected);
      button.setAttribute("aria-pressed", String(selected));
    }
    const modifiers = this.#options.modifiers(this.#values);
    this.#stats.forEach(({ bp }, index) => {
      const value = modifiers[index] ?? 0;
      const capped = Math.abs(value) >= model.maxModifierBp;
      bp.textContent = `${formatModifier(value)}${capped ? " (cap)" : ""}`;
      bp.dataset.sign = value > 0 ? "up" : value < 0 ? "down" : "none";
    });
    const cap = formatModifier(model.maxModifierBp).replace("+", "±");
    this.#statNote.textContent = modifiers.every((value) => value === 0)
      ? `No stat change. Build sliders combine into at most ${cap} per attribute; proportions and face never change stats.`
      : `Combined effect per attribute, capped at ${cap}. Proportions and face never change stats.`;
    this.#cancel.disabled = !this.dirty || this.#saving;
    this.#confirm.disabled = !this.dirty || this.#saving || this.#unavailable !== null;
    this.#root.dataset.dirty = String(this.dirty);
  }
}
