import { readFileSync } from "node:fs";
import { defineConfig } from "vite";

const certificate = process.env.MMORPG_DEV_WEB_HTTPS_CERT;
const privateKey = process.env.MMORPG_DEV_WEB_HTTPS_KEY;
if (Boolean(certificate) !== Boolean(privateKey)) {
  throw new Error("Browser HTTPS requires both a certificate and private key");
}

export default defineConfig({
  base: "/mmorpg/",
  server: {
    https: certificate && privateKey
      ? { cert: readFileSync(certificate), key: readFileSync(privateKey) }
      : undefined,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
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
