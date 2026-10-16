import type { SessionClock, SessionConnection, SessionConnector, SessionEnd } from "../../src/session/connection";
import type { EntityState } from "../../src/replication";
import { decodeCommandFrame, encodeSnapshotFragments, encodeSnapshotFrame, encodeWelcomeFrame } from "./session-host-frames";
import { encodeTestSnapshot } from "./snapshot-encoder";
import { playerEntity, testSnapshot } from "./snapshots";

/** Lets every pending promise callback run, like returning to the event loop. */
export const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

type Timer = { at: number; order: number; callback: () => void };

/** A manual `SessionClock`: time moves only through `advance`, running due timers in order. */
export class ManualClock implements SessionClock {
  #now = 0;
  #order = 0;
  #timers: Timer[] = [];

  now(): number {
    return this.#now;
  }

  after(milliseconds: number, callback: () => void): () => void {
    const timer = { at: this.#now + milliseconds, order: this.#order++, callback };
    this.#timers.push(timer);
    return () => {
      this.#timers = this.#timers.filter((candidate) => candidate !== timer);
    };
  }

  /** Moves time forward by `milliseconds`, settling promise callbacks after every timer. */
  async advance(milliseconds: number): Promise<void> {
    const target = this.#now + milliseconds;
    for (;;) {
      await settle();
      const due = this.#timers
        .filter((timer) => timer.at <= target)
        .sort((a, b) => a.at - b.at || a.order - b.order)[0];
      if (!due) {
        break;
      }
      this.#timers = this.#timers.filter((timer) => timer !== due);
      this.#now = Math.max(this.#now, due.at);
      due.callback();
    }
    this.#now = target;
    await settle();
  }

  /** Runs time a tick at a time until `promise` settles (at most `limitMs`), then returns it. */
  async until<T>(promise: Promise<T>, limitMs = 20_000): Promise<T> {
    let settled = false;
    promise.then(() => { settled = true; }, () => { settled = true; });
    for (let elapsed = 0; !settled && elapsed < limitMs; elapsed += TICK_MS) {
      await this.advance(TICK_MS);
    }
    return promise;
  }
}

const TICK_HZ = 30;
export const TICK_MS = 1_000 / TICK_HZ;
/**
 * Command wire v9 payload lengths by tag (chat, tag 16, carries its own text length): move, jump, select target, start and stop attack, release
 * spirit, move item, loot, use ability, cancel cast, choose class, equip and unequip item, buy and sell item, emote, and accept,
 * complete and abandon quest.
 */
const COMMAND_LENGTHS = new Map([
  [1, 6], [2, 2], [3, 7], [4, 2], [5, 2], [6, 2], [7, 6], [8, 14], [9, 8], [10, 2], [11, 4], [12, 3], [13, 3],
  [14, 9], [15, 9], [17, 3], [18, 7], [19, 8], [20, 3],
]);
const RUN_UNITS_PER_TICK = 21;

type Session = {
  playerId: number;
  epoch: number;
  token: Uint8Array;
  connected: boolean;
  expiresAt: bigint;
  lastSequence: number;
  unit: { x: number; z: number; facing: number; forward: number; strafe: number };
  link: HostLink | null;
};

type HostLink = {
  session: Session;
  receive(datagram: Uint8Array): void;
  end(end: SessionEnd): void;
  open: boolean;
};

export type FakeZoneHostOptions = {
  zoneId?: number;
  contentRevision?: bigint;
  /** Ticks a disconnected player keeps its unit and resume token (game-server's reconnect grace). */
  graceTicks?: bigint;
  /** The connection's datagram budget; larger snapshot frames are fragmented. */
  datagramBytes?: number;
  prefix?: string;
  origin?: string;
};

/**
 * An in-memory zone host that keeps the game-server session semantics the
 * online source depends on (TEST-021): admission and never-reused player
 * IDs, the welcome stream, rotating one-time resume tokens and connection
 * epochs, `AlreadyConnected` and expired resumes refused at the handshake,
 * reconnect grace before a disconnected unit is removed (in the tick after
 * `tick > expiresAt`), stale sequences ignored, the newest applied sequence
 * acknowledged in each player's projection, per-connection snapshot frames
 * with their state hash, and fragmentation above the datagram budget. The
 * zone itself is trivial: units run along their held intent and nothing
 * collides.
 */
export class FakeZoneHost {
  readonly clock: ManualClock;
  readonly zoneId: number;
  /** The revision the host's projections carry; tests change it to simulate a different build. */
  contentRevision: bigint;
  readonly graceTicks: bigint;
  datagramBytes: number;
  readonly #prefix: string;
  readonly #origin: string;
  tick = 0n;
  /** While set, admissions and resumes are refused at the handshake. */
  refuseSessions = false;
  /** The path to a client: what arrives for each host datagram (none when lost, two when duplicated). */
  toClient: (datagram: Uint8Array, playerId: number) => readonly Uint8Array[] = (datagram) => [datagram];
  /** The path to the host, like `toClient`. */
  toHost: (datagram: Uint8Array, playerId: number) => readonly Uint8Array[] = (datagram) => [datagram];
  /** Every applied command, in order. */
  readonly applied: { playerId: number; sequence: number; payload: string }[] = [];
  /** URLs of every session the client opened, admissions and resumes. */
  readonly opened: string[] = [];
  #nextPlayer = 1;
  #nextToken = 1;
  readonly #sessions = new Map<number, Session>();
  #stopTicking: (() => void) | null = null;

