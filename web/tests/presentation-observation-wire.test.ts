/** Same independent semantic fixture as Rust/native decoder acceptance.
 * Fixture bytes are produced once after the owned wire migration is chosen;
 * acceptance fixes the facts rather than preselecting record tags/layout.
 */
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { decodeSnapshot } from "../src/replication";

test("new observation fixture decodes the same source/target/outcome in browser and Rust", () => {
  const hex = readFileSync(new URL("../../fixtures/protocol/presentation-observation-v23.hex", import.meta.url), "utf8").trim();
  const bytes = Uint8Array.from(Buffer.from(hex, "hex"));
  expect(decodeSnapshot(bytes).events).toEqual([
    { kind: "attack-performed", source: { kind: "player", id: 2 }, target: { kind: "creature", id: 101 } },
    { kind: "damage-observed", source: { kind: "player", id: 2 }, target: { kind: "creature", id: 101 }, amount: 7, critical: true },
  ]);
  for (let length = 0; length < bytes.length; length++) {
    expect(() => decodeSnapshot(bytes.slice(0, length))).toThrow();
  }
  const oldHex = readFileSync(new URL("../../fixtures/protocol/player-snapshot-v14.hex", import.meta.url), "utf8").trim();
  expect(() => decodeSnapshot(Uint8Array.from(Buffer.from(oldHex, "hex")))).toThrow("version");
});
