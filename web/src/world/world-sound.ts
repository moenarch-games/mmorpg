import type { GameAudioHandle, Vec3 } from "@moritzbrantner/game-audio";

/**
 * Combat sounds in the browser (#447): the shared `game-audio` director on Web Audio, playing the
 * CC0 `combat-sounds` bundle. The cues and fades come from the shared presenter
 * (`mmorpg_presentation::audio`, the same mapping the native client plays), so this adapter only
 * forwards them, places the listener at the camera and applies the volume preferences. The
 * package unlocks audio on the first gesture and suspends it while the page is hidden; if Web
 * Audio, the wasm or the bundle is unavailable every call is silent and the visuals are unchanged.
 */
export const SOUND_BUNDLE = `${import.meta.env.BASE_URL}sounds/combat-sounds/`;

/** One operation of the presenter's `takeAudio()` JSON. */
export type SoundOp =
  | { play: { key: string; kind: string; entity: number; position: [number, number, number] } }
  | { stop: { entity: number } };

export type AudioPreferences = { masterVolume: number; effectsVolume: number; uiVolume: number; muted: boolean };

type BundleManifest = { assets: { key: string; path: string }[] };

export class WorldSound {
  readonly #audio: GameAudioHandle;

  constructor(audio: GameAudioHandle) {
    this.#audio = audio;
  }

  /**
   * Loads the bundle's SoundManifest, lets `compose` add mmorpg's cues (the wasm
   * `soundManifest`) and opens the audio handle. Resolves to null when the bundle is missing.
   */
  static async open(compose: (bundle: string) => string, fetcher: typeof fetch = fetch): Promise<WorldSound | null> {
    try {
      const bundle = await (await fetcher(`${SOUND_BUNDLE}manifest.json`)).json() as BundleManifest;
      const entry = bundle.assets.find((asset) => asset.key === "combat-sounds.sound-manifest");
      if (!entry) { throw new Error("The combat-sounds bundle has no SoundManifest."); }
      const manifest = compose(await (await fetcher(`${SOUND_BUNDLE}${entry.path}`)).text());
      const { createGameAudio } = await import("@moritzbrantner/game-audio");
      return new WorldSound(await createGameAudio({ manifest, baseUrl: SOUND_BUNDLE }));
    } catch (error) {
      console.warn("Sound is unavailable", error);
      return null;
    }
  }

  /** Plays and fades the operations of the projections just pushed to the presenter. */
  apply(json: string): void {
    for (const op of JSON.parse(json) as SoundOp[]) {
      if ("play" in op) {
        const { key, kind, entity, position } = op.play;
        this.#audio.trigger({ key, kind, entity, position });
      } else {
        this.#audio.stopEntity(op.stop.entity);
      }
    }
  }

  /** A new connection: every voice fades; cue keys already heard stay silent. */
  reset(): void {
    this.#audio.reset();
  }

  /** The listener at the camera eye, with its right ear along the camera's right axis. */
  listen(eye: Vec3, right: Vec3): void {
    this.#audio.setListener(eye, right);
  }

  /** The catalog's audio settings through game-audio's settings mapping (percent volumes). */
  setPreferences(preferences: AudioPreferences): void {
    this.#audio.applySetting("audio.master_volume", preferences.masterVolume, true);
    this.#audio.applySetting("audio.effects_volume", preferences.effectsVolume, true);
    this.#audio.applySetting("audio.ui_volume", preferences.uiVolume, true);
    this.#audio.applySetting("audio.muted", preferences.muted);
  }

  /** Once per frame, so finished voices free their slots. */
  pump(): void {
    this.#audio.pump();
  }
}
