import { describe, expect, test } from "bun:test";
import { MAX_SNAPSHOT_PAYLOAD_BYTES, SESSION_PROTOCOL_VERSION, decodeSnapshotFrame } from "../src/session/frames";
import {
  SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES,
  SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS,
  SNAPSHOT_REASSEMBLY_MAX_PENDING,
  SnapshotReassembler,
} from "../src/session/snapshot-reassembler";
import { encodeSnapshotFragments, encodeSnapshotFrame, rawFragment } from "./support/session-host-frames";

// Ports of the pinned game-server's `reassembly.rs` tests: same bounds, outcomes and counters.

function frame(tick: number, payloadLength: number): Uint8Array {
  const payload = Uint8Array.from({ length: payloadLength }, (_, index) => (index * 31 + tick) & 0xff);
  return encodeSnapshotFrame(BigInt(tick), payload);
}

const fragments = (tick: number, payloadLength: number, budget: number) =>
  encodeSnapshotFragments(frame(tick, payloadLength), budget);

function assertBounded(reassembler: SnapshotReassembler): void {
  expect(reassembler.pendingSnapshots).toBeLessThanOrEqual(SNAPSHOT_REASSEMBLY_MAX_PENDING);
  expect(reassembler.bufferedBytes).toBeLessThanOrEqual(SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES);
  expect(reassembler.bufferedBytes).toBeGreaterThanOrEqual(0);
}

