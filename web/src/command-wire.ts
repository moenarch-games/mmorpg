/**
 * Zone command datagrams (docs/PROTOCOL.md), encoded by `mmorpg_protocol` itself through
 * `mmorpg-wasm` (#541), which also generates the `WorldCommand` type. Load the module
 * (`loadWasm`) before encoding. The session supplies player identity and sequence separately.
 */
import type { WorldCommand } from "./generated/mmorpg-wasm/mmorpg_wasm.js";
import { wasm } from "./wasm";

export type { Axis, MinionOrder, StrikeAim, StrikeHeight, StrikeSide, WorldCommand } from "./generated/mmorpg-wasm/mmorpg_wasm.js";

/**
 * Commands whose timing the zone's action graph judges (#415): an online client sends them with
 * the latest zone tick it had observed, so a combo window that closed while they travelled can
 * still count, within the zone's bounded grace.
 */
export function isTimingSensitive(command: WorldCommand): boolean {
  return command.kind === "use-ability" || command.kind === "directed-strike";
}

/** Encodes `command`; throws for intent the zone's decoder would reject. */
export function encodeCommand(command: WorldCommand): Uint8Array {
  return wasm.encodeCommand(command);
}

/** `command` sent after observing the zone's completed tick `observed` (#415). */
export function encodeObservedCommand(command: WorldCommand, observed: bigint): Uint8Array {
  return wasm.encodeObservedCommand(command, observed);
}

/** Whether an encoded command is movement intent (Move or Rise). */
export function isMovementCommand(bytes: Uint8Array): boolean {
  return wasm.isMovementCommand(bytes);
}
