import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { ASSET_AUTHORING_COMMIT, createPinnedAssetOperationCaller, record, sha256 } from "./asset-operation-api";

export { record, sha256 } from "./asset-operation-api";

export const GRASS_PACKAGE_DIRECTORY = path.resolve(import.meta.dir, "../../crates/mmorpg-scenery/assets/outpost-grass");
export const GRASS_PRODUCER_COMMIT = ASSET_AUTHORING_COMMIT;

export function savedMask(bytes: Uint8Array, columns: number, rows: number): Uint8Array {
  const text = Buffer.from(bytes).toString("utf8");
  const lines = text.split("\n");
  if (lines.pop() !== "" || lines.length !== rows || lines.some((line) => line.length !== columns || /[^.X]/.test(line))) {
    throw new Error("Saved mask must contain exactly the declared rows/columns of . and X, with a final newline");
  }
  return Uint8Array.from(lines.join("").split("").flatMap((glyph) => {
    const value = glyph === "X" ? 255 : 0;
    return [value, value, value, 255];
  }));
}

/** Lower accepted placements exactly to centimetre game units; never sample a mask here. */
function rustPlacements(instances: unknown[], transforms: unknown[]): Uint8Array {
  assert.equal(instances.length, transforms.length);
  const lines = instances.map((value, index) => {
    const instance = record(value);
    const transform = record(transforms[index]);
    assert.equal(instance.id, transform.id);
    assert.ok(typeof instance.id === "string" && /^outpost-grass-[0-9]{3}$/.test(instance.id));
    assert.ok(Array.isArray(instance.positionMicro) && instance.positionMicro.length === 3);
    const position = instance.positionMicro.map((coordinate, axis) => {
      assert.ok(typeof coordinate === "number" && Number.isSafeInteger(coordinate) && coordinate % 10_000 === 0,
        "saved grass positions must lower exactly to centimetres");
      return coordinate / 10_000 + (axis === 2 ? 2000 : 0);
    });
    assert.ok(Array.isArray(transform.halfExtents));
    return `    (${JSON.stringify(instance.id)}, [${position.join(", ")}], ${transform.yaw}, ${transform.scalePermille}, [${transform.halfExtents.join(", ")}]),`;
  });
  return Buffer.from(`[\n${lines.join("\n")}\n]\n`);
}

