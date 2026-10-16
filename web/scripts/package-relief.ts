import { reconcileReliefPackage } from "./relief-package";

const mode = process.argv[3] ?? "--check";
if (mode !== "--check" && mode !== "--write") {
  throw new Error("usage: bun web/scripts/package-relief.ts [clean-asset-tooling-checkout] [--check|--write]");
}
await reconcileReliefPackage(process.argv[2] ?? "../asset-tooling", mode);
