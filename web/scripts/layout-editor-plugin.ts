import { spawnSync } from "node:child_process";
import { readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import type { Plugin } from "vite";
import { buildWasm, generatedDirectory, webRoot } from "./build-wasm";

/** Local-only authoring endpoint; never installed on Pages or a zone host. */
export function layoutEditorPlugin(): Plugin {
  const file = resolve(webRoot, "../content/greyhaven/outpost-layout.json");
  let busy = false;
  return {
    name: "greyhaven-layout-authoring",
    configureServer(server) {
      if (process.env.MMORPG_LAYOUT_EDITOR !== "1") { return; }
      server.middlewares.use("/__layout", async (request, response) => {
        response.setHeader("Content-Type", "application/json");
        response.setHeader("Cache-Control", "no-store");
        const fail = (status: number, message: string) => {
          response.statusCode = status;
          response.end(JSON.stringify({ error: message }));
        };
        const origin = request.headers.origin;
        if (request.headers["sec-fetch-site"] === "cross-site" ||
            (origin && new URL(origin).host !== request.headers.host)) {
          fail(403, "Same-origin authoring requests only"); return;
        }
        if (request.method === "GET") {
          response.end(JSON.stringify({ source: readFileSync(file, "utf8") })); return;
        }
        if (request.method !== "POST") { fail(405, "Use GET or POST"); return; }
        if (busy) { fail(409, "A layout save is already in progress"); return; }
        busy = true;
        const staging = `${file}.pending`;
        try {
          let body = "";
          for await (const chunk of request) {
            body += String(chunk);
            if (body.length > 128_000) { throw new Error("Layout request exceeds 128 KB"); }
          }
          const payload = JSON.parse(body) as { source?: unknown; expected?: unknown };
          const previous = readFileSync(file, "utf8");
          if (payload.expected !== previous) { fail(409, "The repository layout changed. Reopen it before saving."); return; }
          if (typeof payload.source !== "string") { throw new Error("Expected layout JSON source"); }
          writeFileSync(staging, payload.source);
          const check = spawnSync("cargo", ["run", "--locked", "-p", "mmorpg-scenery", "--example", "validate-layout", "--", staging],
            { cwd: resolve(webRoot, ".."), encoding: "utf8", timeout: 120_000 });
          if (check.error || check.status !== 0) {
            throw new Error(check.error?.message ?? check.stderr);
          }
          if (readFileSync(file, "utf8") !== previous) {
            fail(409, "The repository layout changed during validation. Reopen it before saving."); return;
          }
          renameSync(staging, file);
          try { buildWasm(); } catch (error) {
            writeFileSync(staging, previous); renameSync(staging, file); throw error;
          }
          for (const name of ["mmorpg_wasm.js", "mmorpg_wasm_bg.wasm"]) {
            for (const module of server.moduleGraph.getModulesByFile(resolve(generatedDirectory, name)) ?? []) {
              server.moduleGraph.invalidateModule(module);
            }
          }
          response.end(JSON.stringify({ source: payload.source }));
        } catch (error) {
          fail(400, error instanceof Error ? error.message : String(error));
        } finally {
          busy = false;
          try { unlinkSync(staging); } catch { /* No staging file after successful rename. */ }
        }
      });
    },
  };
}
