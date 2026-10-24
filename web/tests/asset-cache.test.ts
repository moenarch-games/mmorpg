import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { assetManifest } from "../scripts/build-asset-cache";
import { assetRevision, isCurrentKey, parseManifest, revisionKey } from "../src/asset-cache/policy";

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
