import { describe, expect, test } from "bun:test";
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { buildGrassPackage, GRASS_PACKAGE_DIRECTORY, GRASS_PRODUCER_COMMIT, record, savedMask, sha256 } from "../scripts/grass-package";

const directory = GRASS_PACKAGE_DIRECTORY;
const json = async (name: string): Promise<unknown> => JSON.parse(await readFile(path.join(directory, name), "utf8"));
const accepted = record(await json("accepted.instances.json"));
const selected = record(await json("selected.instances.json"));
const manifest = record(await json("manifest.json"));
const maskText = await readFile(path.join(directory, "exclusion.txt"), "utf8");
const rows = maskText.trimEnd().split("\n");

// Fixture oracle: the saved 71×67 endpoints are exactly one metre apart.
// This lives in tests; production filtering belongs solely to asset-tooling.
function cell(instance: unknown): [number, number] {
  const position = record(instance).positionMicro;
  if (!Array.isArray(position)) {
    throw new Error("Missing instance position");
  }
  return [Math.round(Number(position[0]) / 1_000_000 + 35), Math.round(Number(position[2]) / 1_000_000 + 33)];
}

function instances(set: unknown): unknown[] {
  const values = record(set).instances;
  if (!Array.isArray(values)) {
    throw new Error("Missing instance array");
  }
  return values;
}

function outputSet(outputs: Map<string, Uint8Array>): unknown[] {
  const bytes = outputs.get("selected.instances.json");
  if (!bytes) {
    throw new Error("Missing selected package output");
  }
  return instances(JSON.parse(Buffer.from(bytes).toString("utf8")));
}

