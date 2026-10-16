import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { assertCleanSourceCheckout } from "./asset-source-checkout";

export function record(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Expected a public producer export record");
  }
  // JSON records and verified module namespaces establish this boundary.
  return value as Record<string, unknown>;
}

export const ASSET_AUTHORING_COMMIT = "1e79d74ee0a62cd29706ed3254a0293c91d996ba";

export function sha256(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

/** Import declared public exports only after checking the exact clean source. */
export async function createPinnedAssetOperationCaller(checkout: string, commit: string, subpaths: readonly string[]) {
  checkout = path.resolve(checkout);
  assertCleanSourceCheckout(checkout, commit);
  const packageJson = record(JSON.parse(await readFile(path.join(checkout, "package.json"), "utf8")));
  const exports = record(packageJson.exports);
  const modules = new Map<string, Record<string, unknown>>();
  for (const subpath of subpaths) {
    const entry = exports[subpath];
    if (typeof entry !== "string" || !entry.startsWith("./")) {
      throw new Error(`Producer has no public ${subpath} export`);
    }
    const filename = path.resolve(checkout, entry);
    if (!filename.startsWith(`${checkout}${path.sep}`)) {
      throw new Error("Producer export escapes the source checkout");
    }
    const loaded: unknown = await import(pathToFileURL(filename).href);
    modules.set(subpath, record(loaded));
  }
  return async (subpath: string, name: string, ...args: unknown[]): Promise<unknown> => {
    const fn = modules.get(subpath)?.[name];
    if (typeof fn !== "function") {
      throw new Error(`Producer has no callable public ${subpath}/${name}`);
    }
    // Signatures belong to the pinned public API; results remain unknown
    // until the producer's validators or the consumer's checks accept them.
    return await Reflect.apply(fn, undefined, args);
  };
}
