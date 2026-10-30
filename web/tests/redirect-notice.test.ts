import { expect, test } from "bun:test";
import { decodeRedirectNotice, isRedirectNotice } from "../src/replication";
import { PROTOCOL_ID_BYTES } from "../src/protocol-id";

/** A notice laid out like `mmorpg_protocol::encode_redirect_notice` (#145). */
function notice(route: string, transferId = 0xfeedn): Uint8Array {
  const text = new TextEncoder().encode(route);
  const bytes = new Uint8Array(33 + text.length);
  const view = new DataView(bytes.buffer);
  bytes.set(PROTOCOL_ID_BYTES, 0);
  bytes[3] = 3;
  view.setUint32(4, 901);
  view.setBigUint64(8, 1_234n);
  view.setUint32(16, 902);
  view.setBigUint64(20, transferId);
  view.setUint32(28, 17);
  bytes[32] = text.length;
  bytes.set(text, 33);
  return bytes;
}

test("a redirect notice decodes strictly", () => {
  const route = "https://127.0.0.1:4434/game/matches/zone-902";
  const payload = notice(route);
  expect(isRedirectNotice(payload)).toBe(true);
  expect(decodeRedirectNotice(payload)).toEqual({
    zoneId: 901, tick: 1_234n, destination: 902, transferId: 0xfeedn, cutoverSequence: 17, route,
  });
  expect(() => decodeRedirectNotice(payload.slice(0, -1))).toThrow();
  expect(() => decodeRedirectNotice(new Uint8Array([...payload, 0]))).toThrow();
  expect(() => decodeRedirectNotice(notice(route, 0n))).toThrow();
  const projection = payload.slice();
  projection[3] = 2;
  expect(isRedirectNotice(projection)).toBe(false);
});
