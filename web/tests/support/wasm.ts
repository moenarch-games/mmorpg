import { readFileSync } from "node:fs";
import { join } from "node:path";
import { buildWasm } from "../../scripts/build-wasm";

/**
 * Preloaded before every test file (`bunfig.toml`): builds the release `mmorpg-wasm` module (the
 * same build `bun run build` uses) once per test process and instantiates it synchronously, so
 * every test decodes projections and encodes commands with the Rust wire code (#541).
 */
const directory = buildWasm();
const generated = (await import(join(directory, "mmorpg_wasm.js"))) as { initSync(options: { module: BufferSource }): unknown };
generated.initSync({ module: readFileSync(join(directory, "mmorpg_wasm_bg.wasm")) });
