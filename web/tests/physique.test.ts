import { describe, expect, test } from "bun:test";
import { DRAWN_PHYSIQUE, formatModifier, loadPendingPhysique, matchingPreset, neutralPhysique, physiqueLook, savePendingPhysique } from "../src/physique";
import { physiqueShape } from "../src/world/units/quaternius-units";
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

  test("presets shape the body through the bundle's build, jaw and height keys (#635)", () => {
    const heights = { min: 0.91, max: 1.09 };
    const shape = (name: string) => physiqueShape(physiqueLook(model, model.presets.find((preset) => preset.name === name)!.values), heights);
    expect(shape("Average")).toEqual({ build: { "build.lean": 0, "build.heavy": 0 }, jaw: { "face.broad": 0, "face.narrow": 0 }, scale: 1 });
    expect(shape("Heavyset").build["build.heavy"]).toBeCloseTo(0.7);
    expect(shape("Lean").build["build.lean"]).toBeCloseTo(0.6);
    expect(shape("Stocky").scale).toBeCloseTo(0.973);
    const jaw = model.params.map((param) => (param.key === "jawWidth" ? 0 : model.neutral));
    expect(physiqueShape(physiqueLook(model, jaw), heights).jaw).toEqual({ "face.broad": 0, "face.narrow": 1 });
    // Only sliders with real shapes are enabled; the rest stay off.
    expect(DRAWN_PHYSIQUE).toEqual(["bodyFat", "height", "jawWidth"]);
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
