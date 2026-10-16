import {
  MAX_SNAPSHOT_FRAGMENTS,
  MAX_SNAPSHOT_FRAME_BYTES,
  SessionProtocolError,
  decodeSnapshotDatagram,
  decodeSnapshotFrame,
  type SnapshotFragment,
  type SnapshotFrame,
} from "./frames";

/** Incomplete snapshots kept at once; the oldest is evicted for a newer one. */
export const SNAPSHOT_REASSEMBLY_MAX_PENDING = 4;
/** Chunk bytes buffered across all incomplete snapshots: two maximum-sized frames. */
export const SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES = 2 * MAX_SNAPSHOT_FRAME_BYTES;
/** Datagrams an incomplete snapshot may go without storing a new fragment before it is dropped. */
export const SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS = SNAPSHOT_REASSEMBLY_MAX_PENDING * MAX_SNAPSHOT_FRAGMENTS;

/** Deterministic counters describing what a reassembler did (`game_server::SnapshotReassemblyStats`). */
export type SnapshotReassemblyStats = {
  wholeSnapshots: number;
  reassembledSnapshots: number;
  bufferedFragments: number;
  staleDatagrams: number;
  duplicateFragments: number;
  evictedSnapshots: number;
  supersededSnapshots: number;
  expiredSnapshots: number;
  rejectedDatagrams: number;
};

type PendingSnapshot = {
  chunks: (Uint8Array | null)[];
  received: number;
  bytes: number;
  /** The reassembler's datagram count when a fragment was last stored. */
  lastStoredAt: number;
};

/**
 * Turns received snapshot datagrams back into verified snapshot frames: a
 * port of the pinned `game_server::SnapshotReassembler` with the same bounds,
 * outcomes and counters. Delivered ticks strictly increase, so a late or
 * reordered snapshot never replaces a newer one. Use one per connection.
 *
 * Memory is bounded by `SNAPSHOT_REASSEMBLY_MAX_PENDING` incomplete snapshots
 * and `SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES` chunk bytes; the oldest
 * incomplete snapshot is dropped first, and one that stores nothing for
 * `SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS` datagrams expires. Malformed or
 * inconsistent datagrams throw `SessionProtocolError` and leave it usable.
 */
