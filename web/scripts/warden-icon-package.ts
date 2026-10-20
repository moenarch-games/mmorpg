import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

export const WARDEN_ICON_DIRECTORY = fileURLToPath(new URL("../assets/warden-icons/", import.meta.url));
export const WARDEN_IDS = [1, 2, 3, 4] as const;
export const WARDEN_VARIANTS = ["64", "512"] as const;
export const WARDEN_SOURCE_COMMIT = "11b19021779349fbb2b465956b5ff609b48f0fc5";
const MANIFEST_SHA256 = "a59d54c4191a07de613769726ad1c41513f5256f0289281fb0b000e7fcdeddc5";
const digest = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");

type ApprovedEntry = {
  key: string; variant: string; path: string; approved: { sha256: string };
  source: { sha256: string; byteLength: number; mediaType: string };
};

/** Fixed consumer selection, not a second producer bundle/PNG parser. The exact
 * producer manifest is trusted only after its immutable content pin matches. */
export async function wardenIconEntries(directory = WARDEN_ICON_DIRECTORY) {
  const bytes = await readFile(path.join(directory, "producer-manifest.json"));
  if (bytes.length !== 53124 || digest(bytes) !== MANIFEST_SHA256) {
    throw new Error("Warden icon producer manifest differs from the owner-approved pin");
  }
  const manifest = JSON.parse(bytes.toString("utf8")) as { schemaVersion: number; profile: string; assets: ApprovedEntry[] };
  if (manifest.schemaVersion !== 3 || manifest.profile !== "approved-png-v1") {
    throw new Error("Warden icons require the approved-png-v1 producer export");
  }
  return WARDEN_IDS.flatMap((id) => WARDEN_VARIANTS.map((variant) => {
    const entry = manifest.assets.find((item) => item.key === `warden.ability.${id}` && item.variant === variant);
    if (!entry || entry.approved.sha256 !== entry.source.sha256 || entry.source.mediaType !== "image/png") {
      throw new Error(`Missing approved Warden icon ${id}/${variant}`);
    }
    return { id, variant, entry, filename: `warden-${id}-${variant}.png` };
  }));
}

/** Builds/dev starts fail before serving missing, changed or unapproved images. */
export async function verifyWardenIcons(directory = WARDEN_ICON_DIRECTORY): Promise<void> {
  const entries = await wardenIconEntries(directory);
  for (const { entry, filename } of entries) {
    const bytes = await readFile(path.join(directory, filename));
    if (bytes.length !== entry.source.byteLength || digest(bytes) !== entry.approved.sha256) {
      throw new Error(`Warden icon ${filename} differs from its approved bytes`);
    }
  }
}

if (import.meta.main) {
  await verifyWardenIcons();
  console.log("Warden icons: all four approved abilities verified (64/512 px)");
}