export async function buildGrassPackage(checkout: string, directory = GRASS_PACKAGE_DIRECTORY) {
  checkout = path.resolve(checkout);
  const call = await createPinnedAssetOperationCaller(checkout, GRASS_PRODUCER_COMMIT,
    ["./operations", "./operations/store", "./operations/instances/masks", "./instance-set", "./image/rgba8"]);
  const names = ["source.json", "accepted.instances.json", "transforms.json", "exclusion.txt"] as const;
  const inputs = new Map<string, Buffer>();
  for (const name of names) {
    inputs.set(name, await readFile(path.join(directory, name)));
  }
  const source = record(JSON.parse(inputs.get("source.json")!.toString("utf8")));
  assert.equal(source.schemaVersion, 1);
  assert.equal(source.family, "grass-tuft");
  assert.equal(source.unitsPerMetre, 100);
  assert.deepEqual(source.worldOriginUnits, [0, 2000]);
  assert.equal(record(source.producer).repository, "https://github.com/moritzbrantner/asset-tooling");
  assert.equal(record(source.producer).commit, GRASS_PRODUCER_COMMIT);
  assert.equal(record(source.producer).operation, "instances.filter.exclusion-mask@1");
  const mask = record(source.mask);
  assert.equal(mask.columns, 71);
  assert.equal(mask.rows, 67);
  assert.equal(mask.keep, ".");
  assert.equal(mask.exclude, "X");
  assert.equal(mask.maxCoverage, 127);
  const bounds = { widthMicro: 70_000_001, depthMicro: 66_000_001 };
  assert.deepEqual(mask.bounds, bounds);
  const accepted = record(await call("./instance-set", "parseInstanceSet", inputs.get("accepted.instances.json")));
  assert.deepEqual(accepted.bounds, bounds, "mask calibration must match the accepted footprint");
  const transforms: unknown = JSON.parse(inputs.get("transforms.json")!.toString("utf8"));
  assert.ok(Array.isArray(transforms));
  assert.ok(Array.isArray(accepted.instances));
  assert.deepEqual(transforms.map((value) => record(value).id), accepted.instances.map((value) => record(value).id));
  for (const value of transforms) {
    const transform = record(value);
    assert.ok(Number.isInteger(transform.yaw) && Number(transform.yaw) >= 0 && Number(transform.yaw) <= 65535);
    assert.ok(Number.isInteger(transform.scalePermille) && Number(transform.scalePermille) >= 700 && Number(transform.scalePermille) <= 1300);
    assert.ok(Array.isArray(transform.halfExtents) && transform.halfExtents.length === 3 && transform.halfExtents.every((v) => Number.isInteger(v) && Number(v) > 0));
  }
  const pixels = savedMask(inputs.get("exclusion.txt")!, 71, 67);
  const encodedMask = await call("./image/rgba8", "encodeRgba8Image", { width: 71, height: 67, pixels });
  assert.ok(encodedMask instanceof Uint8Array);
  const media = "application/vnd.moritzbrantner.rgba8+json";
  const scratch = await mkdtemp(path.join(tmpdir(), "mmorpg-grass-authoring-"));
  const store = path.join(scratch, "warm");
  const cold = path.join(scratch, "cold");
  try {
    const sourceOptions = { kind: "instance-set", mediaType: "application/vnd.asset-tooling.instance-set+json",
      bytes: inputs.get("accepted.instances.json"), metadata: { purpose: "accepted-greyhaven-outpost-grass" } };
    const maskOptions = { kind: "image", mediaType: media, bytes: encodedMask,
      metadata: { sampling: "data", channelColorSpace: "linear", meaning: "exclusion", sourceTextSha256: sha256(inputs.get("exclusion.txt")!) } };
    const sourceRef = record(await call("./operations/store", "storeAssetObject", store, sourceOptions)).asset;
    const maskRef = record(await call("./operations/store", "storeAssetObject", store, maskOptions)).asset;
    const invocation = { inputs: { source: sourceRef, mask: maskRef }, parameters: { maxCoverage: 127, maskBounds: bounds } };
    const build = await call("./operations/instances/masks", "createInstanceExclusionMaskOperationBuildIdentity", store, invocation);
    const result = await call("./operations/instances/masks", "executeInstanceExclusionMaskOperation", store, invocation);
    assert.deepEqual(await call("./operations/instances/masks", "executeInstanceExclusionMaskOperation", store, invocation), result);
    for (const options of [sourceOptions, maskOptions]) {
      await call("./operations/store", "storeAssetObject", cold, options);
    }
    assert.deepEqual(await call("./operations/instances/masks", "executeInstanceExclusionMaskOperation", cold, invocation), result, "cold replay must be exact");
    const output = await call("./operations", "createAssetRef", record(record(result).outputs).output);
    const selectedBytes = await call("./operations/store", "resolveAssetObject", store, output);
    assert.ok(selectedBytes instanceof Uint8Array);
    const selected = record(await call("./instance-set", "parseInstanceSet", selectedBytes));
    assert.ok(Array.isArray(selected.instances));
    const byId = new Map(accepted.instances.map((value) => [record(value).id, value]));
    for (const value of selected.instances) {
      assert.deepEqual(value, byId.get(record(value).id), "filter must retain original XYZ");
    }
    const selectedIds = new Set(selected.instances.map((value) => record(value).id));
    const selectedTransforms = transforms.filter((value) => selectedIds.has(record(value).id));
    const outputFiles = new Map<string, Uint8Array>([
      ["selected.instances.json", selectedBytes],
      ["selected.transforms.json", Buffer.from(`${JSON.stringify(selectedTransforms, null, 2)}\n`)],
      ["exclusion.rgba8.json", encodedMask],
      ["accepted.props.rs", rustPlacements(accepted.instances, transforms)],
      ["selected.props.rs", rustPlacements(selected.instances, selectedTransforms)],
    ]);
    const consumerAdapter = { source: "web/scripts/grass-package.ts", sha256: sha256(await readFile(new URL("./grass-package.ts", import.meta.url))),
      dependencies: { "web/scripts/asset-operation-api.ts": sha256(await readFile(new URL("./asset-operation-api.ts", import.meta.url))),
        "web/scripts/asset-source-checkout.ts": sha256(await readFile(new URL("./asset-source-checkout.ts", import.meta.url))) } };
    const manifest = { schemaVersion: 1, producer: source.producer, consumerAdapter, sourceFiles: Object.fromEntries(names.map((name) => [name, { sha256: sha256(inputs.get(name)!), byteLength: inputs.get(name)!.length }])),
      build, result, outputs: Object.fromEntries([...outputFiles].map(([name, bytes]) => [name, { sha256: sha256(bytes), byteLength: bytes.length }])),
      evidence: { repeatedFilterMatches: true, coldReplayMatches: true, sourceCount: accepted.instances.length, keptCount: selected.instances.length, candidatesGenerated: 0 } };
    outputFiles.set("manifest.json", Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`));
    return outputFiles;
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

export async function reconcileGrassPackage(checkout: string, mode: "--check" | "--write", directory = GRASS_PACKAGE_DIRECTORY): Promise<void> {
  const outputs = await buildGrassPackage(checkout, directory);
  for (const [name, bytes] of outputs) {
    const filename = path.join(directory, name);
    if (mode === "--write") {
      await writeFile(filename, bytes);
    } else {
      assert.deepEqual(await readFile(filename), Buffer.from(bytes), `${name} differs from the pinned asset-tooling build`);
    }
  }
  console.log(`Greyhaven grass package ${mode === "--check" ? "verified" : "written"}`);
}
