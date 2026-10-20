import catalog from "../../../content/settings/catalog.json";
import type {
  BrowserSettingsSession,
  PresentationEntry,
  SettingDefinition,
  SettingState,
  SettingValue,
} from "@moritzbrantner/settings-browser";

/**
 * MMORPG's browser settings catalog over the shared `settings` foundation. The session owns
 * validation, overrides, reset and the portable `User` snapshot; the client only applies the
 * effective values (camera, look, motion). Every entry is a real client capability; nothing here
 * reaches zone authority. Key bindings are not settings values: `input-bindings` owns them.
 */
export const SETTING = {
  fieldOfView: "camera.field_of_view",
  lookSensitivity: "controls.look_sensitivity",
  invertVerticalLook: "controls.invert_vertical_look",
  reduceMotion: "accessibility.reduce_motion",
} as const;

export const MOTION_CHOICES = ["system", "reduce", "full"] as const;
export type MotionPreference = (typeof MOTION_CHOICES)[number];

if (catalog.schema_version !== 1) throw new Error("Unsupported MMORPG settings catalog");
const browserEntries = catalog.entries.filter((entry) => entry.target === "shared" || entry.target === "browser");

// The checked-in catalog is shared with native's settings-core adapter. The
// foundation validates definitions when opening a session; this assertion only
// bridges JSON's widened string types to its public discriminated unions.
export const SETTINGS_DEFINITIONS = browserEntries.map((entry) => entry.definition) as readonly SettingDefinition[];
export const SETTINGS_PRESENTATION = browserEntries.map((entry) => entry.presentation) as readonly PresentationEntry[];

/** English strings for the catalog's localization keys. */
export const SETTINGS_TEXT: Readonly<Record<string, string>> = catalog.text;

/** Consumer category order and platform filtering share the canonical manifest. */
export function settingCategories(presentation: readonly PresentationEntry[]): string[] {
  const applicable = new Set(presentation.map((entry) => entry.metadata.category_key));
  return [...catalog.categories].sort((left, right) => left.order - right.order || (left.key < right.key ? -1 : left.key > right.key ? 1 : 0))
    .filter((category) => applicable.has(category.key)).map((category) => category.key);
}

/** The effective values the client applies. */
export type ClientPreferences = {
  fieldOfViewDegrees: number;
  lookSensitivity: number;
  invertVerticalLook: boolean;
  reduceMotion: MotionPreference;
};

export type PreferenceStorage = Pick<Storage, "getItem" | "setItem">;

const USER_SNAPSHOT_KEY = "mmorpg.settings.user.v1";

function numberValue(value: SettingValue | undefined, fallback: number): number {
  return value && (value.type === "integer" || value.type === "number") ? value.value : fallback;
}

function isMotionPreference(value: unknown): value is MotionPreference {
  return (MOTION_CHOICES as readonly unknown[]).includes(value);
}

export function clientPreferences(values: Readonly<Record<string, SettingValue>>): ClientPreferences {
  const motion = values[SETTING.reduceMotion];
  const inverted = values[SETTING.invertVerticalLook];
  return {
    fieldOfViewDegrees: numberValue(values[SETTING.fieldOfView], 50),
    lookSensitivity: numberValue(values[SETTING.lookSensitivity], 1),
    invertVerticalLook: inverted?.type === "bool" && inverted.value === true,
    reduceMotion: motion?.type === "choice" && isMotionPreference(motion.value) ? motion.value : "system",
  };
}

export type UserPreferenceChange = {
  readonly id: string;
  readonly before: SettingValue;
  readonly after: SettingValue;
  readonly operation: "Add override" | "Change override" | "Reset to default";
  readonly applyMode: SettingDefinition["apply_mode"];
};
export type UserImportPreview = {
  readonly changes: readonly UserPreferenceChange[];
  readonly diagnostics: readonly string[];
};

/**
 * The settings session plus its local `User` snapshot (#41 Q4: local storage, no account sync).
 * Every change is persisted after it commits; a storage failure is reported, never hidden.
 */
export class Preferences {
  readonly #session: BrowserSettingsSession;
  readonly #storage: PreferenceStorage | null;
  readonly #listeners = new Set<(preferences: ClientPreferences) => void>();
  #pendingImport: { preview: UserImportPreview; candidate: BrowserSettingsSession; baseline: string } | null = null;
  #importGeneration = 0;
  #lastPublished: ClientPreferences | null = null;
  /** Why the stored snapshot was not loaded, or the last save failed. */
  problem: string | null = null;

  private constructor(session: BrowserSettingsSession, storage: PreferenceStorage | null) {
    this.#session = session;
    this.#storage = storage;
  }

  static async open(storage: PreferenceStorage | null): Promise<Preferences> {
    const { createSettingsSession } = await import("@moritzbrantner/settings-browser");
    const preferences = new Preferences(await createSettingsSession(SETTINGS_DEFINITIONS, SETTINGS_PRESENTATION), storage);
    preferences.#load();
    return preferences;
  }