  constructor(clock: ManualClock, options: FakeZoneHostOptions = {}) {
    this.clock = clock;
    this.zoneId = options.zoneId ?? 1;
    this.contentRevision = options.contentRevision ?? 2n;
    this.graceTicks = options.graceTicks ?? 0n;
    this.datagramBytes = options.datagramBytes ?? 1_200;
    this.#prefix = options.prefix ?? "/game";
    this.#origin = options.origin ?? "https://127.0.0.1:4433";
  }

  get url(): string {
    return `${this.#origin}${this.#prefix}/matches/zone-${this.zoneId}`;
  }

  /** Ticks at 30 Hz on the clock until `stop`. */
  start(): this {
    const schedule = () => {
      this.#stopTicking = this.clock.after(TICK_MS, () => {
        this.step();
        schedule();
      });
    };
    schedule();
    return this;
  }

  stop(): void {
    this.#stopTicking?.();
    this.#stopTicking = null;
  }

  /** Player IDs with a unit in the zone. */
  players(): number[] {
    return [...this.#sessions.keys()].sort((a, b) => a - b);
  }

  unit(playerId: number): Session["unit"] | undefined {
    return this.#sessions.get(playerId)?.unit;
  }

  connectionEpoch(playerId: number): number | undefined {
    return this.#sessions.get(playerId)?.epoch;
  }

  /** The path to `playerId` fails: the client sees a lost session; the host disconnects the player. */
  lose(playerId: number): void {
    const link = this.#sessions.get(playerId)?.link;
    if (link) {
      this.#disconnect(link);
      link.end({ kind: "lost", message: "Connection lost." });
    }
  }

  /** The host closes `playerId`'s session cleanly: the client sees a closed session; the player enters its grace. */
  closeSession(playerId: number): void {
    const link = this.#sessions.get(playerId)?.link;
    if (link) {
      this.#disconnect(link);
      link.end({ kind: "closed", code: 0, reason: "" });
    }
  }

  /** One authoritative tick: expire, move, then publish each connected player's projection. */
  step(): void {
    const current = this.tick;
    for (const [playerId, session] of this.#sessions) {
      if (!session.connected && current > session.expiresAt) {
        this.#sessions.delete(playerId);
      }
    }
    for (const { unit } of this.#sessions.values()) {
      const radians = (unit.facing / 65_536) * 2 * Math.PI;
      unit.x += Math.round(RUN_UNITS_PER_TICK * (unit.forward * Math.sin(radians) - unit.strafe * Math.cos(radians)));
      unit.z += Math.round(RUN_UNITS_PER_TICK * (unit.forward * Math.cos(radians) + unit.strafe * Math.sin(radians)));
    }
    this.tick += 1n;
    for (const session of this.#sessions.values()) {
      if (session.connected && session.link) {
        this.#publish(session, session.link);
      }
    }
  }

  readonly connector: SessionConnector = (url, receive) => {
    this.opened.push(url);
    let resolveClosed: (end: SessionEnd) => void = () => undefined;
    const closed = new Promise<SessionEnd>((resolve) => {
      resolveClosed = resolve;
    });
    const refuse = (reason: string): SessionConnection => {
      resolveClosed({ kind: "lost", message: "Opening handshake failed." });
      const welcome = Promise.reject(new Error(`Opening handshake failed. (${reason})`));
      welcome.catch(() => undefined);
      return { welcome, closed, send: () => undefined, close: () => undefined };
    };
    const admission = this.#route(url);
    if (this.refuseSessions || !admission) {
      return refuse("refused");
    }
    let session: Session;
    if (admission === "new") {
      session = {
        playerId: this.#nextPlayer++,
        epoch: 1,
        token: this.#token(),
        connected: true,
        expiresAt: 0n,
        lastSequence: 0,
        unit: { x: 0, z: 0, facing: 0, forward: 0, strafe: 0 },
        link: null,
      };
      this.#sessions.set(session.playerId, session);
    } else {
      const found = [...this.#sessions.values()].find((candidate) => candidate.token.every((byte, index) => byte === admission[index]));
      if (!found) {
        return refuse("unknown reconnect token");
      }
      if (found.connected) {
        return refuse("already connected");
      }
      if (this.tick > found.expiresAt) {
        return refuse("reconnect grace period has expired");
      }
      found.epoch += 1;
      found.token = this.#token();
      found.connected = true;
      session = found;
    }
    const link: HostLink = {
      session,
      receive,
      open: true,
      end: (end) => {
        link.open = false;
        resolveClosed(end);
      },
    };
    session.link = link;
    const welcome = Promise.resolve(encodeWelcomeFrame({
      playerId: session.playerId,
      tickHz: TICK_HZ,
      maxPlayers: 64,
      currentTick: this.tick,
      connectionEpoch: session.epoch,
      reconnectToken: session.token.slice(),
      reconnectGraceTicks: this.graceTicks,
    }));
    return {
      welcome,
      closed,
      send: (datagram) => {
        for (const delivered of this.toHost(datagram, session.playerId)) {
          if (link.open) {
            this.#command(link, delivered);
          }
        }
      },
      close: () => {
        if (link.open) {
          this.#disconnect(link);
          link.end({ kind: "closed", code: 0, reason: "client left" });
        }
      },
    };
  };

  /** `"new"`, a resume token, or null for a route this host does not serve. */
  #route(url: string): "new" | Uint8Array | null {
    if (url === this.url) {
      return "new";
    }
    const resume = new RegExp(`^${this.url}/reconnect/([0-9a-f]{32})$`).exec(url);
    return resume?.[1] ? Uint8Array.from(Buffer.from(resume[1], "hex")) : null;
  }

  #token(): Uint8Array {
    const token = new Uint8Array(16);
    new DataView(token.buffer).setUint32(12, this.#nextToken++);
    token[0] = 0xa5;
    return token;
  }

  #disconnect(link: HostLink): void {
    const { session } = link;
    if (session.link === link && session.connected) {
      session.connected = false;
      session.link = null;
      session.expiresAt = this.tick + this.graceTicks;
    }
  }

