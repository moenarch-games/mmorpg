import { reconcileGrassPackage } from "./grass-package";

const mode = process.argv[3] ?? "--check";
if (mode !== "--check" && mode !== "--write") {
  throw new Error("usage: bun web/scripts/package-grass.ts [clean-asset-tooling-checkout] [--check|--write]");
}
await reconcileGrassPackage(process.argv[2] ?? "../asset-tooling", mode);
