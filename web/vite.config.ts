import { cpSync, mkdirSync, readFileSync } from "node:fs";
import { defineConfig, searchForWorkspaceRoot } from "vite";

// Use the transcoder shipped with the pinned Three.js KTX2Loader.
mkdirSync(new URL("./public/basis/", import.meta.url), { recursive: true });
for (const name of ["basis_transcoder.js", "basis_transcoder.wasm"]) {
  cpSync(new URL(`./node_modules/three/examples/jsm/libs/basis/${name}`, import.meta.url), new URL(`./public/basis/${name}`, import.meta.url));
}

// render-engine's tone-mapping LUTs, which the three.js lighting loads at run time (#600).
mkdirSync(new URL("./public/luts/", import.meta.url), { recursive: true });
for (const name of ["tony_mc_mapface.ktx2", "AgX-default_contrast.ktx2"]) {
  cpSync(new URL(`./node_modules/@moritzbrantner/three-lighting/src/luts/${name}`, import.meta.url), new URL(`./public/luts/${name}`, import.meta.url));
}

const certificate = process.env.MMORPG_DEV_WEB_HTTPS_CERT;
const privateKey = process.env.MMORPG_DEV_WEB_HTTPS_KEY;
if (Boolean(certificate) !== Boolean(privateKey)) {
  throw new Error("Browser HTTPS requires both a certificate and private key");
}

export default defineConfig({
  base: "/mmorpg/",
  server: {
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
      input: { game: "index.html", rangerLoader: "ranger-loader.html" },
      output: {
        manualChunks(id) {
          if (id.endsWith("/three/build/three.core.js")) return "three-core";
          if (id.includes("/node_modules/three/")) return "three";
          if (id.includes("/node_modules/@moritzbrantner/three-d-renderer/")) return "renderer";
          if (id.includes("/node_modules/@moritzbrantner/three-lighting/")) return "renderer";
        },
      },
    },
  },
});
