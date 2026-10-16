import { encodeCommand, type WorldCommand } from "../command-wire";
import { SnapshotBuffer, TICK_HZ, decodeSnapshot, type EntityState, type ZoneSnapshot } from "../replication";
import { browserClock, type SessionClock, type SessionConnection, type SessionConnector, type SessionEnd } from "../session/connection";
import { decodeWelcome, type SnapshotFrame, type Welcome } from "../session/frames";
import { reconnectUrl, type SessionRoute } from "../session/route";
import { SequencedOutbox, type Delivery } from "../session/sequenced-outbox";
import { SnapshotReassembler } from "../session/snapshot-reassembler";
import { classChoiceCodes, DEFAULT_JOIN_CHARACTER, type JoinCharacter, type LinkState, type WorldSource } from "./world-source";

/** A join (welcome and first projection) must finish within this bound, like the native client's connect. */
export const CONNECT_TIMEOUT_MS = 10_000;
/** A joined source resumes after this long without a newer projection, like the native client. */
export const SNAPSHOT_TIMEOUT_MS = 5_000;
/**
 * A resume first waits this long so the host can observe the old
 * connection's end; the pinned game-server has no disconnect acknowledgement,
 * so this is best effort, as in the native client.
 */
export const RECONNECT_SETTLE_MS = 250;
/** Presentation runs this many ticks behind the newest projection and holds it when projections stall. */
export const INTERPOLATION_DELAY_TICKS = 2;
/**
 * At most this many projections (one second) wait for the next `advance`; a backgrounded page that
 * stops rendering keeps only the newest, like lost datagrams, and the periodic sheets restore state.
 */
export const MAX_RECEIVED_PROJECTIONS = 30;

export type OnlineZoneOptions = {
  route: SessionRoute;
  /** The content revision of the scenery this page loaded; every projection must carry it. */
  contentRevision: bigint;
  connector: SessionConnector;
  clock?: SessionClock;
};

/** One WebTransport session to the zone host: an admission or a resume. */
type Link = {
  readonly connection: SessionConnection;
  readonly reassembler: SnapshotReassembler;
  /** The welcome this session was admitted or resumed with, once accepted. */
  welcome: Welcome | null;
};

type Phase =
  | { kind: "idle" }
  | { kind: "joining"; resolve(player: number): void; reject(error: Error): void; cancelTimeout(): void }
  | { kind: "connected" }
  | {
    kind: "reconnecting";
    /** The welcome of the lost connection: the player and epoch the resume must continue. */
    previous: Welcome;
    /** The sequence of the stopped move the resumed connection starts with, once sent. */
    barrier: number | null;
    cancelTimers(): void;
  }
  /** Failed closed; the next `advance` or `sendCommand` reports it and the source is unjoined. */
  | { kind: "failed"; error: Error };

/**
 * Movement is held state the presentation resends; every discrete intent is
 * resent until acknowledged, so a lost datagram never drops a press.
 */
function deliveryOf(command: WorldCommand): Delivery {
  return command.kind === "move" ? "latest" : "reliable";
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  return left.byteLength === right.byteLength && left.every((byte, index) => byte === right[index]);
}

/**
 * A world hosted by a zone host (`mmorpg-zone-host`) and reached over
 * WebTransport with the game-server session protocol, the browser
 * counterpart of the native client's session (`mmorpg-client` network and
 * session modules):
 *
 * - `join` opens an admission session, reads the welcome (player, tick rate,
 *   connection epoch, reconnect token) and resolves with the first valid
 *   projection addressed to that player;
 * - commands go out as command frames with strictly increasing sequences;
 *   movement is held state the presentation resends, and discrete intents
 *   are resent until a projection acknowledges them (`SequencedOutbox`);
 * - snapshot datagrams are reassembled and verified per connection, decoded
 *   with the same player-projection decoder the local zone uses, and must
 *   carry the page's zone, content revision and player;
 * - a lost connection or five seconds without projections triggers one
 *   bounded resume with the in-memory reconnect token. It keeps the player
 *   and sequencing, starts stopped, drops intents made meanwhile and resets
 *   presentation history for the new connection epoch. It never admits a new
 *   player instead. `leave` cancels it.
 *
 * Tokens stay in memory: they never appear in errors, logs or page URLs.
 */
