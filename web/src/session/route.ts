import { RECONNECT_TOKEN_BYTES } from "./frames";

/**
 * A hosted zone's WebTransport session route, validated like the native
 * client's `SessionRoute` against game-server's browser route contract:
 * `https://<host>[:port]<prefix>/matches/<match id>`. Credentials, queries and
 * fragments are refused, and so is a reconnect route: resume tokens are a
 * credential the source keeps in memory, never something a page URL carries.
 */
export type SessionRoute = {
  /** The canonical admission URL for a new player. */
  url: string;
  origin: string;
  prefix: string;
  matchId: string;
  /** The zone the match hosts, from the MMO's `zone-<id>` match naming. */
  zoneId: number;
};

const MATCH_SEGMENT = "/matches/";
const RECONNECT_SEGMENT = "reconnect";
const MAX_MATCH_ID_BYTES = 64;
const ROUTE_SEGMENT = /^[A-Za-z0-9_-]+$/;
const ZONE_MATCH_ID = /^zone-(0|[1-9][0-9]{0,9})$/;
const SHA256_BYTES = 32;
const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;

export class SessionRouteError extends Error {
  override readonly name = "SessionRouteError";
}

/** `game_server::BrowserRoutePrefix::new`: canonical absolute segments of letters, digits, `-` or `_`. */
function validPrefix(prefix: string): boolean {
  return prefix.length >= 2 && prefix.startsWith("/") && !prefix.endsWith("/") &&
    prefix.slice(1).split("/").every((segment) => ROUTE_SEGMENT.test(segment));
}

export function parseSessionRoute(value: string): SessionRoute {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new SessionRouteError("The zone host address is not a valid URL.");
  }
  if (url.protocol !== "https:" || url.hostname === "") {
    throw new SessionRouteError("The zone host address must be an https:// URL.");
  }
  if (url.username !== "" || url.password !== "" || value.includes("?") || value.includes("#")) {
    throw new SessionRouteError("The zone host address must not carry credentials, a query or a fragment.");
  }
  // Parsing normalises dot segments, percent-escapes and case; the route must already be canonical.
  if (url.href !== value) {
    throw new SessionRouteError(`The zone host address must be canonical: ${url.href}`);
  }
  const split = url.pathname.lastIndexOf(MATCH_SEGMENT);
  if (split < 0) {
    throw new SessionRouteError("The zone host address must name a hosted match (…/matches/zone-<id>).");
  }
  const prefix = url.pathname.slice(0, split);
  const matchId = url.pathname.slice(split + MATCH_SEGMENT.length);
  if (!validPrefix(prefix)) {
    throw new SessionRouteError("The zone host route prefix must be canonical segments of letters, digits, '-' or '_'.");
  }
  if (matchId.length > MAX_MATCH_ID_BYTES || !ROUTE_SEGMENT.test(matchId)) {
    throw new SessionRouteError("The zone host address must end with a match ID, not a longer route.");
  }
  const zone = ZONE_MATCH_ID.exec(matchId);
  const zoneId = zone ? Number(zone[1]) : Number.NaN;
  if (!Number.isInteger(zoneId) || zoneId > 0xffff_ffff) {
    throw new SessionRouteError("The zone host match must be a zone (zone-<id>).");
  }
  return { url: url.href, origin: url.origin, prefix, matchId, zoneId };
}

/** `BrowserRoutePrefix::reconnect_path` under the route's origin. */
export function reconnectUrl(route: SessionRoute, token: Uint8Array): string {
  if (token.byteLength !== RECONNECT_TOKEN_BYTES) {
    throw new SessionRouteError("A reconnect token has 16 bytes.");
  }
  const hex = Array.from(token, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${route.origin}${route.prefix}${MATCH_SEGMENT}${route.matchId}/${RECONNECT_SEGMENT}/${hex}`;
}

/**
 * The SHA-256 hash of a development server certificate, as base64 (or
 * base64url). A `+` that a query string decoded to a space is restored, so
 * spaces are never trimmed; line breaks from a pasted value are dropped.
 */
export function parseCertificateHash(value: string): Uint8Array {
  const normalised = value.replace(/[\t\r\n]/g, "").replaceAll(" ", "+").replaceAll("-", "+").replaceAll("_", "/");
  if (!BASE64.test(normalised)) {
    throw new SessionRouteError("The certificate hash must be base64.");
  }
  let decoded: string;
  try {
    decoded = atob(normalised.padEnd(Math.ceil(normalised.length / 4) * 4, "="));
  } catch {
    throw new SessionRouteError("The certificate hash must be base64.");
  }
  if (decoded.length !== SHA256_BYTES) {
    throw new SessionRouteError("The certificate hash must be a 32-byte SHA-256 digest.");
  }
  return Uint8Array.from(decoded, (character) => character.charCodeAt(0));
}
