/**
 * Player-visible projections only. Canonical recovery state never enters rendering.
 *
 * The wire belongs to Rust (#541): `mmorpg-wasm` decodes each projection with
 * `mmorpg_protocol::decode_snapshot` and hands it over in the shape below, whose types it
 * generates. Load the module (`loadWasm`) before decoding.
 */
import { sameEntity, type EntityRef } from "./entity-ref";
import type { ItemStack, RedirectNotice, WireCatalog, ZoneSnapshot } from "./generated/mmorpg-wasm/mmorpg_wasm.js";
import { wasm } from "./wasm";

export type {
  AuraKind, AuraState, CastState, ChatLine, ClassChoice, ClassId, ComboPhase, CooldownState, DebrisState, DoorState, EmoteName,
  EntityFlags, EntityKind, EntityRef, EntityState, ErrorCode, GuardState, ItemStack, LootView, MeleeStance, PieceStage, PieceState,
  ProjectileState, QuestEntry, QuestMarker, QuestMarkerKind, QuestSheet, RaidNotice, RaidSignal, RedirectNotice, ResourceKind, ResourceState,
  StatTotals, StrikeAim, StrikeHeight, StrikeSide, Traversal, Vector3, ViewerState, WireCatalog, ZoneEvent, ZoneSnapshot,
} from "./generated/mmorpg-wasm/mmorpg_wasm.js";
import type { EntityState } from "./generated/mmorpg-wasm/mmorpg_wasm.js";

/** A bag slot: a stack, or null when empty. */
export type InventorySlot = ItemStack | null;

/**
 * The authoritative simulation rate (`mmorpg_core::TICK_HZ`): the online handshake refuses another
 * rate and `createLocalWorld` refuses a WASM host at another one.
 */
export const TICK_HZ = 60;
/** Whole ticks in `ms` milliseconds, rounded to the nearest tick like `mmorpg_core::ticks_from_ms`. */
export function ticksFromMs(ms: number): number {
  return Math.floor((ms * TICK_HZ + 500) / 1_000);
}

export const UNITS_PER_METRE = 100;
const YAW_STEPS = 65_536;
const BUFFER_CAPACITY = 32;

let catalog: WireCatalog | null = null;

/** The wire facts the HUD presents (breath, cooldown and guard limits, emotes, strike lanes), from the core. */
export function wireCatalog(): WireCatalog {
  catalog ??= wasm.wireCatalog();
  return catalog;
}

/** Whether `quest` is turned in according to a completed mask (bit `quest - 1`). */
export function questCompleted(completed: number, quest: number): boolean {
  return quest >= 1 && quest <= 32 && ((completed >>> (quest - 1)) & 1) === 1;
}

/** Whether a session snapshot payload is a redirect notice rather than a player projection. */
export function isRedirectNotice(payload: Uint8Array): boolean {
  return wasm.isRedirectNotice(payload);
}

/**
 * What a departed character's session receives in place of its projection once the source zone
 * retired it (#145); strict like every Rust decoder.
 */
export function decodeRedirectNotice(payload: Uint8Array): RedirectNotice {
  return wasm.decodeRedirectNotice(payload);
}

/**
 * Decodes a player projection with the Rust decoder, without sheets received earlier; throws on
 * any malformed payload. A local host sends every sheet each tick; an online session decodes with
 * a `ProjectionDecoder`.
 */
export function decodeSnapshot(payload: Uint8Array): ZoneSnapshot {
  const snapshot = wasm.decodeSnapshot(payload);
  PROJECTION_BYTES.set(snapshot, payload.slice());
  return snapshot;
}

/**
 * One online session's projection decoder (#544). A zone host sends the bag, quest, loot and
 * ability sheets only until the client acknowledges them, so this keeps the newest copy of each
 * (in Rust) and every projection it decodes is complete. Every command the session sends goes
 * through `acknowledge`, which echoes the sheet revisions it holds.
 */
export class ProjectionDecoder {
  readonly #decoder = new wasm.SnapshotDecoder();

  /** Decodes a projection strictly; a malformed one leaves the held sheets as they were. */
  decode(payload: Uint8Array): ZoneSnapshot {
    const snapshot = this.#decoder.decode(payload);
    PROJECTION_BYTES.set(snapshot, payload.slice());
    return snapshot;
  }