export class OnlineZoneSource implements WorldSource {
  readonly #route: SessionRoute;
  readonly #contentRevision: bigint;
  readonly #connector: SessionConnector;
  readonly #clock: SessionClock;
  #phase: Phase = { kind: "idle" };
  #link: Link | null = null;
  /** The welcome of the current session: its player, epoch and one-time resume token. */
  #welcome: Welcome | null = null;
  #outbox = new SequencedOutbox();
  readonly #history = new SnapshotBuffer();
  #latest: ZoneSnapshot | null = null;
  #latestAt = 0;
  /** The facing of the last move sent; a resume starts stopped with it. */
  #facing = 0;
  /** One resume per interruption; a completed resume makes the next one available again. */
  #resumeAvailable = true;
  /** The first projection of a resumed connection replaces the presentation history. */
  #resetHistory = false;
  /** The class and sex the joining player chooses with its first command. */
  #character: JoinCharacter = DEFAULT_JOIN_CHARACTER;
  /** Projections accepted since the last `advance`, in tick order. */
  #received: ZoneSnapshot[] = [];
  /** The sequence the class choice went out under, until a projection acknowledges it. */
  #classSequence: number | null = null;
  #classAcknowledged = false;

  constructor(options: OnlineZoneOptions) {
    this.#route = options.route;
    this.#contentRevision = options.contentRevision;
    this.#connector = options.connector;
    this.#clock = options.clock ?? browserClock;
  }

  join(character: JoinCharacter = DEFAULT_JOIN_CHARACTER): Promise<number> {
    if (this.#phase.kind !== "idle" && this.#phase.kind !== "failed") {
      return Promise.reject(new Error("Already in the world; leave before joining again."));
    }
    this.#teardown();
    this.#character = character;
    return new Promise<number>((resolve, reject) => {
      const cancelTimeout = this.#clock.after(CONNECT_TIMEOUT_MS, () =>
        this.#abortJoin(new Error("The zone host did not admit this player within ten seconds.")));
      this.#phase = { kind: "joining", resolve, reject, cancelTimeout };
      try {
        this.#open(this.#route.url);
      } catch (error) {
        this.#abortJoin(new Error(`Could not connect to the zone host: ${describe(error)}`));
      }
    });
  }

  leave(): void {
    const phase = this.#phase;
    switch (phase.kind) {
      case "idle":
        return;
      case "joining":
        this.#abortJoin(new Error("Entry was cancelled."));
        return;
      case "reconnecting":
        phase.cancelTimers();
        break;
      case "connected":
      case "failed":
        break;
    }
    this.#teardown();
    this.#phase = { kind: "idle" };
  }

  sendCommand(command: WorldCommand): number | null {
    this.#reportFailure();
    const phase = this.#phase;
    if (phase.kind === "reconnecting") {
      // Intent made during an interruption is dropped, never replayed after the resume.
      return null;
    }
    if (phase.kind !== "connected") {
      throw new Error("Join the world before sending commands.");
    }
    const payload = encodeCommand(command);
    if (command.kind === "move") {
      this.#facing = command.facing;
    }
    const sequence = this.#outbox.submit(payload, deliveryOf(command));
    this.#flush();
    return sequence;
  }

  advance(): readonly ZoneSnapshot[] {
    if (this.#phase.kind === "connected") {
      if (this.#clock.now() - this.#latestAt > SNAPSHOT_TIMEOUT_MS) {
        this.#resume("No projection arrived from the zone host for five seconds.");
      } else {
        this.#flush();
      }
    }
    this.#reportFailure();
    return this.#received.splice(0);
  }

  latestProjection(): ZoneSnapshot | null {
    return this.#latest;
  }

  sample(): readonly EntityState[] {
    const latest = this.#latest;
    if (!latest) {
      return [];
    }
    // Like the native client: two ticks behind the newest projection, closing the gap while it stalls.
    const elapsedTicks = ((this.#clock.now() - this.#latestAt) * TICK_HZ) / 1_000;
    const delay = Math.max(0, INTERPOLATION_DELAY_TICKS - elapsedTicks);
    const whole = Math.ceil(delay);
    const tick = latest.tick - BigInt(whole);
    return tick < 0n ? this.#history.sample(0n) : this.#history.sample(tick, whole - delay);
  }

  linkState(): LinkState {
    return this.#phase.kind === "reconnecting" ? "reconnecting" : "connected";
  }

  /** Opens a session to `url` and makes it the current link; callbacks of replaced links are ignored. */
  #open(url: string): void {
    let link: Link | null = null;
    const connection = this.#connector(url, (datagram) => {
      if (link) {
        this.#receive(link, datagram);
      }
    });
    const opened: Link = { connection, reassembler: new SnapshotReassembler(), welcome: null };
    link = opened;
    this.#link = opened;
    void connection.welcome.then(
      (bytes) => this.#welcomed(opened, bytes),
      (error: unknown) => this.#linkFailed(opened, new Error(describe(error))),
    );
    void connection.closed.then((end) => this.#ended(opened, end));
  }