export class SnapshotReassembler {
  /** Incomplete snapshots by tick; at most `SNAPSHOT_REASSEMBLY_MAX_PENDING`. */
  readonly #pending = new Map<bigint, PendingSnapshot>();
  #bufferedBytes = 0;
  #newestTick: bigint | null = null;
  /** Datagrams passed to `accept`: the deterministic clock for idle expiry. */
  #datagrams = 0;
  readonly #stats: SnapshotReassemblyStats = {
    wholeSnapshots: 0,
    reassembledSnapshots: 0,
    bufferedFragments: 0,
    staleDatagrams: 0,
    duplicateFragments: 0,
    evictedSnapshots: 0,
    supersededSnapshots: 0,
    expiredSnapshots: 0,
    rejectedDatagrams: 0,
  };

  /** A whole or completed snapshot, or `null` when the datagram was buffered or ignored. */
  accept(datagram: Uint8Array): SnapshotFrame | null {
    this.#datagrams += 1;
    this.#expireIdle();
    try {
      const decoded = decodeSnapshotDatagram(datagram);
      return decoded.kind === "snapshot" ? this.#acceptWhole(decoded.frame) : this.#acceptFragment(decoded.fragment);
    } catch (error) {
      this.#stats.rejectedDatagrams += 1;
      throw error;
    }
  }

  get newestTick(): bigint | null {
    return this.#newestTick;
  }

  get pendingSnapshots(): number {
    return this.#pending.size;
  }

  get bufferedBytes(): number {
    return this.#bufferedBytes;
  }

  /** Pending ticks in ascending order, for tests of the eviction policy. */
  pendingTicks(): bigint[] {
    return [...this.#pending.keys()].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  }

  stats(): SnapshotReassemblyStats {
    return { ...this.#stats };
  }

  #acceptWhole(frame: SnapshotFrame): SnapshotFrame | null {
    if (this.#isStale(frame.tick)) {
      this.#stats.staleDatagrams += 1;
      return null;
    }
    this.#stats.wholeSnapshots += 1;
    return this.#deliver(frame);
  }

  #acceptFragment(fragment: SnapshotFragment): SnapshotFrame | null {
    const { tick, index, count, chunk } = fragment;
    if (this.#isStale(tick)) {
      this.#stats.staleDatagrams += 1;
      return null;
    }
    const existing = this.#pending.get(tick);
    if (existing) {
      if (existing.chunks.length !== count) {
        throw new SessionProtocolError(`snapshot fragments for tick ${tick} are inconsistent`);
      }
      if (existing.chunks[index] !== null) {
        this.#stats.duplicateFragments += 1;
        return null;
      }
      const frameBytes = existing.bytes + chunk.byteLength;
      if (frameBytes > MAX_SNAPSHOT_FRAME_BYTES) {
        // No valid snapshot frame is this large; the tick cannot complete.
        this.#removePending(tick);
        throw new SessionProtocolError(`payload size ${frameBytes} exceeds maximum ${MAX_SNAPSHOT_FRAME_BYTES}`);
      }
    } else if (this.#pending.size >= SNAPSHOT_REASSEMBLY_MAX_PENDING && !this.#evictOldestBefore(tick)) {
      this.#stats.evictedSnapshots += 1;
      return null;
    }

    while (this.#bufferedBytes + chunk.byteLength > SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES) {
      if (!this.#evictOldestBefore(tick)) {
        // Every other buffered snapshot is newer: drop this one instead.
        this.#removePending(tick);
        this.#stats.evictedSnapshots += 1;
        return null;
      }
    }

    let pending = this.#pending.get(tick);
    if (!pending) {
      pending = { chunks: new Array<Uint8Array | null>(count).fill(null), received: 0, bytes: 0, lastStoredAt: 0 };
      this.#pending.set(tick, pending);
    }
    pending.chunks[index] = chunk.slice();
    pending.received += 1;
    pending.bytes += chunk.byteLength;
    pending.lastStoredAt = this.#datagrams;
    this.#bufferedBytes += chunk.byteLength;
    this.#stats.bufferedFragments += 1;
    if (pending.received < count) {
      return null;
    }

    const complete = this.#removePending(tick);
    if (!complete) {
      return null;
    }
    const frameBytes = new Uint8Array(complete.bytes);
    let offset = 0;
    for (const part of complete.chunks) {
      if (part) {
        frameBytes.set(part, offset);
        offset += part.byteLength;
      }
    }
    const frame = decodeSnapshotFrame(frameBytes);
    if (frame.tick !== tick) {
      throw new SessionProtocolError(`snapshot fragments for tick ${tick} are inconsistent`);
    }
    this.#stats.reassembledSnapshots += 1;
    return this.#deliver(frame);
  }

  #isStale(tick: bigint): boolean {
    return this.#newestTick !== null && tick <= this.#newestTick;
  }

  /** Delivers `frame` and drops every incomplete snapshot at the same or an older tick. */
  #deliver(frame: SnapshotFrame): SnapshotFrame {
    for (const [tick, pending] of [...this.#pending]) {
      if (tick <= frame.tick) {
        this.#pending.delete(tick);
        this.#bufferedBytes -= pending.bytes;
        this.#stats.supersededSnapshots += 1;
      }
    }
    this.#newestTick = frame.tick;
    return frame;
  }

  /** Drops incomplete snapshots that stored no fragment during the idle bound. */
  #expireIdle(): void {
    for (const [tick, pending] of [...this.#pending]) {
      if (this.#datagrams - pending.lastStoredAt > SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS) {
        this.#pending.delete(tick);
        this.#bufferedBytes -= pending.bytes;
        this.#stats.expiredSnapshots += 1;
      }
    }
  }

  /** Evicts the oldest pending snapshot if it is older than `tick`. */
  #evictOldestBefore(tick: bigint): boolean {
    const oldest = this.pendingTicks()[0];
    if (oldest === undefined || oldest >= tick) {
      return false;
    }
    this.#removePending(oldest);
    this.#stats.evictedSnapshots += 1;
    return true;
  }

  #removePending(tick: bigint): PendingSnapshot | null {
    const removed = this.#pending.get(tick);
    if (!removed) {
      return null;
    }
    this.#pending.delete(tick);
    this.#bufferedBytes -= removed.bytes;
    return removed;
  }
}
