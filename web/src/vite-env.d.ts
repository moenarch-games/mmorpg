/** Build-time configuration Vite inlines from `VITE_*` environment variables. */
interface ImportMetaEnv {
  /** The deployed base path (`/mmorpg/`), where `web/public` files are served. */
  readonly BASE_URL: string;
  /** True in `vite build` output, false on the dev server. */
  readonly PROD: boolean;
  /** A development zone host's admission route; "Play online" offers it (see scripts/dev-browser-online.sh). */
  readonly VITE_ZONE_SERVER?: string;
  /** The base64 SHA-256 hash of that host's self-signed development certificate. */
  readonly VITE_ZONE_CERT_HASH?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
