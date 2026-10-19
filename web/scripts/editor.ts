import { buildWasm } from "./build-wasm";

process.env.MMORPG_LAYOUT_EDITOR = "1";
buildWasm();
const { createServer } = await import("vite");
const server = await createServer({ server: { host: "127.0.0.1", open: "/mmorpg/editor.html" } });
await server.listen();
server.printUrls();
