/** Battle Royale protocol/schema/ruleset 2, from battle-royale #18 at 5390ed8. */
import { encodeCommand, type WorldCommand } from "../../command-wire";
import { decodeSnapshot, type ZoneSnapshot } from "../../replication";
import { classChoiceCodes, type JoinCharacter } from "../world-source";

export const ARENA_CONTENT_REVISION = 2n;
export const ARENA_ZONE_ID = 100;
const PHASES = ["waiting", "drop", "combat", "finished"] as const;
const CAUSES = [null, "storm", "session-expired", "combat"] as const;
export type MatchPhase = (typeof PHASES)[number];
export type MatchPlayer = {
  id: number;
  alive: boolean;
  eliminationTick: bigint | null;
  cause: (typeof CAUSES)[number];
};
export type MatchProjection = {
  matchId: bigint;
  tick: bigint;
  phase: MatchPhase;
  phaseStartedTick: bigint;
  combatStartedTick: bigint | null;
  lobbyReadyTick: bigint | null;
  storm: { x: number; z: number; radius: number; active: boolean };
  winner: number | null;
  acknowledgedSequence: number;
  players: readonly MatchPlayer[];
  zone: ZoneSnapshot;
};
/** Main hand, off hand, head, chest, legs, feet. IDs are validated by the host catalog. */
export type ArenaEquipment = readonly [number | null, number | null, number | null, number | null, number | null, number | null];

class Reader {
  readonly #bytes: Uint8Array;
  readonly #view: DataView;
  #offset = 0;
  constructor(bytes: Uint8Array) {
    this.#bytes = bytes;
    this.#view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }
  #take(size: number): number {
    if (this.#offset + size > this.#bytes.byteLength) {
      throw new Error("Truncated Battle Royale projection.");
    }
    const offset = this.#offset;
    this.#offset += size;
    return offset;
  }
  u8(): number { return this.#view.getUint8(this.#take(1)); }
  u16(): number { return this.#view.getUint16(this.#take(2), true); }
  u32(): number { return this.#view.getUint32(this.#take(4), true); }
  i32(): number { return this.#view.getInt32(this.#take(4), true); }
  u64(): bigint { return this.#view.getBigUint64(this.#take(8), true); }
  bool(): boolean {
    const value = this.u8();
    if (value > 1) { throw new Error("Invalid Battle Royale boolean."); }
    return value === 1;
  }
  optionalTick(): bigint | null { return this.bool() ? this.u64() : null; }
  optionalPlayer(): number | null { return this.bool() ? this.u32() : null; }
  bytes(size: number): Uint8Array {
    const offset = this.#take(size);
    return this.#bytes.subarray(offset, offset + size);
  }
  finish(): void {
    if (this.#offset !== this.#bytes.byteLength) { throw new Error("Trailing Battle Royale projection bytes."); }
  }
}

export function decodeMatchProjection(bytes: Uint8Array): MatchProjection {
  const reader = new Reader(bytes);
  if (new TextDecoder().decode(reader.bytes(4)) !== "BRPL" || reader.u16() !== 2 || reader.u16() !== 2 || reader.u64() !== 2n) {
    throw new Error("Unsupported Battle Royale projection version.");
  }
  const matchId = reader.u64();
  const tick = reader.u64();
  const phase = PHASES[reader.u8()];
  if (phase === undefined) { throw new Error("Unknown Battle Royale phase."); }
  const phaseStartedTick = reader.u64();
  const combatStartedTick = reader.optionalTick();
  const lobbyReadyTick = reader.optionalTick();
  const storm = { x: reader.i32(), z: reader.i32(), radius: reader.i32(), active: reader.bool() };
  const winner = reader.optionalPlayer();
  const acknowledgedSequence = reader.u32();
  const count = reader.u16();
  if (count > 100 || phaseStartedTick > tick || storm.radius < 0) { throw new Error("Invalid Battle Royale match state."); }
  const players: MatchPlayer[] = [];
  for (let index = 0; index < count; index += 1) {
    const id = reader.u32();
    const alive = reader.bool();
    const eliminationTick = reader.optionalTick();
    const cause = CAUSES[reader.u8()];
    if (cause === undefined || alive !== (eliminationTick === null) || (cause === null) !== alive ||
      (eliminationTick !== null && eliminationTick > tick) || (index > 0 && id <= players[index - 1]!.id)) {
      throw new Error("Invalid Battle Royale roster.");
    }
    players.push({ id, alive, eliminationTick, cause });
  }
  const zoneBytes = reader.u32();
  if (zoneBytes > 1024 * 1024) { throw new Error("Battle Royale zone projection is too large."); }
  const zone = decodeSnapshot(reader.bytes(zoneBytes));
  reader.finish();
  const viewer = players.find((player) => player.id === zone.viewerId);
  if (zone.zoneId !== ARENA_ZONE_ID || zone.contentRevision !== ARENA_CONTENT_REVISION || !viewer ||
    viewer.alive === zone.viewer.dead || zone.tick > tick || (phase !== "finished" && zone.tick !== tick)) {
    throw new Error("Battle Royale roster, content or clock disagrees with its character projection.");
  }
  return { matchId, tick, phase, phaseStartedTick, combatStartedTick, lobbyReadyTick, storm, winner,
    acknowledgedSequence, players, zone };
}

export function encodeCharacterCommand(command: WorldCommand): Uint8Array {
  const inner = encodeCommand(command);
  const bytes = new Uint8Array(3 + inner.byteLength);
  bytes.set([2, 0, 3]);
  bytes.set(inner, 3);
  return bytes;
}

export function encodeBuild(character: JoinCharacter, equipment: ArenaEquipment): Uint8Array {
  const { classId, sex } = classChoiceCodes(character);
  const bytes = new Uint8Array(17);
  bytes.set([2, 0, 2, classId, sex]);
  const view = new DataView(bytes.buffer);
  equipment.forEach((id, slot) => {
    if (id !== null && (!Number.isInteger(id) || id < 1 || id > 0xffff)) {
      throw new Error("Equipment IDs must be catalog item IDs.");
    }
    view.setUint16(5 + slot * 2, id ?? 0, true);
  });
  return bytes;
}

/** The mode refuses progression, gear changes and revival throughout an admitted life. */
export function canSendCharacterCommand(command: WorldCommand, phase: MatchPhase): boolean {
  if (phase === "finished") { return false; }
  switch (command.kind) {
    case "move":
    case "jump":
    case "roll":
    case "rise":
    case "select-target":
    case "chat":
    case "emote":
    // A release never acts by itself; it only ends a hold (#414).
    case "release-ability":
      return true;
    case "swing":
    case "use-ability":
    case "directed-strike":
    case "guard":
    case "cancel-cast":
    case "command-minions":
      return phase === "combat";
    case "loot":
    case "release-spirit":
    case "move-item":
    case "choose-class":
    case "equip-item":
    case "unequip-item":
    case "buy-item":
    case "sell-item":
    case "accept-quest":
    case "complete-quest":
    case "abandon-quest":
    case "use-door":
    case "repair":
      return false;
  }
}
