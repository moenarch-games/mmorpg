import type { SettingState, SettingValue } from "@moritzbrantner/settings-browser";
import type { Profile } from "@moritzbrantner/input-bindings";
import { SETTINGS_TEXT, settingCategories, type Preferences, type UserImportPreview } from "./preferences";

/**
 * The one Settings modal (#41 Q9/Q11), reachable from character selection and in the world.
 * Values come from and go to the shared settings session; the Controls category embeds the
 * shared `input-bindings` workbench for the selected character's own profile, which saves on
 * every confirmed change (Q19) independently of the settings values.
 */
export type SettingsPanelElements = {
  dialog: HTMLDialogElement;
  close: HTMLButtonElement;
  categories: HTMLElement;
  categorySelect: HTMLSelectElement;
  search: HTMLInputElement;
  resetCategory: HTMLButtonElement;
  content: HTMLElement;
  status: HTMLElement;
  resetAll: HTMLButtonElement;
  exportUser: HTMLButtonElement;
  importUser: HTMLInputElement;
};

export type CharacterBindings = {
  characterId: string;
  characterName: string;
  profile: Profile;
  /** Saves and installs a confirmed profile; returns why it failed, or null. */
  save: (profile: Profile) => string | null;
};

export type SettingsPanelOptions = {
  preferences: Promise<Preferences>;
  bindings: () => CharacterBindings;
  onOpenChange: (open: boolean) => void;
};

const CONTROLS_CATEGORY = "settings.category.controls";
const text = (key: string): string => SETTINGS_TEXT[key] ?? key;

export class SettingsPanel {
  readonly #elements: SettingsPanelElements;
  readonly #options: SettingsPanelOptions;
  #preferences: Preferences | null = null;
  #category: string | null = null;
  #returnFocus: HTMLElement | null = null;
  #importPreview: UserImportPreview | null = null;
  readonly #advanced = new Set<string>();
  #searchTarget: string | null = null;
  #unmountEditor: (() => void) | null = null;