  /** Encoded command bytes wrapped with the sheet revisions held. */
  acknowledge(command: Uint8Array): Uint8Array {
    return this.#decoder.acknowledge(command);
  }
}

/** The wire bytes each decoded projection came from, for the shared unit presenter (#184). */
const PROJECTION_BYTES = new WeakMap<ZoneSnapshot, Uint8Array>();

/** The bytes `snapshot` was decoded from; undefined for a projection not decoded here. */
export function projectionBytes(snapshot: ZoneSnapshot): Uint8Array | undefined {
  return PROJECTION_BYTES.get(snapshot);
}

/** The projection's record of `entity`, if it is visible. */
export function findEntity(snapshot: ZoneSnapshot, entity: EntityRef | null): EntityState | undefined {
  return snapshot.entities.find((candidate) => sameEntity({ kind: candidate.kind, id: candidate.entityId }, entity));
}

/** Nearest u16 yaw for a presentation angle in radians (0 faces +Z, turning toward +X). */
export function yawFromRadians(radians: number): number {
  const turns = radians / (2 * Math.PI);
  return Math.round((turns - Math.floor(turns)) * YAW_STEPS) % YAW_STEPS;
}

/** Interpolates yaw along the shorter arc; the result stays in [0, 65 536). */
function interpolateFacing(from: number, to: number, alpha: number): number {
  const half = YAW_STEPS / 2;
  const delta = ((to - from + YAW_STEPS + half) % YAW_STEPS) - half;
  const facing = (from + delta * alpha) % YAW_STEPS;
  return facing < 0 ? facing + YAW_STEPS : facing;
}

/** Bounded presentation history. It never extrapolates gameplay or runs physics. */
export class SnapshotBuffer {
  #snapshots: ZoneSnapshot[] = [];

  /** Call on reconnect, handoff, or an authority-epoch change before accepting a new stream. */
  reset(): void {
    this.#snapshots = [];
  }

  push(snapshot: ZoneSnapshot): boolean {
    const latest = this.#snapshots.at(-1);
    if (latest) {
      if (
        snapshot.zoneId !== latest.zoneId ||
        snapshot.contentRevision !== latest.contentRevision ||
        snapshot.viewerId !== latest.viewerId
      ) {
        throw new Error("Snapshot stream changed without reset");
      }
      if (snapshot.tick <= latest.tick) {
        return false;
      }
      // After a long stall, snap to current state instead of interpolating a stale journey.
      if (snapshot.tick - latest.tick > BigInt(TICK_HZ * 30)) {
        this.reset();
      }
    }
    this.#snapshots.push(snapshot);
    if (this.#snapshots.length > BUFFER_CAPACITY) {
      this.#snapshots.shift();
    }
    return true;
  }

  sample(tick: bigint, fraction = 0): readonly EntityState[] {
    if (!Number.isFinite(fraction) || fraction < 0 || fraction >= 1) {
      throw new Error("Render tick fraction must be in [0, 1)");
    }
    let before = this.#snapshots[0];
    if (!before) {
      return [];
    }
    if (tick < before.tick) {
      return before.entities;
    }
    for (const after of this.#snapshots.slice(1)) {
      if (tick < after.tick) {
        const alpha = (Number(tick - before.tick) + fraction) / Number(after.tick - before.tick);
        const identity = (entity: EntityState) => `${entity.kind}:${entity.entityId}`;
        const nextEntities = new Map(after.entities.map((entity) => [identity(entity), entity]));
        return before.entities.map((entity) => {
          const next = nextEntities.get(identity(entity));
          // Appearance/disappearance happens at the authoritative sample tick.
          if (!next) {
            return entity;
          }
          const interpolate = (axis: 0 | 1 | 2) =>
            entity.position[axis] + (next.position[axis] - entity.position[axis]) * alpha;
          return {
            ...entity,
            position: [interpolate(0), interpolate(1), interpolate(2)],
            facing: interpolateFacing(entity.facing, next.facing, alpha),
          };
        });
      }
      before = after;
    }
    return before.entities;
  }
}
