import { describe, expect, test } from "bun:test";
import { SessionRouteError, parseCertificateHash, parseSessionRoute, reconnectUrl } from "../src/session/route";

describe("zone host session routes", () => {
  test("accepts a canonical hosted-zone admission route", () => {
    expect(parseSessionRoute("https://127.0.0.1:4433/game/matches/zone-1")).toEqual({
      url: "https://127.0.0.1:4433/game/matches/zone-1",
      origin: "https://127.0.0.1:4433",
      prefix: "/game",
      matchId: "zone-1",
      zoneId: 1,
    });
    expect(parseSessionRoute("https://zones.example/custom/game/matches/zone-4294967295")).toMatchObject({
      prefix: "/custom/game", matchId: "zone-4294967295", zoneId: 4_294_967_295,
    });
  });

  test("refuses credentials, queries, fragments, resume routes and anything not canonical", () => {
    for (const invalid of [
      "",
      "not a url",
      "http://127.0.0.1:4433/game/matches/zone-1",
      "https://user:secret@127.0.0.1:4433/game/matches/zone-1",
      "https://user@127.0.0.1:4433/game/matches/zone-1",
      "https://127.0.0.1:4433/game/matches/zone-1?token=secret",
      "https://127.0.0.1:4433/game/matches/zone-1?",
      "https://127.0.0.1:4433/game/matches/zone-1#fragment",
      "https://127.0.0.1:4433/game/matches/zone-1/reconnect/00112233445566778899aabbccddeeff",
      "https://127.0.0.1:4433/game/matches/zone-1/",
      "https://127.0.0.1:4433/game/./matches/zone-1",
      "https://127.0.0.1:4433/g%61me/matches/zone-1",
      "https://127.0.0.1:4433/matches/zone-1",
      "https://127.0.0.1:4433/game//matches/zone-1",
      "https://127.0.0.1:4433/game/matches/",
      "https://127.0.0.1:4433/game/matches/uno_01",
      "https://127.0.0.1:4433/game/matches/zone-01",
      "https://127.0.0.1:4433/game/matches/zone-4294967296",
      "HTTPS://127.0.0.1:4433/game/matches/zone-1",
    ]) {
      expect(() => parseSessionRoute(invalid), invalid).toThrow(SessionRouteError);
    }
  });

  test("builds the resume route from the admission route and a 16-byte token", () => {
    const route = parseSessionRoute("https://localhost:4433/custom/game/matches/zone-2");
    const token = new Uint8Array(16).fill(0xab);
    expect(reconnectUrl(route, token)).toBe(`https://localhost:4433/custom/game/matches/zone-2/reconnect/${"ab".repeat(16)}`);
    expect(() => reconnectUrl(route, new Uint8Array(15))).toThrow("16 bytes");
  });
});

describe("development certificate hashes", () => {
  const digest = Uint8Array.from({ length: 32 }, (_, index) => (index * 37 + 250) & 0xff);
  const base64 = Buffer.from(digest).toString("base64");

  test("accepts base64, base64url and a query-decoded '+'", () => {
    expect(base64).toContain("+");
    expect(parseCertificateHash(base64)).toEqual(digest);
    expect(parseCertificateHash(Buffer.from(digest).toString("base64url"))).toEqual(digest);
    expect(parseCertificateHash(base64.replaceAll("+", " "))).toEqual(digest);
    expect(parseCertificateHash(`${base64}\r\n`)).toEqual(digest);
  });

  test("refuses anything but a 32-byte SHA-256 digest", () => {
    for (const invalid of ["", "!!!!", `${base64}=`, `=${base64}`, Buffer.from(digest.slice(1)).toString("base64"), Buffer.from([...digest, 1]).toString("base64")]) {
      expect(() => parseCertificateHash(invalid), invalid).toThrow(SessionRouteError);
    }
  });
});
