/** Unit identity shared by commands and projections; `mmorpg-wasm` owns its wire form (#541). */
export type { EntityKind, EntityRef } from "./generated/mmorpg-wasm/mmorpg_wasm.js";
import type { EntityRef } from "./generated/mmorpg-wasm/mmorpg_wasm.js";

export function sameEntity(left: EntityRef | null, right: EntityRef | null): boolean {
  return left !== null && right !== null && left.kind === right.kind && left.id === right.id;
}

/** A stable map key such as `creature:108`. */
export function entityKey(entity: EntityRef): string {
  return `${entity.kind}:${entity.id}`;
}
