import { createLocalWorld } from "../../../src/world/local-world";
import { SceneryFrame, buildSceneryScene, type SceneryScene } from "../../../src/world/scenery-nodes";
import { testCharacters } from "../local-characters";
import { localZoneModule } from "../local-zone-module";

// The `scenery-frame` measurement (#1091, #1120): `web/tests/scene-building.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. A frame reuses each resident instance batch's shown or
// hidden renderer batch instead of copying every one of the world's instance batches again.

/** The shared Rust static world (#184) through the real WASM export, as the browser builds it. */
export async function greyhavenScenery() {
  const wasm = await localZoneModule();
  const world = createLocalWorld(wasm, testCharacters()).scenery;
  return { wasm, world, scene: buildSceneryScene(world.scenery, world.staticScene) };
}

/**
 * A first frame from one eye, then `frames` warm frames from it: the views the first frame built, the views the warm
 * frames built, and whether every warm frame handed out the first frame's batches.
 */
export function warmSceneryFrames(scene: SceneryScene, frames: number) {
  const frame = new SceneryFrame(scene);
  const eye = [0, 5, 0] as const;
  const first = frame.nodes(eye, { seconds: 0, animate: true });
  const firstViews = frame.viewsBuilt;
  let built = 0;
  let reused = true;
  for (let index = 0; index < frames; index += 1) {
    const before = frame.viewsBuilt;
    const warm = frame.nodes(eye, { seconds: index / 60, animate: true });
    built += frame.viewsBuilt - before;
    reused &&= warm.instanceBatches.every((batch, at) => batch === first.instanceBatches[at]);
  }
  return { firstViews, built, reused };
}
