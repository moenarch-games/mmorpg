import assert from "node:assert/strict";
import { mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { ASSET_AUTHORING_COMMIT, createPinnedAssetOperationCaller, record, sha256 } from "./asset-operation-api";

export const RELIEF_PACKAGE_DIRECTORY = path.resolve(import.meta.dir, "../../crates/mmorpg-scenery/assets/outpost-relief");
export const RELIEF_GRID = { originXzUnits: [-3500, -1300], stepUnits: 50, columns: 141, rows: 133, heightOffset: 128, unitsPerHeightByte: 1 } as const;
const GLYPHS: Readonly<Record<string, number>> = { ".": 0, "1": 64, "2": 128, "3": 192, "X": 255 };
const MEDIA = "application/vnd.moritzbrantner.rgba8+json";

function rustField(heights: readonly number[]): Uint8Array {
  const rows = Array.from({ length: RELIEF_GRID.rows }, (_, row) =>
    `        ${heights.slice(row * RELIEF_GRID.columns, (row + 1) * RELIEF_GRID.columns).join(", ")},`);
  return Buffer.from(`HeightField {\n    origin: [${RELIEF_GRID.originXzUnits.join(", ")}],\n    step: ${RELIEF_GRID.stepUnits},\n    columns: ${RELIEF_GRID.columns},\n    rows: ${RELIEF_GRID.rows},\n    heights: &[\n${rows.join("\n")}\n    ],\n}\n`);
}

export function reliefMask(bytes: Uint8Array): Uint8Array {
  const lines = Buffer.from(bytes).toString("utf8").split("\n");
  if (lines.pop() !== "" || lines.length !== RELIEF_GRID.rows || lines.some((line) => line.length !== RELIEF_GRID.columns || /[^.123X]/.test(line))) {
    throw new Error("Relief mask must match the declared non-square grid and glyphs");
  }
  const coverage = Uint8Array.from(lines.join("").split("").map((glyph) => GLYPHS[glyph]!));
  for (let row = 0; row < RELIEF_GRID.rows; row += 1) {
    for (let column = 0; column < RELIEF_GRID.columns; column += 1) {
      if (row < 2 || row >= RELIEF_GRID.rows - 2 || column < 2 || column >= RELIEF_GRID.columns - 2) {
        assert.equal(coverage[row * RELIEF_GRID.columns + column], 0, "mask must preserve the two-sample boundary");
      }
    }
  }
  return coverage;
}

export async function buildReliefPackage(checkout: string, directory = RELIEF_PACKAGE_DIRECTORY) {
  const call = await createPinnedAssetOperationCaller(checkout, ASSET_AUTHORING_COMMIT,
    ["./operations", "./operations/store", "./operations/image/terrain", "./image/rgba8"]);
  const names = ["source.json", "source.heights.json", "flatten.txt"] as const;
  const inputs = new Map<string, Buffer>();
  for (const name of names) {
    inputs.set(name, await readFile(path.join(directory, name)));
  }
  const recipe = record(JSON.parse(inputs.get("source.json")!.toString("utf8")));
  assert.equal(recipe.schemaVersion, 1);
  const captured = record(recipe.capturedFrom);
  assert.equal(captured.repository, "https://github.com/moritzbrantner/mmorpg");
  assert.equal(captured.query, "mmorpg-wasm reliefAt(x,z)");
  assert.ok(typeof captured.commit === "string" && /^[0-9a-f]{40}$/.test(captured.commit), "capture commit must be a full Git identity");
  assert.ok(typeof captured.sceneryExportSha256 === "string" && /^[0-9a-f]{64}$/.test(captured.sceneryExportSha256));
  assert.equal(captured.contentRevision, "4");
  assert.equal(captured.contentFingerprint, "5738a86de795e940");
  for (const key of ["sceneryChecksum", "exportFingerprint"]) {
    assert.ok(typeof captured[key] === "string" && /^[0-9a-f]{16}$/.test(captured[key]));
  }
  assert.deepEqual(recipe.grid, RELIEF_GRID);
  assert.deepEqual(recipe.producer, { repository: "https://github.com/moritzbrantner/asset-tooling", commit: ASSET_AUTHORING_COMMIT, operation: "image.height.mask-flatten@1" });
  assert.deepEqual(recipe.flatten, { targetHeight: 128, channel: "scalar", glyphCoverage: GLYPHS, borderSamples: 2 });
  const source = record(JSON.parse(inputs.get("source.heights.json")!.toString("utf8")));
  assert.equal(sha256(inputs.get("source.heights.json")!), captured.heightSamplesSha256,
    "source heights must match their immutable capture digest");
  const { heights, ...grid } = source;
  assert.deepEqual(grid, { schemaVersion: 1, ...RELIEF_GRID });
  assert.ok(Array.isArray(heights) && heights.length === RELIEF_GRID.columns * RELIEF_GRID.rows);
  const encodedHeights = heights.map((height) => {
    assert.ok(typeof height === "number" && Number.isInteger(height) && height >= -60 && height <= 60,
      "source must fit the declared exact signed-centimetre relief range");
    return height + 128;
  });
  const coverage = reliefMask(inputs.get("flatten.txt")!);
  const encode = async (values: readonly number[] | Uint8Array): Promise<Uint8Array> => {
    const pixels = Uint8Array.from(Array.from(values).flatMap((value) => [value, value, value, 255]));
    const result = await call("./image/rgba8", "encodeRgba8Image", { width: RELIEF_GRID.columns, height: RELIEF_GRID.rows, pixels });
    assert.ok(result instanceof Uint8Array);
    return result;
  };
  const sourceBytes = await encode(encodedHeights);
  const maskBytes = await encode(coverage);
  const scratch = await mkdtemp(path.join(tmpdir(), "mmorpg-relief-authoring-"));
  const store = path.join(scratch, "warm");
  const cold = path.join(scratch, "cold");
  try {
    const sourceOptions = { kind: "image", mediaType: MEDIA, bytes: sourceBytes,
      metadata: { field: "height", sampling: "data", channelColorSpace: "linear", heightOffset: 128, unitsPerHeightByte: 1, sourceHeightSha256: sha256(inputs.get("source.heights.json")!) } };
    const maskOptions = { kind: "image", mediaType: MEDIA, bytes: maskBytes,
      metadata: { field: "flatten-coverage", sampling: "data", channelColorSpace: "linear", sourceTextSha256: sha256(inputs.get("flatten.txt")!) } };
    const sourceRef = record(await call("./operations/store", "storeAssetObject", store, sourceOptions)).asset;
    const maskRef = record(await call("./operations/store", "storeAssetObject", store, maskOptions)).asset;
    const invocation = { inputs: { source: sourceRef, mask: maskRef }, parameters: { targetHeight: 128, channel: "scalar" } };
    const build = await call("./operations/image/terrain", "createHeightMaskFlattenOperationBuildIdentity", store, invocation);
    const result = await call("./operations/image/terrain", "executeHeightMaskFlattenOperation", store, invocation);
    assert.deepEqual(await call("./operations/image/terrain", "executeHeightMaskFlattenOperation", store, invocation), result);
    for (const options of [sourceOptions, maskOptions]) {
      await call("./operations/store", "storeAssetObject", cold, options);
    }
    assert.deepEqual(await call("./operations/image/terrain", "executeHeightMaskFlattenOperation", cold, invocation), result, "cold replay must be exact");
    const output = await call("./operations", "createAssetRef", record(record(result).outputs).output);
    const outputBytes = await call("./operations/store", "resolveAssetObject", store, output);
    assert.ok(outputBytes instanceof Uint8Array);
    const image = record(await call("./image/rgba8", "parseRgba8Image", outputBytes));
    assert.equal(image.width, RELIEF_GRID.columns);
    assert.equal(image.height, RELIEF_GRID.rows);
    assert.ok(image.pixels instanceof Uint8Array);
    const pixels = image.pixels;
    const flattened = Array.from({ length: heights.length }, (_, index) => {
      const offset = index * 4;
      const height = pixels[offset]!;
      assert.equal(pixels[offset + 1], height);
      assert.equal(pixels[offset + 2], height);
      assert.equal(pixels[offset + 3], 255);
      return height - 128;
    });
    const outputs = new Map<string, Uint8Array>([
      ["source.rgba8.json", sourceBytes], ["flatten.rgba8.json", maskBytes], ["flattened.rgba8.json", outputBytes],
      ["flattened.heights.json", Buffer.from(`${JSON.stringify({ schemaVersion: 1, ...RELIEF_GRID, heights: flattened })}\n`)],
      ["source.heights.rs", rustField(heights)], ["flattened.heights.rs", rustField(flattened)],
    ]);
    const consumerAdapter = { source: "web/scripts/relief-package.ts", sha256: sha256(await readFile(new URL("./relief-package.ts", import.meta.url))),
      dependencies: { "web/scripts/asset-operation-api.ts": sha256(await readFile(new URL("./asset-operation-api.ts", import.meta.url))),
        "web/scripts/asset-source-checkout.ts": sha256(await readFile(new URL("./asset-source-checkout.ts", import.meta.url))) } };
    const manifest = { schemaVersion: 1, producer: recipe.producer, grid: RELIEF_GRID, consumerAdapter,
      sourceFiles: Object.fromEntries(names.map((name) => [name, { sha256: sha256(inputs.get(name)!), byteLength: inputs.get(name)!.length }])),
      build, result, outputs: Object.fromEntries([...outputs].map(([name, bytes]) => [name, { sha256: sha256(bytes), byteLength: bytes.length }])),
      evidence: { repeatedFilterMatches: true, coldReplayMatches: true, sampleCount: heights.length } };
    outputs.set("manifest.json", Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`));
    return outputs;
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

export async function reconcileReliefPackage(checkout: string, mode: "--check" | "--write", directory = RELIEF_PACKAGE_DIRECTORY): Promise<void> {
  const outputs = await buildReliefPackage(checkout, directory);
  if (mode === "--write") {
    await replaceReliefOutputs(directory, outputs);
    console.log("Greyhaven relief package written");
    return;
  }
  for (const [name, bytes] of outputs) {
    assert.deepEqual(await readFile(path.join(directory, name)), Buffer.from(bytes), `${name} differs from the pinned asset-tooling build`);
  }
  console.log("Greyhaven relief package verified");
}

/** Stage and validate every output before atomically replacing changed files. */
async function replaceReliefOutputs(directory: string, outputs: Map<string, Uint8Array>): Promise<void> {
  const staging = await mkdtemp(path.join(directory, ".relief-stage-"));
  const changed: string[] = [];
  try {
    for (const [name, bytes] of outputs) {
      const filename = path.join(directory, name);
      const previous = await existingOutput(filename);
      if (previous?.equals(Buffer.from(bytes))) {
        continue;
      }
      const staged = path.join(staging, name);
      await writeFile(staged, bytes);
      assert.deepEqual(await readFile(staged), Buffer.from(bytes), "staged package bytes must validate before replacement");
      if (previous) {
        await writeFile(path.join(staging, `${name}.previous`), previous);
      }
      changed.push(name);
    }
    const replaced: string[] = [];
    try {
      // The manifest is last in the output map: it commits the checked bytes.
      for (const name of changed) {
        await rename(path.join(staging, name), path.join(directory, name));
        replaced.push(name);
      }
    } catch (error) {
      for (const name of replaced.reverse()) {
        const previous = path.join(staging, `${name}.previous`);
        if (await existingOutput(previous)) {
          await rename(previous, path.join(directory, name));
        } else {
          await rm(path.join(directory, name));
        }
      }
      throw error;
    }
  } finally {
    await rm(staging, { recursive: true, force: true });
  }
}

async function existingOutput(filename: string): Promise<Buffer | undefined> {
  try {
    return await readFile(filename);
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") {
      return undefined;
    }
    throw error;
  }
}