  #closeLink(): void {
    const link = this.#link;
    this.#link = null;
    link?.connection.close();
  }

  #welcomed(link: Link, bytes: Uint8Array): void {
    if (link !== this.#link) {
      return;
    }
    let welcome: Welcome;
    try {
      welcome = decodeWelcome(bytes);
    } catch (error) {
      this.#linkFailed(link, new Error(`The zone host sent an invalid welcome: ${describe(error)}`));
      return;
    }
    if (welcome.tickHz !== TICK_HZ || welcome.playerId === 0 || welcome.connectionEpoch === 0) {
      this.#linkFailed(link, new Error(`The zone host's welcome is incompatible with this client (${welcome.tickHz} Hz ticks).`));
      return;
    }
    const phase = this.#phase;
    if (phase.kind === "joining") {
      link.welcome = welcome;
      this.#welcome = welcome;
      // Like the native client, the admitted player chooses its class with its first command.
      this.#outbox.submit(this.#classChoice(), "reliable");
      try {
        this.#flush();
      } catch (error) {
        this.#linkFailed(link, new Error(describe(error)));
        return;
      }
      this.#classSequence = this.#outbox.awaiting;
      return;
    }
    if (phase.kind !== "reconnecting") {
      return;
    }
    const { previous } = phase;
    if (welcome.playerId !== previous.playerId || welcome.connectionEpoch <= previous.connectionEpoch ||
        sameBytes(welcome.reconnectToken, previous.reconnectToken)) {
      this.#fail(new Error("The zone host did not resume this player; a new player is never substituted."));
      return;
    }
    link.welcome = welcome;
    this.#welcome = welcome;
    this.#resetHistory = true;
    // Start the resumed connection stopped, keeping the last facing; nothing queued before is replayed,
    // except a class choice no projection acknowledged yet, which goes out first.
    const stop = encodeCommand({ kind: "move", forward: 0, strafe: 0, facing: this.#facing });
    const classPending = !this.#classAcknowledged;
    this.#outbox.restart(classPending ? this.#classChoice() : stop);
    if (classPending) {
      this.#outbox.submit(stop, "latest");
    }
    try {
      this.#flush();
    } catch (error) {
      this.#fail(new Error(describe(error)));
      return;
    }
    phase.barrier = this.#outbox.awaiting;
    if (classPending) {
      this.#classSequence = phase.barrier;
    }
  }

  #receive(link: Link, datagram: Uint8Array): void {
    const welcome = link.welcome;
    if (link !== this.#link || !welcome) {
      // A datagram that overtook its session's welcome carries a tick the next one supersedes.
      return;
    }
    let frame: SnapshotFrame | null;
    try {
      frame = link.reassembler.accept(datagram);
    } catch (error) {
      this.#linkFailed(link, new Error(`The zone host sent a malformed snapshot: ${describe(error)}`));
      return;
    }
    if (!frame) {
      return;
    }
    try {
      this.#project(welcome, frame);
    } catch (error) {
      this.#linkFailed(link, error instanceof Error ? error : new Error(describe(error)));
    }
  }

  /** Validates and presents one verified snapshot frame of the current session. */
  #project(welcome: Welcome, frame: SnapshotFrame): void {
    const snapshot = decodeSnapshot(frame.payload);
    if (snapshot.contentRevision !== this.#contentRevision) {
      throw new Error(
        `The zone host runs content revision ${snapshot.contentRevision}, but this page loaded revision ${this.#contentRevision}. ` +
        "Both must come from the same build.",
      );
    }
    if (snapshot.zoneId !== this.#route.zoneId) {
      throw new Error(`The zone host sent zone ${snapshot.zoneId} on the route for zone ${this.#route.zoneId}.`);
    }
    if (snapshot.tick !== frame.tick) {
      throw new Error("The zone host's session and projection ticks disagree.");
    }
    if (snapshot.viewerId !== welcome.playerId) {
      throw new Error("The zone host addressed a projection to another player.");
    }
    if (this.#resetHistory) {
      // A new connection epoch: never blend with the lost connection's samples.
      this.#resetHistory = false;
      this.#history.reset();
    }
    if (!this.#history.push(snapshot)) {
      return;
    }
    this.#latest = snapshot;
    this.#latestAt = this.#clock.now();
    this.#received.push(snapshot);
    if (this.#received.length > MAX_RECEIVED_PROJECTIONS) {
      this.#received.shift();
    }
    this.#outbox.acknowledge(snapshot.acknowledgedSequence);
    if (this.#classSequence !== null && snapshot.acknowledgedSequence >= this.#classSequence) {
      this.#classAcknowledged = true;
      this.#classSequence = null;
    }
    this.#flush();
    const phase = this.#phase;
    if (phase.kind === "joining") {
      phase.cancelTimeout();
      this.#phase = { kind: "connected" };
      phase.resolve(welcome.playerId);
    } else if (phase.kind === "reconnecting" && phase.barrier !== null && snapshot.acknowledgedSequence >= phase.barrier) {
      phase.cancelTimers();
      this.#phase = { kind: "connected" };
      this.#resumeAvailable = true;
    }
  }

  #ended(link: Link, end: SessionEnd): void {
    if (link !== this.#link) {
      return;
    }
    const reason = end.kind === "closed"
      ? `The zone host ended the session (code ${end.code}${end.reason ? `: ${end.reason}` : ""}).`
      : `The connection to the zone host was lost: ${end.message}`;
    // A page cannot tell a host's clean close from a lost path, so a connected session treats both as
    // an interruption and lets the host refuse a resume it will not grant (docs/NATIVE_CLIENT.md).
    if (this.#phase.kind === "connected") {
      this.#resume(reason);
      return;
    }
    this.#linkFailed(link, new Error(reason));
  }

  /** A session failed: a join is refused, and anything later fails the source closed. */
  #linkFailed(link: Link, error: Error): void {
    if (link !== this.#link) {
      return;
    }
    const phase = this.#phase;
    if (phase.kind === "joining") {
      this.#abortJoin(new Error(`Could not join the zone host: ${error.message}`));
    } else if (phase.kind === "reconnecting") {
      // The resume route carries the token, so its transport errors are not repeated.
      this.#fail(new Error(link.welcome ? error.message : "The zone host did not resume this session."));
    } else {
      this.#fail(error);
    }
  }

  /** One bounded resume attempt, never a fallback to a new admission. */
  #resume(reason: string): void {
    const previous = this.#welcome;
    if (!this.#resumeAvailable || !previous) {
      this.#fail(new Error(reason));
      return;
    }
    this.#resumeAvailable = false;
    // Close first so the host observes the end and moves the player into its reconnect grace.
    this.#closeLink();
    const graceMs = (Number(previous.reconnectGraceTicks) * 1_000) / previous.tickHz;
    const cancelDeadline = this.#clock.after(Math.min(CONNECT_TIMEOUT_MS, graceMs), () =>
      this.#fail(new Error(`${reason} Resuming the session timed out.`)));
    const cancelSettle = this.#clock.after(RECONNECT_SETTLE_MS, () => {
      try {
        this.#open(reconnectUrl(this.#route, previous.reconnectToken));
      } catch {
        this.#fail(new Error(`${reason} The session could not be resumed.`));
      }
    });
    this.#phase = {
      kind: "reconnecting",
      previous,
      barrier: null,
      cancelTimers: () => {
        cancelDeadline();
        cancelSettle();
      },
    };
  }

  #fail(error: Error): void {
    const phase = this.#phase;
    if (phase.kind === "joining") {
      this.#abortJoin(error);
      return;
    }
    if (phase.kind === "idle" || phase.kind === "failed") {
      return;
    }
    if (phase.kind === "reconnecting") {
      phase.cancelTimers();
    }
    this.#teardown();
    this.#phase = { kind: "failed", error };
  }

  #abortJoin(error: Error): void {
    const phase = this.#phase;
    if (phase.kind !== "joining") {
      return;
    }
    phase.cancelTimeout();
    this.#teardown();
    this.#phase = { kind: "idle" };
    phase.reject(error);
  }

  /** Throws a pending failure once, leaving the source unjoined. */
  #reportFailure(): void {
    const phase = this.#phase;
    if (phase.kind === "failed") {
      this.#phase = { kind: "idle" };
      throw phase.error;
    }
  }

  /** Sends whatever the outbox has due now on the current link. */
  #flush(): void {
    const link = this.#link;
    if (!link) {
      return;
    }
    for (const datagram of this.#outbox.poll(this.#clock.now())) {
      link.connection.send(datagram);
    }
  }

  /** Forgets the session: closes the link and clears presentation and sequencing. */
  #teardown(): void {
    this.#closeLink();
    this.#welcome = null;
    this.#outbox = new SequencedOutbox();
    this.#history.reset();
    this.#latest = null;
    this.#latestAt = 0;
    this.#facing = 0;
    this.#resumeAvailable = true;
    this.#resetHistory = false;
    this.#received = [];
    this.#classSequence = null;
    this.#classAcknowledged = false;
  }

  #classChoice(): Uint8Array {
    const { classId, sex } = classChoiceCodes(this.#character);
    return encodeCommand({ kind: "choose-class", classId, sex });
  }
}
