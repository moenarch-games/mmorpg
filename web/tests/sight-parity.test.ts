// Native-vs-browser line-of-sight parity (#57): the WASM build plays the same scripted
// Greyhaven session as `crates/mmorpg-wasm/tests/sight_parity.rs` (a Firebolt that strikes a
// crate, then one the inn stops) and must digest the same
// projection bytes. The native digest is computed in this run, by running that Rust test with
// `MMORPG_SIGHT_DIGEST_OUT`, never read from a committed file (#487).
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { encodeCommand, type WorldCommand } from "../src/command-wire";
import { decodeSnapshot } from "../src/replication";
import { testCharacters } from "./support/local-characters";
import { localZoneModule } from "./support/local-zone-module";

const wasm = await localZoneModule();
const nativeDigest = runNativeSession();

/** Runs the native sight session and returns its digest (16 hex digits). */
function runNativeSession(): string {
  const directory = mkdtempSync(join(tmpdir(), "mmorpg-sight-"));
  const out = join(directory, "digest.txt");
  try {
    const result = spawnSync(
      "cargo",
      ["test", "--locked", "-p", "mmorpg-wasm", "--test", "sight_parity", "--", "--exact",
        "the_sight_session_digest_is_deterministic_and_handed_to_the_browser_build"],
      {
        cwd: fileURLToPath(new URL("../..", import.meta.url)),
        env: { ...process.env, MMORPG_SIGHT_DIGEST_OUT: out },
        stdio: ["ignore", "ignore", "inherit"],
        timeout: 20 * 60 * 1000,
      },
    );
    if (result.error || result.status !== 0) {
      throw new Error(`The native sight session failed: ${result.error?.message ?? `exit status ${result.status}`}.`);
    }
    return readFileSync(out, "utf8").trim();
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

// The arcanist turns toward the crates before each Firebolt: missiles fly where it faces (#517).
const MOVES = new Map<number, [number, number]>([[0, [1, 24_576]], [160, [0, 13_725]], [290, [1, 24_576]], [340, [0, 11_649]]]);
const TARGETS = new Map<number, number>([[4, 901], [342, 900]]);
const INN_CAST = 344;
const CASTS = [162, INN_CAST];
const FIREBOLT = 9;
const TICKS = 520;
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
  let struck = 0;
  let blockedHits = 0;
  let started = 0;
  for (let tick = 0; tick < TICKS; tick += 1) {
    const move = MOVES.get(tick);
    if (move) {
      send({ kind: "move", forward: move[0], strafe: 0, facing: move[1] });
    }
    const prop = TARGETS.get(tick);
    if (prop !== undefined) {
      send({ kind: "select-target", target: { kind: "creature", id: prop } });
    }
    if (CASTS.includes(tick)) {
      send({ kind: "use-ability", ability: FIREBOLT, target: null });
    }
    zone.tick();
    const projection = zone.projection(player);
    const snapshot = decodeSnapshot(projection);
    // Broken props' debris (the trailing records) is left out, as natively: Rapier's dynamic
    // pieces are not bit-identical across targets and decide nothing.
    digest = fnv(digest, projection.subarray(0, projection.length - snapshot.debris.length * DEBRIS_BYTES));
    // Only the crates can take the missiles' damage: the first strikes one, the inn stops the second.
    const hits = snapshot.events.filter((event) => event.kind === "damage-dealt").length;
    if (tick < INN_CAST) {
      struck += hits;
    } else {
      blockedHits += hits;
    }
    started += snapshot.events.filter((event) => event.kind === "cast-started").length;
  }
  expect(started).toBe(2);
  expect(struck).toBeGreaterThan(0);
  expect(blockedHits).toBe(0);
  expect(nativeDigest).toMatch(/^[0-9a-f]{16}$/);
  expect(digest.toString(16).padStart(16, "0")).toBe(nativeDigest);
});
