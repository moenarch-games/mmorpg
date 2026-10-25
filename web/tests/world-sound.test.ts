import { describe, expect, test } from "bun:test";
import type { GameAudioHandle } from "@moritzbrantner/game-audio";
import { WorldSound } from "../src/world/world-sound";

function recording(): { handle: GameAudioHandle; calls: unknown[][] } {
  const calls: unknown[][] = [];
  const record = (name: string) => (...args: unknown[]) => { calls.push([name, ...args]); return undefined as never; };
  const handle = new Proxy({}, { get: (_, name) => record(String(name)) }) as GameAudioHandle;
  return { handle, calls };
}

describe("combat sounds (#447)", () => {
  test("the presenter's plays and fades reach the game-audio handle", () => {
    const { handle, calls } = recording();
    const sound = new WorldSound(handle);
    sound.apply(JSON.stringify([
      { play: { key: "1/10/0", kind: "combat.hit", entity: 8589934599, position: [7, 1, 0] } },
      { stop: { entity: 8589934599 } },
    ]));
    expect(calls).toEqual([
      ["trigger", { key: "1/10/0", kind: "combat.hit", entity: 8589934599, position: [7, 1, 0] }],
      ["stopEntity", 8589934599],
    ]);
  });

  test("volume preferences go through game-audio's settings mapping", () => {
    const { handle, calls } = recording();
    new WorldSound(handle).setPreferences({ masterVolume: 80, effectsVolume: 100, uiVolume: 50, muted: true });
    expect(calls).toEqual([
      ["applySetting", "audio.master_volume", 80, true],
      ["applySetting", "audio.effects_volume", 100, true],
      ["applySetting", "audio.ui_volume", 50, true],
      ["applySetting", "audio.muted", true],
    ]);
  });

  test("a missing bundle leaves play silent", async () => {
    const missing = (async () => new Response("not found", { status: 404 })) as unknown as typeof fetch;
    expect(await WorldSound.open((bundle) => bundle, missing)).toBeNull();
  });
});