describe("Greyhaven accepted grass package", () => {
  test("source and derived bytes agree with the complete pinned provenance", async () => {
    expect(record(manifest.producer).commit).toBe(GRASS_PRODUCER_COMMIT);
    expect(record(manifest.consumerAdapter).sha256).toBe(sha256(await readFile(new URL("../scripts/grass-package.ts", import.meta.url))));
    for (const [filename, hash] of Object.entries(record(record(manifest.consumerAdapter).dependencies))) {
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
    const refs = record(record(manifest.build).inputs);
    expect(record(refs.source).sha256).toBe(record(record(manifest.sourceFiles)["accepted.instances.json"]).sha256);
    expect(record(refs.mask).sha256).toBe(record(record(manifest.outputs)["exclusion.rgba8.json"]).sha256);
    const output = record(record(record(manifest.result).outputs).output);
    expect(output.sha256).toBe(record(record(manifest.outputs)["selected.instances.json"]).sha256);
    expect(manifest.evidence).toMatchObject({ sourceCount: 118, candidatesGenerated: 0, repeatedFilterMatches: true, coldReplayMatches: true });
  });

  test("non-square endpoint calibration preserves exact ordered IDs, XYZ and transforms", async () => {
    expect(accepted.bounds).toEqual({ widthMicro: 70_000_001, depthMicro: 66_000_001 });
    const original = instances(accepted);
    const expected = original.filter((value) => {
      const [x, z] = cell(value);
      return rows[z]?.[x] === ".";
    });
    expect(instances(selected)).toEqual(expected);
    expect(expected.length).toBeGreaterThan(0);
    expect(expected.length).toBeLessThan(original.length);
    expect(new Set(original.map((value) => record(value).id)).size).toBe(118);
    const transforms = await json("transforms.json");
    const picked = await json("selected.transforms.json");
    if (!Array.isArray(transforms)) {
      throw new Error("Missing accepted transforms");
    }
    const kept = new Set(expected.map((value) => record(value).id));
    expect(picked).toEqual(transforms.filter((value) => kept.has(record(value).id)));
    const endpoint = (x: number, z: number) => cell({ positionMicro: [x, 0, z] });
    expect(endpoint(-35_000_000, -33_000_000)).toEqual([0, 0]);
    expect(endpoint(35_000_000, 33_000_000)).toEqual([70, 66]);
    expect(endpoint(-34_500_000, -32_500_000)).toEqual([1, 1]);
  });

  test("inspectable glyphs encode opaque linear coverage and reject ambiguous saved masks", async () => {
    const pixels = savedMask(Buffer.from(maskText), 71, 67);
    const encoded = record(await json("exclusion.rgba8.json"));
    expect(encoded.width).toBe(71);
    expect(encoded.height).toBe(67);
    expect(Buffer.from(String(encoded.pixelsBase64), "base64")).toEqual(Buffer.from(pixels));
    const maskRef = record(record(record(manifest.build).inputs).mask);
    expect(maskRef.metadata).toMatchObject({ sampling: "data", channelColorSpace: "linear" });
    for (const text of [maskText.slice(0, -1), maskText.replace(".", "?"), maskText.replace("\n", "\r\n"), maskText + "\n"]) {
      expect(() => savedMask(Buffer.from(text), 71, 67)).toThrow("Saved mask");
    }
  });

  const checkout = process.env.ASSET_TOOLING_MASK_SOURCE;
  test.skipIf(!checkout)("public producer replay, localized edit, undo and full clearing retain the original accepted set", async () => {
    const scratch = await mkdtemp(path.join(tmpdir(), "mmorpg-grass-edit-"));
    try {
      await cp(directory, scratch, { recursive: true });
      const baseline = await buildGrassPackage(checkout!, scratch);
      for (const [name, bytes] of baseline) {
        expect(Buffer.from(bytes)).toEqual(await readFile(path.join(directory, name)));
      }
      const [column, row] = cell(instances(selected)[0]);
      const edited = rows.map((line) => line.split(""));
      edited[row]![column] = "X";
      await writeFile(path.join(scratch, "exclusion.txt"), edited.map((line) => line.join("")).join("\n") + "\n");
      const changed = outputSet(await buildGrassPackage(checkout!, scratch));
      expect(changed).toEqual(instances(selected).filter((value) => {
        const [x, z] = cell(value);
        return x !== column || z !== row;
      }));
      expect(changed.length).toBeLessThan(instances(selected).length);
      await writeFile(path.join(scratch, "exclusion.txt"), maskText);
      expect(await buildGrassPackage(checkout!, scratch)).toEqual(baseline);
      await writeFile(path.join(scratch, "exclusion.txt"), (".".repeat(71) + "\n").repeat(67));
      expect(outputSet(await buildGrassPackage(checkout!, scratch))).toEqual(instances(accepted));
      await writeFile(path.join(scratch, "exclusion.txt"), ("X".repeat(71) + "\n").repeat(67));
      expect(outputSet(await buildGrassPackage(checkout!, scratch))).toEqual([]);
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  });

  test.skipIf(!checkout)("mismatched calibration and producer attribution fail before producing a package", async () => {
    const scratch = await mkdtemp(path.join(tmpdir(), "mmorpg-grass-invalid-"));
    try {
      await cp(directory, scratch, { recursive: true });
      const source = record(await json("source.json"));
      const mask = record(source.mask);
      await writeFile(path.join(scratch, "source.json"), JSON.stringify({ ...source, mask: { ...mask, bounds: { widthMicro: 70_000_000, depthMicro: 66_000_001 } } }));
      await expect(buildGrassPackage(checkout!, scratch)).rejects.toThrow();
      await writeFile(path.join(scratch, "source.json"), JSON.stringify({ ...source, producer: { ...record(source.producer), commit: "0000000000000000000000000000000000000000" } }));
      await expect(buildGrassPackage(checkout!, scratch)).rejects.toThrow();
      await writeFile(path.join(scratch, "source.json"), await readFile(path.join(directory, "source.json")));
      const fractional = record(await json("accepted.instances.json"));
      const first = record(instances(fractional)[0]);
      const position = first.positionMicro;
      if (!Array.isArray(position)) {
        throw new Error("Missing accepted position");
      }
      position[0] = Number(position[0]) + 1;
      await writeFile(path.join(scratch, "accepted.instances.json"), JSON.stringify(fractional));
      await expect(buildGrassPackage(checkout!, scratch)).rejects.toThrow("saved grass positions must lower exactly");
      expect(await readFile(path.join(scratch, "selected.instances.json"))).toEqual(await readFile(path.join(directory, "selected.instances.json")));
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  });
});
