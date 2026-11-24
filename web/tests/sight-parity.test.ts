// Native-vs-browser parity (#57, #1150): the WASM build plays the same scripted Greyhaven
// sessions as `crates/mmorpg-wasm/tests/sight_parity.rs` and must digest the same bytes. The
// sight session casts a Firebolt that strikes a crate, then one the inn stops. The physical
// sessions of `crates/mmorpg-wasm/tests/parity-sessions.json` walk slopes, shoulder a boulder,
// breathe a cone, fight creatures hand to hand and walk two characters into each other; they
// digest every projection and the canonical snapshot after each tick. The native digests are
// computed in this run, by running those Rust tests with `MMORPG_SIGHT_DIGEST_OUT` and
// `MMORPG_PHYSICAL_DIGESTS_OUT`, never read from a committed file (#487).
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
const repository = fileURLToPath(new URL("../..", import.meta.url));
const native = runNativeSessions();

/** Runs the native parity tests; returns the sight digest and each physical session's. */
function runNativeSessions(): { sight: string; physical: Map<string, string> } {
  const directory = mkdtempSync(join(tmpdir(), "mmorpg-sight-"));
  const sight = join(directory, "digest.txt");
  const physical = join(directory, "physical.txt");
  try {
    const result = spawnSync("cargo", ["test", "--locked", "-p", "mmorpg-wasm", "--test", "sight_parity"], {
      cwd: repository,
      env: { ...process.env, MMORPG_SIGHT_DIGEST_OUT: sight, MMORPG_PHYSICAL_DIGESTS_OUT: physical },
      stdio: ["ignore", "ignore", "inherit"],
      timeout: 20 * 60 * 1000,
    });
    if (result.error || result.status !== 0) {
      throw new Error(`The native parity sessions failed: ${result.error?.message ?? `exit status ${result.status}`}.`);
    }
    const lines = readFileSync(physical, "utf8").trim().split("\n");
    return {
      sight: readFileSync(sight, "utf8").trim(),
      physical: new Map(lines.map((line) => line.split(" ") as [string, string])),
    };
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
const FNV_OFFSET = 0xcbf29ce484222325n;

function fnv(hash: bigint, bytes: Uint8Array): bigint {
  let value = hash;
  for (const byte of bytes) {
    value = BigInt.asUintN(64, (value ^ BigInt(byte)) * 0x100000001b3n);
  }
  return value;
}

const hex = (digest: bigint) => digest.toString(16).padStart(16, "0");

test("the WASM build reaches the native sight session digest", () => {
  const zone = new wasm.LocalZone();
  const { entries } = testCharacters();
  const player = zone.enter(entries, "local-1", 2, 0, 1n);
  let sequence = 0;
  const send = (command: WorldCommand) => {
    sequence += 1;
    expect(zone.submit(player, sequence, encodeCommand(command))).toBe(true);
  };
  let digest = FNV_OFFSET;
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
      send({ kind: "use-ability", ability: FIREBOLT, target: null, facing: null });
    }
    zone.tick();
    const projection = zone.projection(player);
    const snapshot = decodeSnapshot(projection);
    // The whole projection, broken props' debris included, as natively (#1109).
    digest = fnv(digest, projection);
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
  expect(native.sight).toMatch(/^[0-9a-f]{16}$/);
  expect(hex(digest)).toBe(native.sight);
});

/** `parity-sessions.json`; the native test checks what each session exercises. */
interface Session {
  name: string;
  world: string;
  lab?: { class: string; level: number };
  characters: { id: string; class: number; sex: number }[];
  ticks: number;
  steps: { tick: number; player: number; command: WorldCommand }[];
}

const script = JSON.parse(
  readFileSync(join(repository, "crates/mmorpg-wasm/tests/parity-sessions.json"), "utf8"),
) as { sessions: Session[] };

/** The session's world and its players, as `physical_session` in the Rust test builds them. */
function open(session: Session): { zone: InstanceType<typeof wasm.LocalZone>; players: number[] } {
  if (session.world === "greyhaven") {
    const zone = new wasm.LocalZone();
    const { entries } = testCharacters();
    return { zone, players: session.characters.map((character) => zone.enter(entries, character.id, character.class, character.sex, 1n)) };
  }
  const zone = session.lab
    ? wasm.LocalZone.forActionLab(session.lab.class, session.lab.level, "standard", 0, 0xffff, 0n)
    : wasm.LocalZone.forScenario(session.world, 0n);
  return { zone, players: [zone.scenarioPlayer!] };
}

for (const session of script.sessions) {
  test(`the WASM build reaches the native ${session.name} session digest`, () => {
    const { zone, players } = open(session);
    const sequences = players.map(() => 0);
    const text = new TextEncoder();
    let digest = FNV_OFFSET;
    for (let tick = 0; tick < session.ticks; tick += 1) {
      for (const step of session.steps.filter((candidate) => candidate.tick === tick)) {
        sequences[step.player]! += 1;
        expect(zone.submit(players[step.player]!, sequences[step.player]!, encodeCommand(step.command))).toBe(true);
      }
      zone.tick();
      for (const player of players) {
        digest = fnv(digest, zone.projection(player));
      }
      digest = fnv(digest, text.encode(zone.canonicalDigest()));
    }
    expect(native.physical.get(session.name)).toMatch(/^[0-9a-f]{16}$/);
    expect(hex(digest)).toBe(native.physical.get(session.name)!);
  });
}
