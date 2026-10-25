// Native-vs-browser line-of-sight parity (#57): the WASM build plays the same scripted
// Greyhaven session as `crates/mmorpg-wasm/tests/sight_parity.rs` and must digest the same
// projection bytes the native build recorded in `fixtures/parity/greyhaven-sight-session.txt`.
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { encodeCommand, type WorldCommand } from "../src/command-wire";
import { decodeSnapshot } from "../src/replication";
import { testCharacters } from "./support/local-characters";
import { localZoneModule } from "./support/local-zone-module";

const wasm = await localZoneModule();

const MOVES = new Map<number, [number, number]>([[0, [1, 24_576]], [160, [0, 24_576]], [290, [1, 24_576]], [340, [0, 24_576]]]);
const TARGETS = new Map<number, number>([[4, 901], [342, 900]]);
const CASTS = new Set([162, 344, 380]);
const FIREBOLT = 9;
const TICKS = 400;
/** One debris record on the wire (`DEBRIS_RECORD_BYTES`). */
const DEBRIS_BYTES = 19;

function fnv(hash: bigint, bytes: Uint8Array): bigint {
  let value = hash;
  for (const byte of bytes) {
    value = BigInt.asUintN(64, (value ^ BigInt(byte)) * 0x100000001b3n);
  }
  return value;
}

test("the WASM build reaches the native sight session digest", () => {
  const zone = new wasm.LocalZone();
  const { entries } = testCharacters();
  const player = zone.enter(entries, "local-1", 2, 0, 1n);
  let sequence = 0;
  const send = (command: WorldCommand) => {
    sequence += 1;
    expect(zone.submit(player, sequence, encodeCommand(command))).toBe(true);
  };
  let digest = 0xcbf29ce484222325n;
  let refused = 0;
  for (let tick = 0; tick < TICKS; tick += 1) {
    const move = MOVES.get(tick);
    if (move) {
      send({ kind: "move", forward: move[0], strafe: 0, facing: move[1] });
    }
    const prop = TARGETS.get(tick);
    if (prop !== undefined) {
      send({ kind: "select-target", target: { kind: "creature", id: prop } });
    }
    if (CASTS.has(tick)) {
      send({ kind: "use-ability", ability: FIREBOLT, target: null });
    }
    zone.tick();
    const projection = zone.projection(player);
    const snapshot = decodeSnapshot(projection);
    // Broken props' debris (the trailing records) is left out, as natively: Rapier's dynamic
    // pieces are not bit-identical across targets and decide nothing.
    digest = fnv(digest, projection.subarray(0, projection.length - snapshot.debris.length * DEBRIS_BYTES));
    refused += snapshot.events.filter((event) => event.kind === "error" && event.code === "line-of-sight").length;
  }
  expect(refused).toBeGreaterThan(0);
  const recorded = readFileSync(new URL("../../fixtures/parity/greyhaven-sight-session.txt", import.meta.url), "utf8").trim();
  expect(digest.toString(16).padStart(16, "0")).toBe(recorded);
});
