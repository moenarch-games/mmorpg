import { describe, expect, test } from "bun:test";
import { onlineChoice } from "../src/world/online-config";

const ROUTE = "https://127.0.0.1:4433/game/matches/zone-1";
const digest = Uint8Array.from({ length: 32 }, (_, index) => (index * 37 + 250) & 0xff);
const HASH = Buffer.from(digest).toString("base64");
const query = (fields: Record<string, string>) => `?${new URLSearchParams(fields)}`;

describe("online play selection", () => {
  test("defaults to the local zone", () => {
    expect(onlineChoice("", {})).toEqual({ kind: "local" });
    expect(onlineChoice("?debug", { server: "", certificateHash: "" })).toEqual({ kind: "local" });
  });

  test("the page query selects a zone host and its development certificate", () => {
    const choice = onlineChoice(query({ server: ROUTE, certHash: HASH, debug: "" }), {});
    expect(choice).toMatchObject({ kind: "online", preferred: true, server: { label: "127.0.0.1:4433", certificateHash: digest } });
    expect(choice.kind === "online" && choice.server.route).toMatchObject({ url: ROUTE, zoneId: 1 });
    // A raw "+" in a hand-written query decodes to a space; the hash survives it.
    expect(HASH).toContain("+");
    expect(onlineChoice(`?server=${ROUTE}&certHash=${HASH}`, {})).toMatchObject({ kind: "online", server: { certificateHash: digest } });
    expect(onlineChoice(query({ server: ROUTE }), {})).toMatchObject({ kind: "online", server: { certificateHash: null } });
  });

  test("build configuration offers a zone host without preferring it, and the query wins", () => {
    expect(onlineChoice("", { server: ROUTE, certificateHash: HASH })).toMatchObject({ kind: "online", preferred: false });
    const other = "https://zones.example/game/matches/zone-2";
    expect(onlineChoice(query({ server: other }), { server: ROUTE, certificateHash: HASH }))
      .toMatchObject({ kind: "online", preferred: true, server: { label: "zones.example", certificateHash: null } });
  });

  test("invalid configuration and credentials in the URL fail closed instead of falling back", () => {
    for (const search of [
      query({ server: "https://user:secret@127.0.0.1:4433/game/matches/zone-1" }),
      query({ server: `${ROUTE}/reconnect/00112233445566778899aabbccddeeff` }),
      query({ server: "http://127.0.0.1:4433/game/matches/zone-1" }),
      query({ server: ROUTE, certHash: "not-a-hash" }),
      query({ certHash: HASH }),
    ]) {
      expect(onlineChoice(search, {}), search).toMatchObject({ kind: "invalid" });
    }
    expect(onlineChoice("", { server: "https://127.0.0.1:4433/elsewhere" })).toMatchObject({ kind: "invalid" });
  });
});
