import { describe, expect, test } from "bun:test";
import { cp, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { ASSET_AUTHORING_COMMIT, record, sha256 } from "../scripts/asset-operation-api";
import { buildReliefPackage, reconcileReliefPackage, RELIEF_GRID, RELIEF_PACKAGE_DIRECTORY, reliefMask } from "../scripts/relief-package";

const directory = RELIEF_PACKAGE_DIRECTORY;
const json = async (name: string): Promise<unknown> => JSON.parse(await readFile(path.join(directory, name), "utf8"));
const manifest = record(await json("manifest.json"));
const source = record(await json("source.json"));
const maskText = await readFile(path.join(directory, "flatten.txt"), "utf8");
const coverage = reliefMask(Buffer.from(maskText));
function heights(value: unknown): number[] {
  const samples = record(value).heights;
  if (!Array.isArray(samples) || !samples.every((height: unknown) => typeof height === "number" && Number.isInteger(height))) {
    throw new Error("Missing signed integer height samples");
  }
  return samples;
}
const original = heights(await json("source.heights.json"));
const flattened = heights(await json("flattened.heights.json"));
function outputHeights(outputs: Map<string, Uint8Array>): number[] {
  const bytes = outputs.get("flattened.heights.json");
  if (!bytes) {
    throw new Error("Missing generated heightfield");
  }
  return heights(JSON.parse(Buffer.from(bytes).toString("utf8")));
}

describe("Greyhaven saved relief package", () => {
  test("complete file and adapter identities agree with pinned producer provenance", async () => {
    expect(record(manifest.producer).commit).toBe(ASSET_AUTHORING_COMMIT);
    const adapter = record(manifest.consumerAdapter);
    expect(adapter.sha256).toBe(sha256(await readFile(new URL("../scripts/relief-package.ts", import.meta.url))));
    for (const [filename, hash] of Object.entries(record(adapter.dependencies))) {
      if (typeof hash !== "string") {
        throw new Error("Invalid adapter dependency identity");
      }
      expect(sha256(await readFile(path.resolve(import.meta.dir, "../..", filename)))).toBe(hash);
    }
    for (const group of ["sourceFiles", "outputs"]) {
      for (const [name, value] of Object.entries(record(manifest[group]))) {
        const identity = record(value);
        if (typeof identity.sha256 !== "string" || typeof identity.byteLength !== "number") {
          throw new Error("Invalid package file identity");
        }
        const bytes = await readFile(path.join(directory, name));
        expect(sha256(bytes)).toBe(identity.sha256);
        expect(bytes.length).toBe(identity.byteLength);
      }
    }
    const inputs = record(record(manifest.build).inputs);
    const outputs = record(manifest.outputs);
    expect(record(inputs.source).sha256).toBe(record(outputs["source.rgba8.json"]).sha256);
    expect(record(inputs.mask).sha256).toBe(record(outputs["flatten.rgba8.json"]).sha256);
    expect(record(record(record(manifest.result).outputs).output).sha256).toBe(record(outputs["flattened.rgba8.json"]).sha256);
    expect(manifest.evidence).toEqual({ repeatedFilterMatches: true, coldReplayMatches: true, sampleCount: 18753 });
    expect(record(source.capturedFrom)).toMatchObject({ commit: "6265e1c842aad9d2034bfe7946dec4582158b9e6", query: "mmorpg-wasm reliefAt(x,z)", contentRevision: "4", contentFingerprint: "5738a86de795e940" });
    expect(record(source.capturedFrom).heightSamplesSha256).toBe(sha256(await readFile(path.join(directory, "source.heights.json"))));
  });

  test("zero, full and partial scalar weights match an independent signed-height oracle", () => {
    const counts = { zero: 0, full: 0, partial: 0, changed: 0 };
    for (let index = 0; index < original.length; index += 1) {
      const weight = coverage[index]!;
      // Test-only Q8 oracle; production blending belongs to asset-tooling.
      const expected = Math.floor(((255 - weight) * (original[index]! + 128) + weight * 128 + 127) / 255) - 128;
      expect(flattened[index]).toBe(expected);
      if (weight === 0) {
        counts.zero += 1;
        expect(flattened[index]).toBe(original[index]);
      } else if (weight === 255) {
        counts.full += 1;
        expect(flattened[index]).toBe(0);
      } else {
        counts.partial += 1;
      }
      if (flattened[index] !== original[index]) {
        counts.changed += 1;
      }
    }
    expect(counts).toEqual({ zero: 2670, full: 12213, partial: 3870, changed: 4517 });
  });

  test("non-square row-major calibration and opaque linear centimetres preserve boundaries", async () => {
    expect(source.grid).toEqual(RELIEF_GRID);
    const endpoint = (column: number, row: number) => [RELIEF_GRID.originXzUnits[0] + column * 50, RELIEF_GRID.originXzUnits[1] + row * 50];
    expect(endpoint(0, 0)).toEqual([-3500, -1300]);
    expect(endpoint(140, 132)).toEqual([3500, 5300]);
    expect(endpoint(70, 66)).toEqual([0, 2000]);
    for (const [filename, values, offset] of [["source.rgba8.json", original, 128], ["flatten.rgba8.json", coverage, 0], ["flattened.rgba8.json", flattened, 128]] as const) {
      const image = record(await json(filename));
      expect([image.width, image.height]).toEqual([141, 133]);
      const pixels = Buffer.from(String(image.pixelsBase64), "base64");
      for (let index = 0; index < values.length; index += 1) {
        const scalar = values[index]! + offset;
        expect([...pixels.subarray(index * 4, index * 4 + 4)]).toEqual([scalar, scalar, scalar, 255]);
      }
    }
    for (let row = 0; row < 133; row += 1) {
      for (let column = 0; column < 141; column += 1) {
        if (row < 2 || row >= 131 || column < 2 || column >= 139) {
          const index = row * 141 + column;
          expect(flattened[index]).toBe(original[index]);
        }
      }
    }
    expect(record(record(record(manifest.build).inputs).source).metadata).toMatchObject({ sampling: "data", channelColorSpace: "linear", heightOffset: 128, unitsPerHeightByte: 1 });
  });

  test("ambiguous glyphs, dimensions and changed borders fail closed", () => {
    for (const text of [maskText.slice(0, -1), maskText.replace(".", "?"), maskText.replace("\n", "\r\n"), maskText + "\n", "X" + maskText.slice(1)]) {
      expect(() => reliefMask(Buffer.from(text))).toThrow();
    }
  });

  const checkout = process.env.ASSET_TOOLING_MASK_SOURCE;
  test.skipIf(!checkout)("public replay, one-cell edit, undo and zero/full masks retain immutable source heights", async () => {
    const scratch = await mkdtemp(path.join(tmpdir(), "mmorpg-relief-edit-"));
    try {
      await cp(directory, scratch, { recursive: true });
      const baseline = await buildReliefPackage(checkout!, scratch);
      for (const [name, bytes] of baseline) {
        expect(Buffer.from(bytes)).toEqual(await readFile(path.join(directory, name)));
      }
      const index = original.findIndex((height, index) => height !== 0 && coverage[index] === 0 && index % 141 > 1 && index % 141 < 139 && Math.floor(index / 141) > 1 && Math.floor(index / 141) < 131);
      expect(index).toBeGreaterThan(0);
      const edited = maskText.trimEnd().split("\n").map((line) => line.split(""));
      edited[Math.floor(index / 141)]![index % 141] = "X";
      await writeFile(path.join(scratch, "flatten.txt"), edited.map((line) => line.join("")).join("\n") + "\n");
      const expected = [...flattened];
      expected[index] = 0;
      expect(outputHeights(await buildReliefPackage(checkout!, scratch))).toEqual(expected);
      await writeFile(path.join(scratch, "flatten.txt"), maskText);
      expect(await buildReliefPackage(checkout!, scratch)).toEqual(baseline);
      await writeFile(path.join(scratch, "flatten.txt"), (".".repeat(141) + "\n").repeat(133));
      expect(outputHeights(await buildReliefPackage(checkout!, scratch))).toEqual(original);
      const full = Array.from({ length: 133 }, (_, row) => Array.from({ length: 141 }, (_, column) => {
        if (row < 2 || row >= 131 || column < 2 || column >= 139) {
          return ".";
        }
        return "X";
      }).join("")).join("\n") + "\n";
      await writeFile(path.join(scratch, "flatten.txt"), full);
      const fullHeights = outputHeights(await buildReliefPackage(checkout!, scratch));
      const fullCoverage = reliefMask(Buffer.from(full));
      expect(fullHeights).toEqual(original.map((height, index) => {
        if (fullCoverage[index] === 255) {
          return 0;
        }
        return height;
      }));
      expect(await readFile(path.join(scratch, "source.heights.json"))).toEqual(await readFile(path.join(directory, "source.heights.json")));
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  });

  test.skipIf(!checkout)("invalid coordinate and producer identities fail before changing generated files", async () => {
    const scratch = await mkdtemp(path.join(tmpdir(), "mmorpg-relief-invalid-"));
    try {
      await cp(directory, scratch, { recursive: true });
      for (const recipe of [
        { ...source, grid: { ...RELIEF_GRID, stepUnits: 51 } },
        { ...source, producer: { ...record(source.producer), commit: "0".repeat(40) } },
        { ...source, capturedFrom: { ...record(source.capturedFrom), commit: "main" } },
      ]) {
        await writeFile(path.join(scratch, "source.json"), JSON.stringify(recipe));
        await expect(buildReliefPackage(checkout!, scratch)).rejects.toThrow();
      }
      await writeFile(path.join(scratch, "source.json"), await readFile(path.join(directory, "source.json")));
      const field = record(await json("source.heights.json"));
      const edited = [...original];
      const covered = coverage.findIndex((weight) => weight === 255);
      edited[covered] = edited[covered]! + 1;
      await writeFile(path.join(scratch, "source.heights.json"), JSON.stringify({ ...field, heights: edited }) + "\n");
      await expect(reconcileReliefPackage(checkout!, "--write", scratch)).rejects.toThrow("immutable capture digest");
      expect(await readFile(path.join(scratch, "flattened.heights.json"))).toEqual(await readFile(path.join(directory, "flattened.heights.json")));
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  });

  test.skipIf(!checkout)("reconciliation preserves identical files and validates all staging before replacement", async () => {
    const scratch = await mkdtemp(path.join(tmpdir(), "mmorpg-relief-write-"));
    try {
      await cp(directory, scratch, { recursive: true });
      const before = await stat(path.join(scratch, "manifest.json"));
      await reconcileReliefPackage(checkout!, "--write", scratch);
      const after = await stat(path.join(scratch, "manifest.json"));
      expect([after.ino, after.mtimeMs]).toEqual([before.ino, before.mtimeMs]);
      await rm(path.join(scratch, "flattened.heights.rs"));
      await reconcileReliefPackage(checkout!, "--write", scratch);
      expect(await readFile(path.join(scratch, "flattened.heights.rs"))).toEqual(await readFile(path.join(directory, "flattened.heights.rs")));
      // A later durable target is unreadable; earlier staged bytes must not commit.
      await writeFile(path.join(scratch, "flatten.txt"), (".".repeat(141) + "\n").repeat(133));
      await rm(path.join(scratch, "flattened.heights.json"));
      await mkdir(path.join(scratch, "flattened.heights.json"));
      await expect(reconcileReliefPackage(checkout!, "--write", scratch)).rejects.toThrow();
      for (const name of ["flatten.rgba8.json", "flattened.rgba8.json", "manifest.json"]) {
        expect(await readFile(path.join(scratch, name))).toEqual(await readFile(path.join(directory, name)));
      }
      await rm(path.join(scratch, "flattened.heights.json"), { recursive: true });
      await cp(path.join(directory, "flattened.heights.json"), path.join(scratch, "flattened.heights.json"));
      await reconcileReliefPackage(checkout!, "--write", scratch);
      await reconcileReliefPackage(checkout!, "--check", scratch);
      expect(heights(JSON.parse(await readFile(path.join(scratch, "flattened.heights.json"), "utf8")))).toEqual(original);
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  });
});
