import { describe, expect, test } from "bun:test";
import { lookItems } from "../src/world/units/quaternius-units";

// The picker's selected look loads only what it wears before optional art (#979).
describe("selected look items", () => {
  test("a look asks for its loadout, headwear and hairstyle only", () => {
    expect(lookItems("bow-and-quiver", "ranger-cap", { style: "long", color: "brown" }))
      .toEqual(["equipment.bow.long", "equipment.quiver.back", "equipment.cap.ranger", "hair.long"]);
    expect(lookItems(null, "wayfarer-hood", { style: "bald", color: "black" })).toEqual([]);
  });

  test("the dagger brings the throwables it draws from the belt", () => {
    const items = lookItems("dagger.twin", null, null);
    expect(items[0]).toBe("equipment.dagger.blackened");
    expect(items.length).toBeGreaterThan(1);
    expect(new Set(items).size).toBe(items.length);
  });
});
