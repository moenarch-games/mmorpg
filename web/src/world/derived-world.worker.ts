import init, * as generated from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import type { DerivedWorldMessage, DerivedWorldRequest } from "./derived-world";

/**
 * Derives the presented world's scenery, static scene and grass away from the page (#855), in this worker's own
 * instance of the same `mmorpg-wasm` build, so every value equals the page's own derivation. The page sends the compiled
 * module first, then the cached terrain grids (or null); the content and scenery build in between. Answers four times:
 * the zone content's costly base parts first (`hostedContentSeed`, which the page seeds instead of building them), then
 * the terrain grids and scenery export, then the static scene's layout, buffers and skeleton (with the terrain's overlay
 * weights), then the grass's seed. The page ends the worker after the fourth.
 */
const scope = self as unknown as {
  onmessage: ((event: MessageEvent<DerivedWorldRequest>) => void) | null;
  postMessage(message: DerivedWorldMessage, transfer?: Transferable[]): void;
};

let supplyGrids: (grids: Uint8Array | null) => void = () => {};
const cachedGrids = new Promise<Uint8Array | null>((resolve) => { supplyGrids = resolve; });

scope.onmessage = ({ data }) => {
  if (data.kind === "grids") {
    supplyGrids(data.grids);
    return;
  }
  void derive(data.module).catch((error: unknown) => {
    scope.postMessage({ kind: "failed", message: error instanceof Error ? error.message : String(error) });
  });
};

async function derive(module: WebAssembly.Module): Promise<void> {
  await init({ module_or_path: module });
  const content = generated.LocalZone.hostedContentSeed();
  scope.postMessage({ kind: "content", seed: content }, [content.buffer]);
  // The rest of the content, then the presentation scenery without its grids.
  generated.reliefAt(0, 0);
  const grids = await cachedGrids;
  if (grids) { generated.seedTerrainGrids(grids); }
  const terrain = generated.terrainGridCache();
  scope.postMessage({ kind: "scenery", grids: terrain, scenery: generated.scenery() }, [terrain.buffer]);
  const vertices = generated.staticSceneVertices();
  const indices = generated.staticSceneIndices();
  const models = generated.staticSceneModels();
  const skeleton = generated.staticSceneSkeleton();
  scope.postMessage(
    { kind: "scene", layout: generated.staticScene(), vertices, indices, models, skeleton },
    [vertices.buffer, indices.buffer, models.buffer, skeleton.buffer],
  );
  const grass = generated.grassSeed();
  scope.postMessage({ kind: "grass", grass }, [grass.buffer]);
}
