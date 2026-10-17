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
  reduceMotion: "accessibility.reduce_motion",
} as const;

export const MOTION_CHOICES = ["system", "reduce", "full"] as const;
export type MotionPreference = (typeof MOTION_CHOICES)[number];

export const SETTINGS_DEFINITIONS: readonly SettingDefinition[] = [
  {
    id: SETTING.fieldOfView,
    kind: { type: "integer", min: 40, max: 90 },
    default: { type: "integer", value: 50 },
    scope: "user",
    apply_mode: "immediate",
  },
  {
    id: SETTING.lookSensitivity,
    kind: { type: "number", min: 0.25, max: 3 },
    default: { type: "number", value: 1 },
    scope: "user",
    apply_mode: "immediate",
  },
  {
    id: SETTING.reduceMotion,
    kind: { type: "choice", options: [...MOTION_CHOICES] },
    default: { type: "choice", value: "system" },
    scope: "user",
    apply_mode: "immediate",
  },
];

export const SETTINGS_PRESENTATION: readonly PresentationEntry[] = [
  {
    id: SETTING.fieldOfView,
    metadata: {
      label_key: "settings.camera.field_of_view.label",
      description_key: "settings.camera.field_of_view.description",
      category_key: "settings.category.camera",
      order: 10,
      discoverability: "primary",
      search_keys: ["settings.search.fov"],
    },
  },
  {
    id: SETTING.lookSensitivity,
    metadata: {
      label_key: "settings.controls.look_sensitivity.label",
      description_key: "settings.controls.look_sensitivity.description",
      category_key: "settings.category.controls",
      order: 10,
      discoverability: "primary",
      search_keys: ["settings.search.mouse"],
    },
  },
  {
    id: SETTING.reduceMotion,
    metadata: {
      label_key: "settings.accessibility.reduce_motion.label",
      description_key: "settings.accessibility.reduce_motion.description",
      category_key: "settings.category.accessibility",
      order: 10,
      discoverability: "primary",
      search_keys: ["settings.search.animation"],
    },
  },
];

/** English strings for the catalog's localization keys. */
export const SETTINGS_TEXT: Readonly<Record<string, string>> = {
  "settings.category.camera": "Camera",
  "settings.category.controls": "Controls",
  "settings.category.accessibility": "Accessibility",
  "settings.camera.field_of_view.label": "Field of view (degrees)",
  "settings.camera.field_of_view.description": "Vertical field of view of the world camera.",
  "settings.controls.look_sensitivity.label": "Look sensitivity",
  "settings.controls.look_sensitivity.description": "How far a mouse drag turns the camera (1 is the default).",
  "settings.accessibility.reduce_motion.label": "Reduce motion",
  "settings.accessibility.reduce_motion.description": "Calms camera zoom, idle animation and drifting clouds. System follows the browser setting.",
  "settings.choice.system": "Follow system",
  "settings.choice.reduce": "Reduce",
  "settings.choice.full": "Full motion",
};

/** The effective values the client applies. */
export type ClientPreferences = {
  fieldOfViewDegrees: number;
  lookSensitivity: number;
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
  return {
    fieldOfViewDegrees: numberValue(values[SETTING.fieldOfView], 50),
    lookSensitivity: numberValue(values[SETTING.lookSensitivity], 1),
    reduceMotion: motion?.type === "choice" && isMotionPreference(motion.value) ? motion.value : "system",
  };
}

/**
 * The settings session plus its local `User` snapshot (#41 Q4: local storage, no account sync).
 * Every change is persisted after it commits; a storage failure is reported, never hidden.
 */
export class Preferences {
  readonly #session: BrowserSettingsSession;
  readonly #storage: PreferenceStorage | null;
  readonly #listeners = new Set<(preferences: ClientPreferences) => void>();
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

  resetAll(): void {
    this.#session.resetAll();
    this.#committed();
  }

  /** Calls `listener` now and after every committed change. */
  subscribe(listener: (preferences: ClientPreferences) => void): () => void {
    this.#listeners.add(listener);
    listener(this.values());
    return () => this.#listeners.delete(listener);
  }

  #committed(): void {
    try {
      this.#storage?.setItem(USER_SNAPSHOT_KEY, this.#session.exportScope("user"));
      this.problem = null;
    } catch (error) {
      this.problem = `Settings apply now but could not be saved: ${error instanceof Error ? error.message : String(error)}`;
    }
    const values = this.values();
    for (const listener of this.#listeners) {
      listener(values);
    }
  }
}
