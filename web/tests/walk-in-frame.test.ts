import { expect, test } from "bun:test";
import { PER_FRAME, walkInFrames } from "./support/work/walk-in-frame";

// The `walk-in-frame` budget (#1286, `.performance/contract.json`): a frame asks the shared walk-in presenter each of
// its questions once for every building, so its calls into WASM do not grow with the buildings.
for (const frames of [1, 8, 32]) {
  test(`${frames} walk-in frames ask the shared presenter ${PER_FRAME.length} times each`, async () => {
    const { buildings, largestFrameCalls } = await walkInFrames(frames);
    expect(buildings).toBeGreaterThan(PER_FRAME.length);
    expect(largestFrameCalls).toBe(PER_FRAME.length);
  });
}
