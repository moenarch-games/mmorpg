import { describe, expect, test } from "bun:test";
import { MAX_QUEUED_RELIABLE, RELIABLE_RESEND_MS, SequencedOutbox } from "../src/session/sequenced-outbox";
import { decodeCommandFrame } from "./support/session-host-frames";

const move = (facing: number) => Uint8Array.of(2, 1, 1, 0, facing >> 8, facing & 0xff);
const JUMP = Uint8Array.of(2, 2);
const hex = (bytes: Uint8Array) => Buffer.from(bytes).toString("hex");
const sent = (datagrams: Uint8Array[]) => datagrams.map((datagram) => {
  const frame = decodeCommandFrame(datagram);
  return [frame.sequence, hex(frame.payload)] as const;
});

/**
 * The host's rule, as `game_server::MatchRuntime::submit_command` applies it:
 * a sequence at or below the newest applied one is ignored; the projection
 * acknowledges the newest applied sequence.
 */
class Host {
  acknowledged = 0;
  readonly applied: string[] = [];

  receive(datagrams: Uint8Array[]): void {
    for (const datagram of datagrams) {
      const frame = decodeCommandFrame(datagram);
      if (frame.sequence > this.acknowledged) {
        this.acknowledged = frame.sequence;
        this.applied.push(hex(frame.payload));
      }
    }
  }
}

describe("sequenced command outbox", () => {
  test("held state goes out at once under strictly increasing sequences", () => {
    const outbox = new SequencedOutbox();
    outbox.submit(move(1), "latest");
    expect(sent(outbox.poll(0))).toEqual([[1, hex(move(1))]]);
    outbox.submit(move(2), "latest");
    outbox.submit(move(3), "latest");
    // Nothing waits for an acknowledgement, so the newest held state replaces the unsent one.
    expect(sent(outbox.poll(1))).toEqual([[2, hex(move(3))]]);
    expect(outbox.poll(2)).toEqual([]);
    expect(outbox.sequence).toBe(2);
    expect(outbox.stats()).toEqual({ sent: 2, resent: 0, coalesced: 1, dropped: 0 });
  });

  test("a reliable intent is resent byte for byte until acknowledged, and later commands wait", () => {
    const outbox = new SequencedOutbox();
    outbox.submit(move(1), "latest");
    outbox.submit(JUMP, "reliable");
    outbox.submit(move(2), "latest");
    const first = outbox.poll(0);
    expect(sent(first)).toEqual([[1, hex(move(1))], [2, "0202"]]);
    expect(outbox.awaiting).toBe(2);
    expect(outbox.poll(RELIABLE_RESEND_MS - 1)).toEqual([]);
    const resend = outbox.poll(RELIABLE_RESEND_MS);
    expect(resend).toEqual([first[1]!]);
    outbox.submit(move(3), "latest");
    expect(outbox.queued).toBe(1);
    outbox.acknowledge(1);
    expect(outbox.poll(RELIABLE_RESEND_MS * 2)).toEqual([first[1]!]);
    outbox.acknowledge(2);
    expect(outbox.awaiting).toBeNull();
    expect(sent(outbox.poll(RELIABLE_RESEND_MS * 2 + 1))).toEqual([[3, hex(move(3))]]);
    expect(outbox.stats()).toEqual({ sent: 3, resent: 2, coalesced: 1, dropped: 0 });
  });

  test("every reliable intent is applied exactly once despite loss and duplication", () => {
    const outbox = new SequencedOutbox();
    const host = new Host();
    let now = 0;
    let state = 0x2545_f491;
    const lossy = (datagrams: Uint8Array[]) => datagrams.flatMap((datagram) => {
      state ^= state << 13;
      state ^= state >>> 17;
      state ^= state << 5;
      const roll = (state >>> 0) % 4;
      return roll === 0 ? [] : roll === 1 ? [datagram, datagram] : [datagram];
    });
    for (let step = 0; step < 400; step += 1) {
      now += 17;
      if (step % 7 === 0) outbox.submit(JUMP, "reliable");
      outbox.submit(move(step % 3), "latest");
      host.receive(lossy(outbox.poll(now)));
      // Projections carrying the acknowledgement are lossy too.
      if (step % 3 !== 0) outbox.acknowledge(host.acknowledged);
    }
    for (let drain = 0; drain < 100 && (outbox.awaiting !== null || outbox.queued > 0); drain += 1) {
      now += RELIABLE_RESEND_MS;
      host.receive(outbox.poll(now));
      outbox.acknowledge(host.acknowledged);
    }
    expect(host.applied.filter((payload) => payload === "0202").length).toBe(Math.ceil(400 / 7));
    expect(outbox.stats().dropped).toBe(0);
  });

  test("the reliable queue is bounded; overflow drops the newest intent", () => {
    const outbox = new SequencedOutbox();
    for (let index = 0; index < MAX_QUEUED_RELIABLE; index += 1) {
      // Each queued command learns the sequence it will be sent under.
      expect(outbox.submit(JUMP, "reliable")).toBe(index + 1);
    }
    expect(outbox.submit(JUMP, "reliable")).toBeNull();
    expect(outbox.submit(move(1), "latest")).toBe(MAX_QUEUED_RELIABLE + 1);
    expect(outbox.stats().dropped).toBe(1);
    // The first intent goes out as sequence 1 and waits for its acknowledgement.
    outbox.poll(0);
    expect(outbox.awaiting).toBe(1);
    // It left the queue, which makes room for one more, sent after the queued move.
    expect(outbox.submit(JUMP, "reliable")).toBe(MAX_QUEUED_RELIABLE + 2);
  });

  test("a restart forgets queued and unacknowledged commands and sends its barrier first", () => {
    const outbox = new SequencedOutbox();
    outbox.submit(JUMP, "reliable");
    outbox.submit(move(5), "latest");
    outbox.poll(0);
    expect(outbox.awaiting).toBe(1);
    outbox.restart(move(0));
    expect(outbox.awaiting).toBeNull();
    // The unacknowledged sequence 1 counts as used: sequences continue above it.
    expect(sent(outbox.poll(1))).toEqual([[2, hex(move(0))]]);
    expect(outbox.awaiting).toBe(2);
    outbox.acknowledge(2);
    expect(outbox.poll(2)).toEqual([]);
  });

  test("sequence exhaustion fails closed instead of wrapping", () => {
    const outbox = new SequencedOutbox(0xffff_fffe);
    outbox.submit(move(1), "latest");
    expect(sent(outbox.poll(0))).toEqual([[0xffff_ffff, hex(move(1))]]);
    outbox.submit(move(2), "latest");
    expect(() => outbox.poll(1)).toThrow("exhausted");
    expect(() => new SequencedOutbox(-1)).toThrow();
    expect(() => new SequencedOutbox(0x1_0000_0000)).toThrow();
  });
});