  #load(): void {
    let snapshot: string | null;
    try {
      snapshot = this.#storage?.getItem(USER_SNAPSHOT_KEY) ?? null;
    } catch {
      this.problem = "Saved settings could not be read; using defaults.";
      return;
    }
    if (snapshot === null) {
      return;
    }
    try {
      const diagnostics = this.#session.importScope("user", snapshot);
      if (diagnostics.length > 0) {
        console.warn("Settings snapshot diagnostics", diagnostics);
      }
    } catch (error) {
      this.problem = `Saved settings are invalid and were ignored: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  values(): ClientPreferences {
    return clientPreferences(this.#session.effectiveValues());
  }

  states(): SettingState[] {
    return this.#session.settingStates();
  }

  presentation(): PresentationEntry[] {
    return this.#session.presentation();
  }

  definition(id: string): SettingDefinition | undefined {
    return SETTINGS_DEFINITIONS.find((definition) => definition.id === id);
  }

  /** Commits one value (validated by the session); throws on an invalid value. */
  set(id: string, value: SettingValue): void {
    this.#session.set(id, value);
    this.#committed();
  }

  reset(id: string): void {
    this.#session.reset(id);
    this.#committed();
  }

  resetMany(ids: readonly string[]): void {
    this.#session.resetMany(ids);
    this.#committed();
  }

  resetAll(): void {
    this.#session.resetAll();
    this.#committed();
  }

  exportUserProfile(): string {
    return this.#session.exportScope("user");
  }

  /** Parse/migrate in an isolated canonical session; live and stored values stay unchanged. */
  async previewUserImport(json: string): Promise<UserImportPreview> {
    this.cancelUserImport();
    const generation = this.#importGeneration;
    const baseline = this.exportUserProfile();
    const { createSettingsSession } = await import("@moritzbrantner/settings-browser");
    const candidate = await createSettingsSession(SETTINGS_DEFINITIONS, SETTINGS_PRESENTATION);
    try {
      const diagnostics = candidate.importScope("user", json);
      if (generation !== this.#importGeneration || this.exportUserProfile() !== baseline) {
        throw new Error("Preferences changed or import was cancelled; preview the file again.");
      }
      const before = new Map(this.states().map((state) => [state.id, state]));
      const changes: UserPreferenceChange[] = candidate.settingStates().flatMap((after) => {
        const definition = this.definition(after.id);
        const previous = before.get(after.id);
        if (definition?.scope !== "user" || !previous ||
          (JSON.stringify(previous.value) === JSON.stringify(after.value) && previous.overridden === after.overridden)) return [];
        return [{ id: after.id, before: previous.value, after: after.value,
          operation: !after.overridden ? "Reset to default" : previous.overridden ? "Change override" : "Add override",
          applyMode: definition.apply_mode }];
      });
      const preview: UserImportPreview = { changes, diagnostics };
      this.#pendingImport = { preview, candidate, baseline };
      return preview;
    } catch (error) {
      candidate.dispose();
      throw error;
    }
  }

  /** Persist once before replacing live User; Device, bindings and other scopes stay intact. */
  confirmUserImport(preview: UserImportPreview): void {
    const pending = this.#pendingImport;
    if (!pending || pending.preview !== preview || pending.baseline !== this.exportUserProfile()) {
      throw new Error("User preferences changed; preview the import again.");
    }
    if (!this.#storage) throw new Error("User preferences cannot be saved on this device.");
    const snapshot = pending.candidate.exportScope("user");
    this.#storage.setItem(USER_SNAPSHOT_KEY, snapshot);
    this.#session.importScope("user", snapshot);
    this.cancelUserImport();
    this.problem = null;
    this.#notify();
  }

  cancelUserImport(): void {
    this.#importGeneration += 1;
    this.#pendingImport?.candidate.dispose();
    this.#pendingImport = null;
  }

  /** Calls `listener` now and after every committed change. */
  subscribe(listener: (preferences: ClientPreferences) => void): () => void {
    this.#listeners.add(listener);
    const values = this.values();
    this.#lastPublished = values;
    listener(values);
    return () => this.#listeners.delete(listener);
  }

  #committed(): void {
    try {
      this.#storage?.setItem(USER_SNAPSHOT_KEY, this.#session.exportScope("user"));
      this.problem = null;
    } catch (error) {
      this.problem = `Settings apply now but could not be saved: ${error instanceof Error ? error.message : String(error)}`;
    }
    this.#notify();
  }

  #notify(): void {
    const values = this.values();
    if (this.#lastPublished && JSON.stringify(this.#lastPublished) === JSON.stringify(values)) return;
    this.#lastPublished = values;
    for (const listener of this.#listeners) {
      listener(values);
    }
  }
}
