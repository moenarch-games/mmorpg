import { describe, expect, test } from "bun:test";
import { formatModifier, loadPendingPhysique, matchingPreset, neutralPhysique, savePendingPhysique } from "../src/physique";
import { committedPhysique, customizePhysique, physiqueModel, physiqueModifiers } from "../src/world/wasm-runtime";
import { createLocalWorld } from "../src/world/local-world";
import { MemoryStorage, testCharacters } from "./support/local-characters";
import { localZoneModule } from "./support/local-zone-module";

// The picker's physique (#634): every number is mmorpg-core's, through mmorpg-wasm.
describe("physique picker", () => {
  const model = physiqueModel();

  test("the model carries core's sliders, presets and ±2% bound", () => {
    expect(model.params.map((param) => param.key)).toHaveLength(11);
    expect(model.params.filter((param) => param.affectsStats)).toHaveLength(5);
    expect(model.presets.map((preset) => preset.name)).toEqual(["Average", "Lean", "Athletic", "Muscular", "Stocky", "Heavyset"]);
    expect(model.maxModifierBp).toBe(200);
    expect(matchingPreset(model, neutralPhysique(model))).toBe("Average");
  });

  test("the preview is core's clamped modifier; cosmetic sliders change nothing", () => {
    expect(physiqueModifiers(neutralPhysique(model))).toEqual([0, 0]);
    const muscular = model.presets.find((preset) => preset.name === "Muscular")!.values;
    expect(physiqueModifiers(muscular)).toEqual([40, 145]);
    const extreme = model.params.map((param) => (param.affectsStats ? model.scale : model.neutral));
    expect(physiqueModifiers(extreme)).toEqual([200, 200]);
    const cosmetic = muscular.map((value, index) => (model.params[index]!.affectsStats ? value : 0));
    expect(physiqueModifiers(cosmetic)).toEqual([40, 145]);
    expect(formatModifier(145)).toBe("+1.45%");
    expect(formatModifier(-50)).toBe("−0.50%");
    expect(() => physiqueModifiers([1, 2, 3])).toThrow();
  });

  test("confirmed sliders create a new character, then edit and reload its record", async () => {
    const characters = testCharacters();
    const lean = model.presets.find((preset) => preset.name === "Lean")!.values;
    expect(await customizePhysique(characters, "local-1", lean)).toBe(false);
    const pending = new MemoryStorage();
    savePendingPhysique(pending, "local-1", lean);
    expect(loadPendingPhysique(pending, model, "local-1")).toEqual(lean);

    const world = createLocalWorld(await localZoneModule(), characters);
    await world.source.join({ id: "local-1", classId: "warden", sex: "male", physique: lean });
    world.source.leave();
    expect(committedPhysique(characters, "local-1")).toEqual(lean);

    const stocky = model.presets.find((preset) => preset.name === "Stocky")!.values;
    expect(await customizePhysique(characters, "local-1", stocky)).toBe(true);
    expect(committedPhysique(characters, "local-1")).toEqual(stocky);
  });
});
