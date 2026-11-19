import { describe, expect, test } from "bun:test";
import { clipFamilies, FamilyClips } from "../src/world/units/clip-families";

const index = {
  clips: {
    file: "ranger.clips.glb",
    families: [
      { family: "base", file: "ranger.clips.base.glb", clips: [{ name: "Idle_Loop", durationSeconds: 2.5 }, { name: "Sword_Idle", durationSeconds: 1.6 }] },
      { family: "strike", file: "ranger.clips.strike.glb", clips: [{ name: "Sword_Regular_A", durationSeconds: 1.2 }] },
      { family: "chores", file: "ranger.clips.chores.glb", clips: [{ name: "Sweep_Loop", durationSeconds: 3 }] },
    ],
  },
};

describe("Ranger clip families (#898)", () => {
  test("the base family loads up front and the rest on first use, standing in until then", async () => {
    const { families } = clipFamilies(index);
    const fetched: string[] = [];
    let land: () => void = () => {};
    const landed = new Promise<void>((resolve) => { land = resolve; });
    const clips = new FamilyClips(new Map([["Idle_Loop", "idle"], ["Sword_Idle", "stance"]]), new Map([["Idle_Loop", 2.5], ["Sword_Idle", 1.6]]),
      families, async (family) => {
        fetched.push(family.file);
        await landed;
        return new Map(family.clips.map((clip) => [clip.name, clip.name.toLowerCase()]));
      });

    // Every clip is known with its length before its family loads.
    expect([...clips.durations()]).toContainEqual(["Sword_Regular_A", 1.2]);
    expect(fetched).toEqual([]);

    // A strike stands in with the sword stance, a chore with the idle; each family is fetched once.
    expect(clips.get("Sword_Regular_A")).toBe("stance");
    expect(clips.get("Sword_Regular_A")).toBe("stance");
    expect(clips.get("Sweep_Loop")).toBe("idle");
    expect(fetched).toEqual(["ranger.clips.strike.glb", "ranger.clips.chores.glb"]);

    land();
    await landed;
    await Promise.resolve();
    expect(clips.get("Sword_Regular_A")).toBe("sword_regular_a");
    expect(clips.loaded("Sweep_Loop")).toBe(true);
    expect(clips.get("Unknown")).toBeUndefined();
  });

  test("a release without a base family falls back to the combined file", () => {
    expect(clipFamilies({ clips: { file: "ranger.clips.glb" } })).toEqual({ combined: "ranger.clips.glb", families: [] });
    expect(clipFamilies({ clips: { file: "ranger.clips.glb", families: [index.clips.families[1]] } }).families).toEqual([]);
  });
});
