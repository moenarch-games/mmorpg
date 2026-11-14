/** Offline adapter over asset-tooling's public terrain recipe; outputs are game-owned dressing data. */
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const producerRevision = "b2166ba96285ed36093e9c71aeb38907d1218635";
const source = path.resolve(process.argv[2] ?? `${process.env.HOME}/.cache/mmorpg/ranger-tools/asset-tooling-${producerRevision}`);
const root = path.resolve(import.meta.dir, "../..");
const directory = path.join(root, "content/greyhaven/terrain-patches/southmeadow");
// The acquisition cache is the immutable source archive at producerRevision, never a mutable checkout.
type Package = { exports: Record<string, unknown> };
const pkg: Package = await Bun.file(path.join(source, "package.json")).json();
const entry: unknown = pkg.exports["./recipes/terrain-patches"];
if (typeof entry !== "string" || !entry.startsWith("./src/")) {
  throw new Error("Missing public terrain-patches export");
}
type Recipe = { sampleSpacing: number };
type Patch = { origin: number[]; width: number; depth: number; slope: Uint8Array; exclusion: Uint8Array; paint: { meadow: Uint8Array; gravel: Uint8Array } };
type Detail = { id: string; layer: string; position: number[]; yaw: number; scale: number; variant: number };
type TerrainApi = {
  normalizeTerrainPatchRecipe(value: unknown): Recipe;
  generateTerrainPatch(recipe: Recipe, index: [number, number]): Patch;
  scatterTerrainDetail(patch: Patch): Detail[];
};
// The declared public module is the trusted producer boundary; its normalizer validates the saved recipe.
const loaded: unknown = await import(pathToFileURL(path.join(source, entry)).href);
if (loaded === null || typeof loaded !== "object" ||
    !["normalizeTerrainPatchRecipe", "generateTerrainPatch", "scatterTerrainDetail"].every((key) =>
      typeof Reflect.get(loaded, key) === "function")) {
  throw new Error("Invalid public terrain recipe module");
}
const api = loaded as TerrainApi;
const recipe = api.normalizeTerrainPatchRecipe(JSON.parse(await readFile(path.join(directory, "recipe.json"), "utf8")));
const patch = api.generateTerrainPatch(recipe, [0, 0]);
const detail = api.scatterTerrainDetail(patch);
const pieces: Record<string, string[]> = {
  grass: ["grass-short", "grass-tall", "grass-clump", "grass-dry"].map((piece) => `greyhaven-meadow.kit.${piece}`),
  flowers: ["flowers-yellow", "flowers-white", "flowers-red", "flowers-mixed"].map((piece) => `greyhaven-meadow.kit.${piece}`),
  stones: ["river-stones.kit.bed-1", "river-stones.kit.bed-2"],
};
const masks = { version: 1, producerRevision, origin: patch.origin.map((v: number) => v * 100), step: recipe.sampleSpacing * 100,
  columns: patch.width, rows: patch.depth, slope: [...patch.slope], exclusion: [...patch.exclusion],
  meadow: [...patch.paint.meadow], gravel: [...patch.paint.gravel] };
const dressing = { version: 1, area: "southmeadow-terrain-patch", placements: detail.map((instance) => {
  const asset = pieces[instance.layer]?.[instance.variant];
  if (!asset) { throw new Error(`Unknown detail variant ${instance.id}`); }
  return { id: `southmeadow:${instance.id}`, asset,
    // Y is deliberately discarded: the existing shared floor anchors the art at runtime.
    position: [Math.round(instance.position[0]! * 100), 0, Math.round(instance.position[2]! * 100)],
    yaw: Math.round(instance.yaw / (2 * Math.PI) * 65536) % 65536,
    scale: [Math.round(instance.scale * 1000), Math.round(instance.scale * 1000), Math.round(instance.scale * 1000)], groundDetail: true };
}) };
await writeFile(path.join(directory, "masks.json"), `${JSON.stringify(masks)}\n`);
await writeFile(path.join(root, "content/greyhaven/dressing/southmeadow.json"), `${JSON.stringify(dressing, null, 2)}\n`);
console.log(`Packaged ${detail.length} stable detail instances and ${patch.width} × ${patch.depth} recipe masks (${producerRevision}).`);
