/**
 * The Ranger's clips as per-family animation files (#898, asset-tooling #733): the download index's
 * `clips.families` lists each family's file and its clips with their lengths. A figure loads the
 * `base` family with its model and every other family on the first use of one of its clips; until
 * then the clip stands in with a base clip, so a unit never drops to its bind pose.
 */

/** One family's animation file and the clips it holds, as the download index declares them. */
export type ClipFamily = { family: string; file: string; clips: { name: string; durationSeconds: number }[] };

/** The family every figure loads up front: locomotion, idles, jumps, grips. */
export const BASE_FAMILY = "base";

/**
 * The index's clip files: the combined library (`clips.file`, kept as a fallback until it is retired)
 * and the families. No families, or none named `base`, means the combined file alone.
 */
export function clipFamilies(index: unknown): { combined: string | null; families: ClipFamily[] } {
  const clips = (index as { clips?: { file?: unknown; families?: unknown } } | null)?.clips;
  const combined = typeof clips?.file === "string" ? clips.file : null;
  const families = (Array.isArray(clips?.families) ? clips.families as Partial<ClipFamily>[] : [])
    .filter((family): family is ClipFamily => typeof family.family === "string" && typeof family.file === "string"
      && Array.isArray(family.clips));
  return { combined, families: families.some((family) => family.family === BASE_FAMILY) ? families : [] };
}

/** Families whose clips are weapon work: they stand in with the base sword stance. */
const ARMED = new Set(["strike", "combat", "sword", "dagger", "ready", "defend", "hit", "guard"]);

/** The base clip a clip of `family` plays while its family loads. */
export function standIn(family: string): string {
  return ARMED.has(family) ? "Sword_Idle" : "Idle_Loop";
}

/**
 * A figure's clips by name: the loaded ones, and every other family fetched with `load` on its first
 * request. `get` answers a clip still loading (or whose family failed) with its {@link standIn}.
 */
export class FamilyClips<Clip> {
  readonly #loaded: Map<string, Clip>;
  readonly #families = new Map<string, ClipFamily>();
  /** Families requested once: loading, loaded or failed. */
  readonly #requested = new Set<string>();
  readonly #load: (family: ClipFamily) => Promise<ReadonlyMap<string, Clip>>;
  readonly #durations: Map<string, number>;

  /** `loaded` is the base family's clips (or the combined file's), `durations` their lengths. */
  constructor(loaded: Map<string, Clip>, durations: ReadonlyMap<string, number>, families: readonly ClipFamily[],
    load: (family: ClipFamily) => Promise<ReadonlyMap<string, Clip>>) {
    this.#loaded = loaded;
    this.#load = load;
    this.#durations = new Map(durations);
    for (const family of families) {
      if (family.family === BASE_FAMILY) { continue; }
      for (const clip of family.clips) {
        if (this.#loaded.has(clip.name)) { continue; }
        this.#families.set(clip.name, family);
        this.#durations.set(clip.name, clip.durationSeconds);
      }
    }
  }

  /** Every clip the figure can play, loaded or not, with its length in seconds. */
  durations(): ReadonlyMap<string, number> {
    return this.#durations;
  }

  /** Whether `name` is loaded, not merely standing in. */
  loaded(name: string): boolean {
    return this.#loaded.has(name);
  }

  /** Clip `name`, starting its family's download on first use and standing in until it lands. */
  get(name: string): Clip | undefined {
    const clip = this.#loaded.get(name);
    if (clip !== undefined) { return clip; }
    const family = this.#families.get(name);
    if (!family) { return undefined; }
    if (!this.#requested.has(family.family)) {
      this.#requested.add(family.family);
      this.#load(family).then((clips) => {
        for (const [clip, value] of clips) { this.#loaded.set(clip, value); }
      }, (error: unknown) => {
        console.warn(`The Ranger's ${family.family} clips failed to load; they keep standing in.`, error);
      });
    }
    return this.#loaded.get(standIn(family.family));
  }
}
