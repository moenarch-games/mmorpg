/** Build-time configuration Vite inlines from `VITE_*` environment variables. */
interface ImportMetaEnv {
  /** A development zone host's admission route; "Play online" offers it (see scripts/dev-browser-online.sh). */
  readonly VITE_ZONE_SERVER?: string;
  /** The base64 SHA-256 hash of that host's self-signed development certificate. */
  readonly VITE_ZONE_CERT_HASH?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
