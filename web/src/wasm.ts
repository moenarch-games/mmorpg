import init from "./generated/mmorpg-wasm/mmorpg_wasm.js";
import { countWasmBoundary, profiling } from "./world/frame-profile";

/** The generated `mmorpg-wasm` bindings: the local zone host and the wire boundary (#541). */
export * as wasm from "./generated/mmorpg-wasm/mmorpg_wasm.js";

let loading: Promise<WebAssembly.Module> | null = null;

/** Compiles the module's build, streaming where the server sends it as `application/wasm`. */
async function compile(): Promise<WebAssembly.Module> {
  const response = await fetch(new URL("./generated/mmorpg-wasm/mmorpg_wasm_bg.wasm", import.meta.url));
  if (!response.ok) { throw new Error(`failed to fetch Wasm: ${response.status} ${response.statusText}`); }
  if (response.headers.get("Content-Type") === "application/wasm") {
    return WebAssembly.compileStreaming(response);
  }
  return WebAssembly.compile(await response.arrayBuffer());
}

/**
 * Fetches and instantiates `mmorpg-wasm` once per page. Everything that decodes a projection or
 * encodes a command, local or online, needs it first. Resolves to the compiled module, which a
 * worker instantiates without compiling again (#855).
 */
export function loadWasm(): Promise<WebAssembly.Module> {
  loading ??= compile().then(async (module) => {
    // Under `?profile` each frame counts its boundary crossings (#1256).
    const instantiate = () => init({ module_or_path: module });
    await (profiling() ? countWasmBoundary(instantiate) : instantiate());
    return module;
  });
  return loading;
}
