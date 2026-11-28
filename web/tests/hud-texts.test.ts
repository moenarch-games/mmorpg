import { expect, test } from "bun:test";
import { panelRefusal } from "../src/world/units/hud-texts";
import { hudTextReadsPerProjection } from "./support/work/hud-texts";

// The `hud-texts` operation of `.performance/contract.json` (#1278): one WASM read per projection, however many frames
// show it. `support/work/hud-texts.ts` measures it; `scripts/work-evidence.ts` emits the same measurement.
for (const frames of [1, 4, 16]) {
  test(`${frames} frames of a projection read its HUD texts once`, () => {
    expect(hudTextReadsPerProjection(frames)).toBe(1);
  });
}

test("panels word refusals in their own context and ignore the others' (#1278)", () => {
  expect(panelRefusal("inventory-full", "loot")).toBe("Your bags are full. The corpse keeps its rewards.");
  expect(panelRefusal("too-many-intents", "bag")).toBe("Too many actions. Try the bag move again.");
  expect(panelRefusal("not-ready", "loot")).toBeNull();
});
