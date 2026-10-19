import { readFileSync } from "node:fs";
import { layoutEditorPlugin } from "./scripts/layout-editor-plugin";
import { defineConfig, searchForWorkspaceRoot } from "vite";

const certificate = process.env.MMORPG_DEV_WEB_HTTPS_CERT;
const privateKey = process.env.MMORPG_DEV_WEB_HTTPS_KEY;
if (Boolean(certificate) !== Boolean(privateKey)) {
  throw new Error("Browser HTTPS requires both a certificate and private key");
}

export default defineConfig({
  base: "/mmorpg/",
  plugins: [layoutEditorPlugin()],
  server: {
    // Authoring saves rebuild WASM explicitly. Keep the designer's page and selection
    // alive; the save endpoint invalidates modules for the next game-page request.
    watch: process.env.MMORPG_LAYOUT_EDITOR === "1"
      ? { ignored: ["**/src/generated/**"] }
      : {},
    // Quaternius unit models are served from the repository's shared `assets/quaternius`.
    fs: { allow: [searchForWorkspaceRoot(process.cwd()), "../assets/quaternius"] },
    https: certificate && privateKey
      ? { cert: readFileSync(certificate), key: readFileSync(privateKey) }
      : undefined,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: { game: "index.html", editor: "editor.html" },
      output: {
        manualChunks(id) {
          if (id.endsWith("/three/build/three.core.js")) return "three-core";
          if (id.includes("/node_modules/three/")) return "three";
          if (id.includes("/node_modules/@moritzbrantner/three-d-renderer/")) return "renderer";
        },
      },
    },
  },
});