/** `MatchRuntime::submit_command` plus the MMO's command decoding; a malformed frame closes the session. */
  #command(link: HostLink, datagram: Uint8Array): void {
    const { session } = link;
    try {
      const frame = decodeCommandFrame(datagram);
      if (frame.sequence <= session.lastSequence) {
        return;
      }
      const [version, tag] = frame.payload;
      if (version !== 9 || tag === undefined || (tag === 16 ? frame.payload.length !== 4 + (frame.payload[3] ?? 0) : COMMAND_LENGTHS.get(tag) !== frame.payload.length)) {
        throw new Error("malformed command payload");
      }
      if (tag === 1) {
        const fields = new DataView(frame.payload.buffer, frame.payload.byteOffset);
        session.unit.forward = fields.getInt8(2);
        session.unit.strafe = fields.getInt8(3);
        session.unit.facing = fields.getUint16(4);
      }
      session.lastSequence = frame.sequence;
      this.applied.push({ playerId: session.playerId, sequence: frame.sequence, payload: Buffer.from(frame.payload).toString("hex") });
    } catch {
      this.#disconnect(link);
      link.end({ kind: "lost", message: "Connection lost." });
    }
  }

  #publish(viewer: Session, link: HostLink): void {
    const entity = (session: Session): EntityState =>
      playerEntity(session.playerId, [session.unit.x, 90, session.unit.z], [0, 0, 0], session.unit.facing);
    const others = [...this.#sessions.values()].filter((session) => session !== viewer);
    const payload = encodeTestSnapshot(testSnapshot({
      zoneId: this.zoneId,
      tick: this.tick,
      contentRevision: this.contentRevision,
      acknowledgedSequence: viewer.lastSequence,
      viewerId: viewer.playerId,
      entities: [entity(viewer), ...others.map(entity)],
    }));
    const frame = encodeSnapshotFrame(this.tick, payload);
    const datagrams = frame.byteLength <= this.datagramBytes ? [frame] : encodeSnapshotFragments(frame, this.datagramBytes);
    for (const datagram of datagrams) {
      for (const delivered of this.toClient(datagram, viewer.playerId)) {
        if (link.open) {
          link.receive(delivered);
        }
      }
    }
  }
}
