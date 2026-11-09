import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { assetManifest } from "../scripts/build-asset-cache";
import { assetRevision, isCurrentKey, parseManifest, revisionKey } from "../src/asset-cache/policy";
import { readDerived, releaseOf, writeDerived } from "../src/asset-cache/derived";

const base = new URL("https://example.test/mmorpg/");
const A = "a".repeat(64);
const B = "b".repeat(64);
const manifest = { files: { "assets/game-x.js": A, "characters/humanoid.ranger.male/clips.glb": B } };

test("the build manifest hashes static files and leaves page shells out", async () => {
  const dist = await mkdtemp(path.join(tmpdir(), "asset-cache-"));
  await mkdir(path.join(dist, "assets"));
  await writeFile(path.join(dist, "index.html"), "<html>");
  await writeFile(path.join(dist, "sw.js"), "");
  await writeFile(path.join(dist, "assets", "a.wasm"), "abc");
  const built = await assetManifest(dist);
  assert.deepEqual(built, { files: { "assets/a.wasm": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" } });
  assert.deepEqual(parseManifest(built), built);
  assert.equal(parseManifest({ files: { x: "stale" } }), null);
});

test("only listed same-origin static paths without a query have a revision", () => {
  assert.equal(assetRevision(manifest, base, new URL("assets/game-x.js", base)), A);
  assert.equal(assetRevision(manifest, base, new URL("assets/game-x.js?debug", base)), null);
  assert.equal(assetRevision(manifest, base, new URL("assets/other.js", base)), null);
  assert.equal(assetRevision(manifest, base, new URL("https://elsewhere.test/mmorpg/assets/game-x.js")), null);
});

test("a cached revision stays current only while the manifest still names it", () => {
  const url = new URL("characters/humanoid.ranger.male/clips.glb", base);
  const key = revisionKey(url, B);
  assert.equal(isCurrentKey(manifest, base, key), true);
  const republished = { files: { ...manifest.files, "characters/humanoid.ranger.male/clips.glb": A } };
  assert.equal(isCurrentKey(republished, base, key), false);
  assert.equal(isCurrentKey(manifest, base, new URL("index.html", base).href), true);
});

// #526: reuse of what the page derives per release; every failure reads as a miss.
function memoryCaches(fail = false): CacheStorage {
  const stores = new Map<string, Map<string, Response>>();
  return {
    open: async (name: string) => {
      if (fail) { throw new Error("no storage"); }
      const store = stores.get(name) ?? new Map<string, Response>();
      stores.set(name, store);
      return {
        match: async (key: string) => store.get(key)?.clone(),
        put: async (key: string, value: Response) => { store.set(key, value.clone()); },
        keys: async () => [...store.keys()].map((url) => ({ url })),
        delete: async (key: { url: string }) => store.delete(key.url),
      } as unknown as Cache;
    },
  } as unknown as CacheStorage;
}

const ORIGIN = "https://example.test";
const OLD = "c".repeat(64);
const NEW = "d".repeat(64);

test("a cold, cleared or failing derived cache is a miss and a warm one returns the bytes", async () => {
  const storage = memoryCaches();
  assert.equal(await readDerived(storage, ORIGIN, NEW, "grids"), null);
  await writeDerived(storage, ORIGIN, NEW, "grids", new Uint8Array([1, 2, 3]));
  assert.deepEqual(await readDerived(storage, ORIGIN, NEW, "grids"), new Uint8Array([1, 2, 3]));
  assert.equal(await readDerived(undefined, ORIGIN, NEW, "grids"), null);
  assert.equal(await readDerived(memoryCaches(true), ORIGIN, NEW, "grids"), null);
  await writeDerived(memoryCaches(true), ORIGIN, NEW, "grids", new Uint8Array([9]));
});

test("a new release invalidates every earlier derived entry", async () => {
  const storage = memoryCaches();
  await writeDerived(storage, ORIGIN, OLD, "grids", new Uint8Array([1]));
  assert.deepEqual(await readDerived(storage, ORIGIN, OLD, "grids"), new Uint8Array([1]));
  await writeDerived(storage, ORIGIN, NEW, "grids", new Uint8Array([2]));
  assert.equal(await readDerived(storage, ORIGIN, OLD, "grids"), null);
  assert.deepEqual(await readDerived(storage, ORIGIN, NEW, "grids"), new Uint8Array([2]));
});

test("a release's identity is its WASM build's hash", () => {
  assert.equal(releaseOf({ files: { "assets/mmorpg_wasm_bg-Ab1.wasm": A, "assets/x.js": B } }), A);
  assert.equal(releaseOf({ files: { "assets/x.js": B } }), null);
});
