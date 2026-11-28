import type { WalkInSetHandle } from "../../../src/world/scenery";
import { WalkInBuildings } from "../../../src/world/walk-in-buildings";
import { greyhavenScenery } from "./scenery-frame";

// The `walk-in-frame` measurement (#1286): `web/tests/walk-in-frame.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. A frame asks the shared walk-in presenter (every
// building at once) a fixed handful of questions — parts, camera, interior look, lamps, rooms stood in — instead
// of six calls per building into WASM.

/** The per-frame questions the world view asks, in its order. */
const PER_FRAME = ["frame", "cottagesInside", "camera", "lighting", "pointLights"] as const;

/** `set` with every call into it counted. */
function counted(set: WalkInSetHandle): { set: WalkInSetHandle; calls: () => number } {
  let calls = 0;
  const wrapped = new Proxy(set, {
    get(target, key, receiver) {
      const value = Reflect.get(target, key, receiver) as unknown;
      if (typeof value !== "function") { return value; }
      return (...args: unknown[]) => { calls += 1; return (value as (...a: unknown[]) => unknown).apply(target, args); };
    },
  });
  return { set: wrapped, calls: () => calls };
}

/**
 * `frames` frames of the Greyhaven walk-ins as the world view drives them, standing at the inn's door: the buildings
 * presented and the largest number of calls into the shared presenter in one frame.
 */
export async function walkInFrames(frames: number): Promise<{ buildings: number; largestFrameCalls: number }> {
  const { world } = await greyhavenScenery();
  const source = world.buildings.set();
  if (!source) { throw new Error("Greyhaven has walk-in buildings"); }
  const { set, calls } = counted(source);
  const buildings = new WalkInBuildings(set);
  const feet = [0, 0.1, 0] as const;
  let largestFrameCalls = 0;
  for (let index = 0; index < frames; index += 1) {
    const before = calls();
    buildings.update([], feet, 1 / 60, false);
    buildings.cottagesInside(feet);
    buildings.camera({ eye: [0, 3, 6], target: [0, 1, 0] }, feet, 1 / 60, false);
    buildings.interior(feet);
    buildings.pointLights(new Float32Array());
    largestFrameCalls = Math.max(largestFrameCalls, calls() - before);
  }
  const count = source.count();
  buildings.dispose();
  return { buildings: count, largestFrameCalls };
}

export { PER_FRAME };
