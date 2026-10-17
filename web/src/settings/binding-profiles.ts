import {
  portableConfigurationFromProfile,
  profileFromPortableConfiguration,
  resolvePortableConfiguration,
  serializePortableConfiguration,
  type Binding,
  type PortableConfigurationV1,
  type Profile,
} from "@moritzbrantner/input-bindings";
import { GAME_ACTION_REGISTRY, GAME_ACTION_REGISTRY_VERSION } from "../input/game-controls";

/**
 * Per-character binding profiles (#41 Q21 B). `input-bindings` owns the profile, its portable
 * serialization and validation; MMORPG only chooses the stable character id as the storage key.
 * Nothing here enters the settings session or any server-authoritative state.
 */
export type BindingProfileStorage = Pick<Storage, "getItem" | "setItem">;

const KEY_PREFIX = "mmorpg.bindings.v1";

export function bindingProfileKey(characterId: string): string {
  return `${KEY_PREFIX}.${characterId}`;
}

function defaultBindings(): Binding[] {
  return GAME_ACTION_REGISTRY.actions.flatMap((action) => action.defaults ?? []);
}

/** A character's unmodified profile: the MMORPG defaults. */
export function defaultBindingProfile(characterId: string): Profile {
  return { id: `character:${characterId}`, patches: [] };
}

export type LoadedBindingProfile = { profile: Profile; problem: string | null };

/** Reads a character's profile; a missing one is the defaults and an unreadable one fails closed to them. */
export function loadBindingProfile(storage: BindingProfileStorage, characterId: string): LoadedBindingProfile {
  let raw: string | null;
  try {
    raw = storage.getItem(bindingProfileKey(characterId));
  } catch {
    return { profile: defaultBindingProfile(characterId), problem: "Saved key bindings could not be read; using defaults." };
  }
  if (raw === null) {
    return { profile: defaultBindingProfile(characterId), problem: null };
  }
  try {
    const configuration = JSON.parse(raw) as PortableConfigurationV1;
    const report = resolvePortableConfiguration(configuration, {
      registry: GAME_ACTION_REGISTRY,
      currentRegistryVersion: GAME_ACTION_REGISTRY_VERSION,
    });
    if (!report.valid || !report.configuration) {
      const kinds = [...new Set(report.diagnostics.map((diagnostic) => diagnostic.kind))].join(", ");
      return { profile: defaultBindingProfile(characterId), problem: `Saved key bindings are invalid (${kinds}); using defaults.` };
    }
    return { profile: profileFromPortableConfiguration(report.configuration), problem: null };
  } catch {
    return { profile: defaultBindingProfile(characterId), problem: "Saved key bindings are unreadable; using defaults." };
  }
}

/** Durably saves a character's profile; throws when it cannot be represented or stored. */
export function saveBindingProfile(storage: BindingProfileStorage, characterId: string, profile: Profile): void {
  const { configuration, diagnostics } = portableConfigurationFromProfile(
    profile,
    defaultBindings(),
    GAME_ACTION_REGISTRY_VERSION,
  );
  const errors = diagnostics.filter((diagnostic) => diagnostic.severity === "error");
  if (errors.length > 0) {
    throw new Error(`The binding profile cannot be saved (${errors.map((error) => error.kind).join(", ")}).`);
  }
  storage.setItem(bindingProfileKey(characterId), serializePortableConfiguration(configuration));
}