describe("snapshot reassembly", () => {
  test("whole snapshots are delivered in strictly increasing tick order", () => {
    const reassembler = new SnapshotReassembler();
    expect(reassembler.accept(frame(1, 8))).toEqual(decodeSnapshotFrame(frame(1, 8)));
    expect(reassembler.accept(frame(1, 8))).toBeNull();
    expect(reassembler.accept(frame(3, 8))?.tick).toBe(3n);
    expect(reassembler.accept(frame(2, 8))).toBeNull();
    expect(reassembler.newestTick).toBe(3n);
    expect(reassembler.stats()).toMatchObject({ wholeSnapshots: 2, staleDatagrams: 2 });
  });

  test("in-order fragments reassemble the exact snapshot", () => {
    const original = frame(7, 5_000);
    const datagrams = encodeSnapshotFragments(original, 1_200);
    const reassembler = new SnapshotReassembler();
    for (const datagram of datagrams.slice(0, -1)) {
      expect(reassembler.accept(datagram)).toBeNull();
      assertBounded(reassembler);
    }
    expect(reassembler.pendingSnapshots).toBe(1);
    expect(reassembler.accept(datagrams.at(-1)!)).toEqual(decodeSnapshotFrame(original));
    expect(reassembler.pendingSnapshots).toBe(0);
    expect(reassembler.bufferedBytes).toBe(0);
    expect(reassembler.stats()).toEqual({
      wholeSnapshots: 0, reassembledSnapshots: 1, bufferedFragments: datagrams.length, staleDatagrams: 0,
      duplicateFragments: 0, evictedSnapshots: 0, supersededSnapshots: 0, expiredSnapshots: 0, rejectedDatagrams: 0,
    });
  });

  test("reordered and duplicated fragments reassemble once", () => {
    const original = frame(9, 4_000);
    const datagrams = encodeSnapshotFragments(original, 900);
    expect(datagrams.length).toBeGreaterThanOrEqual(4);
    const reassembler = new SnapshotReassembler();
    const order = datagrams.map((_, index) => index).reverse();
    order.splice(1, 0, order[0]!);
    const last = order.pop()!;
    for (const index of order) {
      expect(reassembler.accept(datagrams[index]!)).toBeNull();
    }
    expect(reassembler.accept(datagrams[last]!)).toEqual(decodeSnapshotFrame(original));
    expect(reassembler.accept(datagrams[0]!)).toBeNull();
    expect(reassembler.stats()).toMatchObject({ reassembledSnapshots: 1, duplicateFragments: 1, staleDatagrams: 1 });
  });

  test("a lost fragment is superseded by a newer snapshot", () => {
    const lossy = fragments(4, 3_000, 1_000);
    const reassembler = new SnapshotReassembler();
    for (const datagram of lossy.slice(1)) {
      expect(reassembler.accept(datagram)).toBeNull();
    }
    for (const datagram of fragments(5, 3_000, 1_000)) {
      const delivered = reassembler.accept(datagram);
      if (delivered) expect(delivered.tick).toBe(5n);
    }
    expect(reassembler.newestTick).toBe(5n);
    expect(reassembler.pendingSnapshots).toBe(0);
    expect(reassembler.bufferedBytes).toBe(0);
    expect(reassembler.accept(lossy[0]!)).toBeNull();
    expect(reassembler.stats()).toMatchObject({ supersededSnapshots: 1, staleDatagrams: 1, reassembledSnapshots: 1 });
  });

  test("a whole snapshot supersedes pending fragments of older and equal ticks", () => {
    const reassembler = new SnapshotReassembler();
    for (const tick of [1, 2, 3]) {
      reassembler.accept(fragments(tick, 3_000, 1_000)[0]!);
    }
    expect(reassembler.accept(frame(2, 8))?.tick).toBe(2n);
    expect(reassembler.pendingTicks()).toEqual([3n]);
    expect(reassembler.stats().supersededSnapshots).toBe(2);
    assertBounded(reassembler);
  });

  test("the pending-snapshot bound keeps the newest ticks", () => {
    const reassembler = new SnapshotReassembler();
    const byTick = [1, 2, 3, 4, 5, 6].map((tick) => fragments(tick, 3_000, 1_000));
    for (const tickFragments of byTick.slice(1, 5)) {
      reassembler.accept(tickFragments[0]!);
    }
    expect(reassembler.pendingSnapshots).toBe(SNAPSHOT_REASSEMBLY_MAX_PENDING);
    // Tick 6 evicts tick 2, the oldest pending snapshot.
    reassembler.accept(byTick[5]![0]!);
    expect(reassembler.pendingTicks()).toEqual([3n, 4n, 5n, 6n]);
    // Tick 1 is older than every pending snapshot, so it is refused.
    expect(reassembler.accept(byTick[0]![0]!)).toBeNull();
    expect(reassembler.pendingTicks()).toEqual([3n, 4n, 5n, 6n]);
    expect(reassembler.stats().evictedSnapshots).toBe(2);
    let delivered = null;
    for (const datagram of byTick[5]!.slice(1)) {
      delivered = reassembler.accept(datagram) ?? delivered;
    }
    expect(delivered?.tick).toBe(6n);
    expect(reassembler.pendingSnapshots).toBe(0);
    expect(reassembler.bufferedBytes).toBe(0);
    expect(reassembler.stats().supersededSnapshots).toBe(3);
  });

  test("the buffered-byte bound evicts older snapshots first", () => {
    const big = [1, 2, 3].map((tick) => fragments(tick, MAX_SNAPSHOT_PAYLOAD_BYTES, 60_000));
    expect(big.every((datagrams) => datagrams.length === 2)).toBe(true);
    const refusing = new SnapshotReassembler();
    refusing.accept(big[1]![0]!);
    refusing.accept(big[2]![0]!);
    // A third large chunk does not fit beside two others; tick 1 is oldest and refused.
    expect(refusing.accept(big[0]![0]!)).toBeNull();
    expect(refusing.pendingTicks()).toEqual([2n, 3n]);
    assertBounded(refusing);
    expect(refusing.accept(big[1]![1]!)?.tick).toBe(2n);
    expect(refusing.accept(big[2]![1]!)?.tick).toBe(3n);
    expect(refusing.bufferedBytes).toBe(0);
    expect(refusing.stats().evictedSnapshots).toBe(1);

    const evicting = new SnapshotReassembler();
    for (const datagrams of big) {
      evicting.accept(datagrams[0]!);
    }
    expect(evicting.pendingTicks()).toEqual([2n, 3n]);
    expect(evicting.stats().evictedSnapshots).toBe(1);
    assertBounded(evicting);
  });

  test("inconsistent fragment counts are rejected without losing progress", () => {
    const original = frame(5, 30);
    const datagrams = encodeSnapshotFragments(original, 30);
    expect(datagrams.length).toBe(4);
    const reassembler = new SnapshotReassembler();
    reassembler.accept(datagrams[0]!);
    expect(() => reassembler.accept(rawFragment(5n, 1, 3, new TextEncoder().encode("conflict")))).toThrow("inconsistent");
    let delivered = null;
    for (const datagram of datagrams.slice(1)) {
      delivered = reassembler.accept(datagram);
    }
    expect(delivered).toEqual(decodeSnapshotFrame(original));
    expect(reassembler.stats().rejectedDatagrams).toBe(1);
  });

  test("malicious datagrams are rejected without state changes", () => {
    const reassembler = new SnapshotReassembler();
    reassembler.accept(fragments(2, 3_000, 1_000)[0]!);
    const before = [reassembler.pendingSnapshots, reassembler.bufferedBytes];
    const chunk = new TextEncoder().encode("chunk");
    const wrongVersion = rawFragment(3n, 0, 2, chunk);
    wrongVersion[0] = SESSION_PROTOCOL_VERSION + 1;
    const unknownKind = rawFragment(3n, 0, 2, chunk);
    unknownKind[1] = 0xff;
    const declaredTooLong = rawFragment(3n, 0, 2, chunk);
    new DataView(declaredTooLong.buffer).setUint16(12, 0xffff);
    const malicious = [
      new Uint8Array(),
      Uint8Array.of(SESSION_PROTOCOL_VERSION),
      wrongVersion,
      unknownKind,
      declaredTooLong,
      rawFragment(3n, 0, 0, chunk),
      rawFragment(3n, 0, 0xff, chunk),
      rawFragment(3n, 7, 2, chunk),
      rawFragment(3n, 0, 2, new Uint8Array()),
      rawFragment(0xffff_ffff_ffff_ffffn, 0xff, 0xff, chunk),
    ];
    for (const datagram of malicious) {
      expect(() => reassembler.accept(datagram)).toThrow();
    }
    expect([reassembler.pendingSnapshots, reassembler.bufferedBytes]).toEqual(before);
    expect(reassembler.stats().rejectedDatagrams).toBe(malicious.length);
  });

  test("abandoned far-future fragments expire and do not block later snapshots", () => {
    const reassembler = new SnapshotReassembler();
    for (let offset = 0n; offset < BigInt(SNAPSHOT_REASSEMBLY_MAX_PENDING); offset += 1n) {
      expect(reassembler.accept(rawFragment(0xffff_ffff_ffff_ffffn - offset, 0, 2, Uint8Array.of(0xaa)))).toBeNull();
    }
    expect(reassembler.pendingSnapshots).toBe(SNAPSHOT_REASSEMBLY_MAX_PENDING);
    const delivered: bigint[] = [];
    for (let tick = 1; tick <= 100; tick += 1) {
      for (const datagram of fragments(tick, 3_000, 1_100)) {
        const snapshot = reassembler.accept(datagram);
        if (snapshot) delivered.push(snapshot.tick);
        assertBounded(reassembler);
      }
    }
    expect(reassembler.stats().expiredSnapshots).toBe(SNAPSHOT_REASSEMBLY_MAX_PENDING);
    expect(reassembler.newestTick).toBe(100n);
    expect(reassembler.pendingSnapshots).toBe(0);
    expect(delivered.length).toBeGreaterThanOrEqual(100 - (Math.floor(SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS / 3) + 2));
  });

  test("an incomplete snapshot expires after the idle datagram bound", () => {
    const datagrams = fragments(5, 3_000, 1_100);
    expect(datagrams.length).toBe(3);
    const reassembler = new SnapshotReassembler();
    reassembler.accept(datagrams[0]!);
    for (let count = 0; count < SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS - 1; count += 1) {
      expect(() => reassembler.accept(new Uint8Array())).toThrow();
    }
    // A stored fragment resets the idle count.
    expect(reassembler.accept(datagrams[1]!)).toBeNull();
    for (let count = 0; count < SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS; count += 1) {
      expect(() => reassembler.accept(new Uint8Array())).toThrow();
    }
    expect(reassembler.pendingSnapshots).toBe(1);
    expect(reassembler.stats().expiredSnapshots).toBe(0);
    expect(() => reassembler.accept(new Uint8Array())).toThrow();
    expect(reassembler.pendingSnapshots).toBe(0);
    expect(reassembler.bufferedBytes).toBe(0);
    expect(reassembler.stats().expiredSnapshots).toBe(1);
    expect(reassembler.accept(datagrams[2]!)).toBeNull();
    expect(reassembler.pendingSnapshots).toBe(1);
  });

  test("forged chunks fail snapshot verification after reassembly and leave it usable", () => {
    const forged = fragments(6, 2_000, 700);
    forged[1]![forged[1]!.length - 1]! ^= 0xff;
    const reassembler = new SnapshotReassembler();
    const outcomes = forged.map((datagram) => {
      try {
        return reassembler.accept(datagram);
      } catch (error) {
        return error as Error;
      }
    });
    expect((outcomes.at(-1) as Error).message).toContain("snapshot hash mismatch");
    expect(reassembler.pendingSnapshots).toBe(0);
    expect(reassembler.bufferedBytes).toBe(0);
    expect(reassembler.newestTick).toBeNull();
    let delivered = null;
    for (const datagram of fragments(6, 2_000, 700)) {
      delivered = reassembler.accept(datagram) ?? delivered;
    }
    expect(delivered?.tick).toBe(6n);
  });

  test("the fragment tick must match the reassembled snapshot's tick", () => {
    const retagged = fragments(6, 2_000, 700);
    for (const datagram of retagged) {
      new DataView(datagram.buffer, datagram.byteOffset).setBigUint64(2, 7n);
    }
    const reassembler = new SnapshotReassembler();
    for (const datagram of retagged.slice(0, -1)) {
      reassembler.accept(datagram);
    }
    expect(() => reassembler.accept(retagged.at(-1)!)).toThrow("inconsistent");
    expect(reassembler.newestTick).toBeNull();
  });

  test("oversized fragment sets are dropped before reassembly", () => {
    const chunk = new Uint8Array(0xffff);
    const reassembler = new SnapshotReassembler();
    expect(reassembler.accept(rawFragment(1n, 0, 2, chunk))).toBeNull();
    expect(() => reassembler.accept(rawFragment(1n, 1, 2, chunk))).toThrow("exceeds maximum");
    expect(reassembler.pendingSnapshots).toBe(0);
    expect(reassembler.bufferedBytes).toBe(0);
  });

  test("a deterministic lossy, duplicating and reordering stream stays bounded and monotonic", () => {
    const originals = Array.from({ length: 40 }, (_, index) => frame(index + 1, 500 + ((index + 1) * 137) % 4_000));
    let state = 0x2545_f491_4f6c_dd1dn;
    const next = () => {
      state ^= BigInt.asUintN(64, state << 13n);
      state ^= state >> 7n;
      state ^= BigInt.asUintN(64, state << 17n);
      return state;
    };
    const stream: Uint8Array[] = [];
    for (const original of originals) {
      for (const datagram of encodeSnapshotFragments(original, 600)) {
        const roll = next() % 10n;
        if (roll === 0n) continue;
        stream.push(datagram);
        if (roll === 1n) stream.push(datagram);
      }
    }
    for (let index = stream.length - 1; index >= 1; index -= 1) {
      if (next() % 4n === 0n) {
        const other = Math.max(0, index - 1 - Number(next() % 6n));
        [stream[index], stream[other]] = [stream[other]!, stream[index]!];
      }
    }
    const reassembler = new SnapshotReassembler();
    let lastTick = 0n;
    let delivered = 0;
    for (const datagram of stream) {
      const snapshot = reassembler.accept(datagram);
      if (snapshot) {
        expect(snapshot.tick > lastTick).toBe(true);
        expect(snapshot).toEqual(decodeSnapshotFrame(originals[Number(snapshot.tick) - 1]!));
        lastTick = snapshot.tick;
        delivered += 1;
      }
      assertBounded(reassembler);
    }
    expect(delivered).toBeGreaterThan(0);
    expect(reassembler.stats().reassembledSnapshots).toBe(delivered);
  });
});
