import type { SettingState, SettingValue } from "@moritzbrantner/settings-browser";
import type { Profile } from "@moritzbrantner/input-bindings";
import { SETTINGS_TEXT, type Preferences } from "./preferences";

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
  content: HTMLElement;
  status: HTMLElement;
  resetAll: HTMLButtonElement;
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
  #unmountEditor: (() => void) | null = null;

  constructor(elements: SettingsPanelElements, options: SettingsPanelOptions) {
    this.#elements = elements;
    this.#options = options;
    elements.close.addEventListener("click", () => this.close());
    elements.resetAll.addEventListener("click", () => this.#resetAll());
    // Escape (cancel) and form-method closes all come through `close`.
    elements.dialog.addEventListener("close", () => this.#closed());
    options.preferences.then((preferences) => {
      this.#preferences = preferences;
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
    const ordered = [...preferences.presentation()].sort((left, right) => (left.metadata.order ?? 0) - (right.metadata.order ?? 0));
    const categories = [...new Set(ordered.map((entry) => entry.metadata.category_key))];
    // Fixed order: Camera, Controls, Accessibility, then anything else.
    const preferred = ["settings.category.camera", CONTROLS_CATEGORY, "settings.category.accessibility"];
    return [...preferred.filter((key) => categories.includes(key)), ...categories.filter((key) => !preferred.includes(key))];
  }

  #render(): void {
    const preferences = this.#preferences;
    const { categories, content, status } = this.#elements;
    if (!preferences) {
      status.textContent = "Loading settings…";
      return;
    }
    status.textContent = preferences.problem ?? "";
    const keys = this.#categories();
    this.#category ??= keys[0] ?? null;
    categories.replaceChildren(...keys.map((key) => {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = text(key);
      button.setAttribute("aria-current", String(key === this.#category));
      button.addEventListener("click", () => {
        this.#category = key;
        this.#render();
        button.focus();
      });
      return button;
    }));
    const currentStates = preferences.states();
    this.#elements.resetAll.hidden = !currentStates.some((state) => state.overridden);
    const states = new Map(currentStates.map((state) => [state.id, state]));
    const rows = preferences.presentation()
      .filter((entry) => entry.metadata.category_key === this.#category)
      .sort((left, right) => (left.metadata.order ?? 0) - (right.metadata.order ?? 0))
      .flatMap((entry) => {
        const state = states.get(entry.id);
        return state && state.availability.status !== "hidden" ? [this.#row(preferences, entry.id, entry.metadata.label_key, entry.metadata.description_key, state)] : [];
      });
    this.#unmountEditor?.();
    this.#unmountEditor = null;
    content.replaceChildren(...rows);
    if (this.#category === CONTROLS_CATEGORY) {
      content.append(this.#bindingsSection());
    }
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
