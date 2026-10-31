import { wasm } from "../../src/wasm";
import type { LocalZoneModule } from "../../src/world/local-world";

/** The `mmorpg-wasm` module the test preload (`support/wasm.ts`) built and instantiated. */
export function localZoneModule(): Promise<LocalZoneModule> {
  return Promise.resolve(wasm);
}
