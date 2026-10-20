import { cp, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { assertCleanSourceCheckout } from "./asset-source-checkout";
import { WARDEN_ICON_DIRECTORY, WARDEN_SOURCE_COMMIT, verifyWardenIcons, wardenIconEntries } from "./warden-icon-package";

const [checkoutArgument, mode = "--check", ...extra] = process.argv.slice(2);
if (!checkoutArgument || extra.length || (mode !== "--check" && mode !== "--write")) {
  throw new Error("usage: bun scripts/package-warden-icons.ts <asset-tooling-checkout> [--check|--write]");
}
const checkout = path.resolve(checkoutArgument);
assertCleanSourceCheckout(checkout, WARDEN_SOURCE_COMMIT);
const producer = await import(pathToFileURL(path.join(checkout, "src/asset-bundle.ts")).href);
const sourceDirectory = path.join(checkout, "review/warden-icons/approved");
const ref = JSON.parse(await readFile(path.join(sourceDirectory, "export.ref.json"), "utf8"));
if (ref.sha256 !== "a59d54c4191a07de613769726ad1c41513f5256f0289281fb0b000e7fcdeddc5") {
  throw new Error("The source checkout does not contain the selected approved Warden export");
}
const { manifest } = await producer.verifyAssetBundle(sourceDirectory, ref, { profile: producer.APPROVED_IMAGE_BUNDLE_PROFILE });
if (mode === "--write") {
  await cp(path.join(sourceDirectory, "manifest.json"), path.join(WARDEN_ICON_DIRECTORY, "producer-manifest.json"));
}
for (const { id, variant, filename } of await wardenIconEntries()) {
  const bytes = await producer.readAssetBundleAsset(sourceDirectory, manifest, `warden.ability.${id}`, variant);
  const destination = path.join(WARDEN_ICON_DIRECTORY, filename);
  if (mode === "--write") {
    await writeFile(destination, bytes);
  } else if (!(await readFile(destination)).equals(bytes)) {
    throw new Error(`${filename} differs from the pinned producer export`);
  }
}
await verifyWardenIcons();
console.log(`Warden icons ${mode === "--write" ? "packaged" : "verified"} from ${WARDEN_SOURCE_COMMIT}`);