  constructor(elements: SettingsPanelElements, options: SettingsPanelOptions) {
    this.#elements = elements;
    this.#options = options;
    elements.close.addEventListener("click", () => this.close());
    elements.resetAll.addEventListener("click", () => this.#resetAll());
    elements.resetCategory.addEventListener("click", () => this.#resetCategory());
    elements.search.addEventListener("input", () => this.#render());
    elements.categorySelect.addEventListener("change", () => { this.#category = elements.categorySelect.value; this.#searchTarget = null; this.#render(); });
    elements.exportUser.disabled = true;
    elements.importUser.disabled = true;
    elements.exportUser.addEventListener("click", () => this.#exportUser());
    elements.importUser.addEventListener("change", () => { void this.#importUser(); });
    // Escape (cancel) and form-method closes all come through `close`.
    elements.dialog.addEventListener("close", () => this.#closed());
    options.preferences.then((preferences) => {
      this.#preferences = preferences;
      elements.exportUser.disabled = false;
      elements.importUser.disabled = false;
      if (this.open) {
        this.#render();
      }
    }, (error: unknown) => {
      elements.status.textContent = `Settings are unavailable: ${error instanceof Error ? error.message : String(error)}`;
    });
  }

  get open(): boolean {
    return this.#elements.dialog.open;
  }

  /** Opens the modal; closing returns focus to `returnFocus`, else to the element focused now. */
  show(returnFocus?: HTMLElement): void {
    if (this.open) {
      return;
    }
    const active = document.activeElement;
    this.#returnFocus = returnFocus ?? (active instanceof HTMLElement ? active : null);
    this.#options.onOpenChange(true);
    this.#elements.dialog.showModal();
    this.#render();
  }

  close(): void {
    if (this.open) {
      this.#elements.dialog.close();
    }
  }

  #closed(): void {
    this.#preferences?.cancelUserImport();
    this.#importPreview = null;
    this.#unmountEditor?.();
    this.#unmountEditor = null;
    this.#options.onOpenChange(false);
    this.#returnFocus?.focus({ preventScroll: true });
    this.#returnFocus = null;
  }

  #categories(): string[] {
    const preferences = this.#preferences;
    if (!preferences) {
      return [];
    }
    return settingCategories(preferences.presentation());
  }

  #render(): void {
    const preferences = this.#preferences;
    const { categories, content, status } = this.#elements;
    if (!preferences) {
      status.textContent = "Loading settings…";
      return;
    }
    status.textContent = preferences.problem ?? "";
    categories.hidden = this.#importPreview !== null;
    this.#elements.categorySelect.hidden = this.#importPreview !== null;
    this.#elements.search.disabled = this.#importPreview !== null;
    if (this.#importPreview) { this.#renderImport(preferences, this.#importPreview); return; }
    const keys = this.#categories();
    this.#category ??= keys[0] ?? null;
    categories.replaceChildren(...keys.map((key) => {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = text(key);
      button.setAttribute("aria-current", String(key === this.#category));
      button.addEventListener("click", () => {
        this.#category = key;
        this.#searchTarget = null;
        this.#render();
        this.#elements.categories.querySelector<HTMLButtonElement>('[aria-current="true"]')?.focus();
      });
      return button;
    }));
    this.#elements.categorySelect.replaceChildren(...keys.map((key) => {
      const option = document.createElement("option"); option.value = key; option.textContent = text(key); return option;
    }));
    this.#elements.categorySelect.value = this.#category ?? "";
    const currentStates = preferences.states();
    this.#elements.resetAll.hidden = !currentStates.some((state) => state.overridden);
    const states = new Map(currentStates.map((state) => [state.id, state]));
    this.#unmountEditor?.();
    this.#unmountEditor = null;
    const query = this.#elements.search.value.trim().toLowerCase();
    this.#elements.resetCategory.hidden = Boolean(query);
    const categoryIds = preferences.presentation().filter((entry) => entry.metadata.category_key === this.#category).map((entry) => entry.id);
    this.#elements.resetCategory.disabled = !currentStates.some((state) => state.overridden && categoryIds.includes(state.id));
    if (query) {
      const results: HTMLElement[] = [];
      for (const category of keys) {
        const entries = preferences.presentation().filter((entry) => {
          const metadata = entry.metadata;
          const searchable = [entry.id, text(metadata.label_key), text(metadata.description_key ?? ""), ...(metadata.search_keys ?? []).map(text)].join(" ").toLowerCase();
          return metadata.category_key === category && states.get(entry.id)?.availability.status !== "hidden" && searchable.includes(query);
        });
        if (entries.length === 0) continue;
        const heading = document.createElement("h3"); heading.textContent = text(category); results.push(heading);
        for (const entry of entries) {
          const button = document.createElement("button"); button.type = "button"; button.textContent = text(entry.metadata.label_key);
          button.addEventListener("click", () => {
            this.#category = category; this.#searchTarget = entry.id; this.#elements.search.value = "";
            if (entry.metadata.discoverability === "advanced") this.#advanced.add(category);
            this.#render();
            document.getElementById(`setting-${entry.id.replaceAll(".", "-")}`)?.focus();
          }); results.push(button);
        }
      }
      if (results.length === 0) { const empty = document.createElement("p"); empty.setAttribute("role", "status"); empty.textContent = "No matching settings."; results.push(empty); }
      content.replaceChildren(...results);
      return;
    }
    const entries = preferences.presentation().filter((entry) => entry.metadata.category_key === this.#category);
    const row = (entry: (typeof entries)[number]): HTMLElement[] => {
      const state = states.get(entry.id);
      return state && state.availability.status !== "hidden" ? [this.#row(preferences, entry.id, entry.metadata.label_key, entry.metadata.description_key, state)] : [];
    };
    content.replaceChildren(...entries.filter((entry) => !entry.metadata.discoverability || entry.metadata.discoverability === "primary" || entry.id === this.#searchTarget && entry.metadata.discoverability === "search_only").flatMap(row));
    const advanced = entries.filter((entry) => entry.metadata.discoverability === "advanced");
    if (advanced.length > 0) {
      const disclosure = document.createElement("details"); const summary = document.createElement("summary"); summary.textContent = "Advanced";
      const category = this.#category ?? "";
      disclosure.open = this.#advanced.has(category);
      disclosure.addEventListener("toggle", () => { if (disclosure.open) this.#advanced.add(category); else this.#advanced.delete(category); });
      disclosure.append(summary, ...advanced.flatMap(row)); content.append(disclosure);
    }
    if (this.#category === CONTROLS_CATEGORY) content.append(this.#bindingsSection());
  }


  #row(preferences: Preferences, id: string, labelKey: string, descriptionKey: string | undefined, state: SettingState): HTMLElement {
    const row = document.createElement("div");
    row.className = "settings-row";
    const label = document.createElement("label");
    label.className = "settings-label";
    const inputId = `setting-${id.replaceAll(".", "-")}`;
    label.htmlFor = inputId;
    label.textContent = text(labelKey);
    const description = document.createElement("p");
    description.className = "settings-description";
    description.textContent = descriptionKey ? text(descriptionKey) : "";
    const control = document.createElement("div");
    control.className = "settings-control";
    const commit = (value: SettingValue) => {
      try {
        preferences.set(id, value);
        this.#render();
        document.getElementById(inputId)?.focus();
      } catch (error) {
        this.#elements.status.textContent = `${text(labelKey)}: ${error instanceof Error ? error.message : String(error)}`;
      }
    };
    const definition = preferences.definition(id);
    const kind = definition?.kind;
    if (kind?.type === "bool") {
      const checkbox = document.createElement("input");
      checkbox.id = inputId;
      checkbox.type = "checkbox";
      checkbox.checked = state.value.type === "bool" && state.value.value;
      checkbox.addEventListener("change", () => commit({ type: "bool", value: checkbox.checked }));
      control.append(checkbox);
    } else if (kind && (kind.type === "integer" || kind.type === "number")) {
      const numeric = state.value.type === "integer" || state.value.type === "number" ? state.value.value : 0;
      const step = kind.type === "integer" ? "1" : "0.05";
      const exact = document.createElement("input");
      exact.id = inputId;
      exact.type = "number";
      exact.min = String(kind.min);
      exact.max = String(kind.max);
      exact.step = step;
      exact.value = String(numeric);
      const slider = document.createElement("input");
      slider.type = "range";
      slider.min = exact.min;
      slider.max = exact.max;
      slider.step = step;
      slider.value = exact.value;
      slider.setAttribute("aria-label", `${text(labelKey)} slider`);
      const valueOf = (raw: string): SettingValue | null => {
        const parsed = Number(raw);
        if (raw.trim() === "" || !Number.isFinite(parsed)) {
          return null;
        }
        return kind.type === "integer" ? { type: "integer", value: Math.round(parsed) } : { type: "number", value: parsed };
      };
      exact.addEventListener("change", () => {
        const value = valueOf(exact.value);
        if (value) {
          commit(value);
        }
      });
      slider.addEventListener("change", () => {
        const value = valueOf(slider.value);
        if (value) {
          commit(value);
        }
      });
      slider.addEventListener("input", () => {
        exact.value = slider.value;
      });
      control.append(exact, slider);
    } else if (kind?.type === "choice") {
      const select = document.createElement("select");
      select.id = inputId;
      for (const option of kind.options) {
        const element = document.createElement("option");
        element.value = option;
        element.textContent = text(`settings.choice.${option}`);
        select.append(element);
      }
      select.value = state.value.type === "choice" ? state.value.value : "";
      select.addEventListener("change", () => commit({ type: "choice", value: select.value }));
      control.append(select);
    }
    const reset = document.createElement("button");
    reset.type = "button";
    reset.className = "settings-reset";
    reset.textContent = "Reset";
    reset.setAttribute("aria-label", `Reset ${text(labelKey)} to default`);
    reset.addEventListener("click", () => {
      preferences.reset(id);
      this.#render();
    });
    if (state.overridden) {
      control.append(reset);
    }
    row.append(label, control);
    if (description.textContent) {
      row.append(description);
    }
    return row;
  }

  #bindingsSection(): HTMLElement {
    const bindings = this.#options.bindings();
    const section = document.createElement("section");
    section.className = "settings-bindings";
    section.setAttribute("aria-label", `Key bindings for ${bindings.characterName}`);
    const host = document.createElement("div");
    host.className = "settings-bindings-host";
    host.textContent = "Loading the binding editor…";
    section.append(host);
    let cancelled = false;
    this.#unmountEditor = () => {
      cancelled = true;
    };
    void import("./bindings-editor").then(({ mountBindingsEditor }) => {
      if (cancelled) {
        return;
      }
      host.textContent = "";
      this.#unmountEditor = mountBindingsEditor(host, bindings);
    }, (error: unknown) => {
      host.textContent = `The binding editor could not load: ${error instanceof Error ? error.message : String(error)}`;
    });
    return section;
  }

  #exportUser(): void {
    if (!this.#preferences) return;
    const file = new Blob([this.#preferences.exportUserProfile()], { type: "application/json" });
    const url = URL.createObjectURL(file);
    const link = document.createElement("a");
    link.href = url;
    link.download = "mmorpg-user-preferences.json";
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  async #importUser(): Promise<void> {
    const preferences = this.#preferences;
    const file = this.#elements.importUser.files?.[0];
    if (!preferences || !file) return;
    this.#importPreview = null;
    preferences.cancelUserImport();
    this.#unmountEditor?.();
    this.#unmountEditor = null;
    this.#elements.content.textContent = "Checking User preferences…";
    this.#elements.categories.hidden = true;
    this.#elements.resetAll.hidden = true;
    this.#elements.importUser.disabled = true;
    this.#elements.status.textContent = "Checking User preferences…";
    try {
      const json = await file.text();
      if (!this.open) return;
      const preview = await preferences.previewUserImport(json);
      if (!this.open) { preferences.cancelUserImport(); return; }
      this.#importPreview = preview;
      this.#render();
    } catch (error) {
      this.#render();
      this.#elements.status.textContent = `Import failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      this.#elements.importUser.value = "";
      this.#elements.importUser.disabled = false;
    }
  }

  #renderImport(preferences: Preferences, preview: UserImportPreview): void {
    this.#unmountEditor?.();
    this.#unmountEditor = null;
    this.#elements.resetAll.hidden = true;
    this.#elements.resetCategory.hidden = true;
    const section = document.createElement("section");
    const title = document.createElement("h3");
    title.textContent = "Preview User preferences replacement";
    const explanation = document.createElement("p");
    explanation.textContent = "Replace all User overrides, including entries retained for another platform. Missing entries reset to defaults. Device settings and character key bindings stay unchanged.";
    const changes = document.createElement("ul");
    changes.append(...preview.changes.map((change) => {
      const item = document.createElement("li");
      const entry = preferences.presentation().find((entry) => entry.id === change.id);
      const label = entry ? text(entry.metadata.label_key) : change.id;
      item.textContent = `${change.operation}: ${label} — ${change.before.value} → ${change.after.value} (${change.applyMode})`;
      return item;
    }));
    const diagnostics = document.createElement("pre");
    diagnostics.className = "settings-import-diagnostics";
    diagnostics.textContent = preview.diagnostics.join("\n");
    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.textContent = "Replace User preferences";
    confirm.addEventListener("click", () => {
      try {
        preferences.confirmUserImport(preview);
        this.#importPreview = null;
        this.#render();
        this.#elements.status.textContent = "User preferences replaced and saved.";
      } catch (error) {
        this.#elements.status.textContent = `Import was not completed: ${error instanceof Error ? error.message : String(error)}`;
      }
    });
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = "Cancel import";
    cancel.addEventListener("click", () => { preferences.cancelUserImport(); this.#importPreview = null; this.#render(); });
    section.append(title, explanation, changes, diagnostics, confirm, cancel);
    this.#elements.content.replaceChildren(section);
    confirm.focus();
  }

  #resetCategory(): void {
    const preferences = this.#preferences;
    if (!preferences || !this.#category) return;
    const entries = preferences.presentation().filter((entry) => entry.metadata.category_key === this.#category);
    const changed = new Set(preferences.states().filter((state) => state.overridden).map((state) => state.id));
    const targets = entries.filter((entry) => changed.has(entry.id));
    if (targets.length === 0 || !window.confirm(`Reset ${text(this.#category)} preferences to defaults?\n\n${targets.map((entry) => text(entry.metadata.label_key)).join("\n")}\n\nKey bindings are not changed.`)) return;
    preferences.resetMany(targets.map((entry) => entry.id));
    this.#render();
  }

  #resetAll(): void {
    const preferences = this.#preferences;
    if (!preferences) {
      return;
    }
    const changed = preferences.states().filter((state) => state.overridden);
    if (changed.length === 0) {
      this.#elements.status.textContent = "All settings already use their defaults.";
      return;
    }
    const labels = changed.map((state) => {
      const entry = preferences.presentation().find((candidate) => candidate.id === state.id);
      return entry ? text(entry.metadata.label_key) : state.id;
    });
    if (!window.confirm(`Reset these settings to their defaults?\n\n${labels.join("\n")}\n\nKey bindings are not changed.`)) {
      return;
    }
    preferences.resetAll();
    this.#render();
  }
}
