import init from "./generated/mmorpg-wasm/mmorpg_wasm.js";

/** The generated `mmorpg-wasm` bindings: the local zone host and the wire boundary (#541). */
export * as wasm from "./generated/mmorpg-wasm/mmorpg_wasm.js";

let loading: Promise<void> | null = null;

/**
 * Fetches and instantiates `mmorpg-wasm` once per page. Everything that decodes a projection or
 * encodes a command, local or online, needs it first.
 */
export function loadWasm(): Promise<void> {
  loading ??= init().then(() => undefined);
  return loading;
}
