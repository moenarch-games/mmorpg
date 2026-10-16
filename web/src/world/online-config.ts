import { parseCertificateHash, parseSessionRoute, type SessionRoute } from "../session/route";

/** A zone host the page can join instead of its local zone. */
export type ZoneServer = {
  route: SessionRoute;
  /** The SHA-256 hash of a self-signed development certificate, or null for a publicly trusted one. */
  certificateHash: Uint8Array | null;
  /** `host:port`, for the page to name where it plays. */
  label: string;
};

/** Where the page may play, from its URL and build configuration. */
export type OnlineChoice =
  /** No zone host is configured: the page hosts its own local zone only. */
  | { kind: "local" }
  /** A zone host is configured; `preferred` when the page URL asked for it. */
  | { kind: "online"; server: ZoneServer; preferred: boolean }
  /** A configured zone host is invalid: online play fails closed with this message. */
  | { kind: "invalid"; message: string };

/** Build-time configuration, e.g. from `scripts/dev-browser-online.sh` through Vite env. */
export type OnlineEnvironment = { server?: string | undefined; certificateHash?: string | undefined };

function server(address: string, certificateHash: string | null): ZoneServer {
  const route = parseSessionRoute(address);
  return {
    route,
    certificateHash: certificateHash === null ? null : parseCertificateHash(certificateHash),
    label: new URL(route.url).host,
  };
}

function choose(address: string, certificateHash: string | null, preferred: boolean): OnlineChoice {
  try {
    return { kind: "online", server: server(address, certificateHash), preferred };
  } catch (error) {
    return { kind: "invalid", message: error instanceof Error ? error.message : String(error) };
  }
}

/**
 * Selects the zone host from the page query (`?server=<admission
 * route>&certHash=<base64 SHA-256>`) or, when the query names none, from
 * build configuration. The default is the local zone. The server must be a
 * canonical admission route without credentials; a resume route, which
 * carries a token, is refused like any other credential in a URL.
 */
export function onlineChoice(search: string, environment: OnlineEnvironment): OnlineChoice {
  const query = new URLSearchParams(search);
  const address = query.get("server");
  const certificateHash = query.get("certHash");
  if (address !== null) {
    return choose(address, certificateHash, true);
  }
  if (certificateHash !== null) {
    return { kind: "invalid", message: "The page names a certificate hash but no zone host (server=…)." };
  }
  if (environment.server) {
    return choose(environment.server, environment.certificateHash || null, false);
  }
  return { kind: "local" };
}
