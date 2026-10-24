/**
 * After `vite build`: write `dist/asset-cache.json` (each static file's SHA-256 by
 * its path below the base) and bundle the service worker to `dist/sw.js` (#410).
 */
import { createHash } from "node:crypto";
import { readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { MANIFEST_FILE, isCacheableFile } from "../src/asset-cache/policy";

export async function assetManifest(dist: string): Promise<{ files: Record<string, string> }> {
  const entries = await readdir(dist, { recursive: true, withFileTypes: true });
  const relatives = entries
    .filter((entry) => entry.isFile())
    .map((entry) => path.relative(dist, path.join(entry.parentPath, entry.name)).split(path.sep).join("/"))
    .filter(isCacheableFile)
    .sort();
  const files: Record<string, string> = {};
  for (const relative of relatives) {
    files[relative] = createHash("sha256").update(await readFile(path.join(dist, relative))).digest("hex");
  }
  return { files };
}

if (import.meta.main) {
  const dist = fileURLToPath(new URL("../dist/", import.meta.url));
  const manifest = await assetManifest(dist);
  await writeFile(path.join(dist, MANIFEST_FILE), `${JSON.stringify(manifest)}\n`);
  const built = await Bun.build({
    entrypoints: [fileURLToPath(new URL("../src/asset-cache/service-worker.ts", import.meta.url))],
    target: "browser",
    minify: true,
  });
  const [output] = built.outputs;
  if (!built.success || !output) { throw new AggregateError(built.logs, "Service worker bundle failed"); }
  await writeFile(path.join(dist, "sw.js"), await output.text());
  console.log(`Asset cache manifest lists ${Object.keys(manifest.files).length} files`);
}
