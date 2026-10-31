import { encodeCommand, encodeObservedCommand, isTimingSensitive, type WorldCommand } from "../command-wire";
import { ProjectionDecoder, SnapshotBuffer, TICK_HZ, decodeRedirectNotice, isRedirectNotice, ticksFromMs, type EntityState, type ZoneSnapshot } from "../replication";
import { browserClock, type SessionClock, type SessionConnection, type SessionConnector, type SessionEnd } from "../session/connection";
import { decodeWelcome, type SnapshotFrame, type Welcome } from "../session/frames";
import { parseSessionRoute, reconnectUrl, type SessionRoute } from "../session/route";
import { SequencedOutbox, type Delivery } from "../session/sequenced-outbox";
import { SnapshotReassembler } from "../session/snapshot-reassembler";
import { loadWasm } from "../wasm";
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
/** Following a redirect notice (#145) must reach a destination that takes the character within this bound, like the native client. */
export const FOLLOW_TIMEOUT_MS = 20_000;
/** A destination refuses the claim until the source retired the character; the claim is retried this often. */
export const CLAIM_RETRY_MS = 100;
/** Presentation runs this many ticks (4 at 60 Hz) behind the newest projection and holds it when projections stall. */
export const INTERPOLATION_DELAY_TICKS = ticksFromMs(67);
/**
 * At most this many projections (one second) wait for the next `advance`; a backgrounded page that
 * stops rendering keeps only the newest, like lost datagrams; every projection is complete, its sheets held (#544).
 */
export const MAX_RECEIVED_PROJECTIONS = TICK_HZ;

/** An ephemeral activity reuses session/reconnect delivery while supplying its game envelope. */
export type ActivityProtocol = {
  encodeCommand(command: WorldCommand): Uint8Array;
  initialCommand(character: JoinCharacter): Uint8Array;
  decodeProjection(payload: Uint8Array): { snapshot: ZoneSnapshot; finished: boolean };
};

export type OnlineZoneOptions = {
  route: SessionRoute;
  /** The content revision of the scenery this page loaded; every projection must carry it. */
  contentRevision: bigint;
  connector: SessionConnector;
  resolveCharacterId?: (localId: string) => Promise<string>;
  activityProtocol?: ActivityProtocol;
  clock?: SessionClock;
  /**
   * The page presents zone `zoneId` and returns that zone's content revision (#145): on join, the
   * route's zone, and after a redirect notice, the destination's. Without it, only the page's own
   * content is played and a redirect fails the session closed.
   */
  enterZone?: (zoneId: number) => bigint;
};

/** One WebTransport session to the zone host: an admission or a resume. */
type Link = {
  readonly connection: SessionConnection;
  readonly reassembler: SnapshotReassembler;
  /** The welcome this session was admitted or resumed with, once accepted. */
  welcome: Welcome | null;
  admitting: boolean;
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
  /**
   * Following the character to another zone's host after a redirect notice (#145): the source's
   * route and resume token are spent; the destination's first projection connects the source.
   */
  | { kind: "travelling"; cancelTimeout(): void }
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
  /** The joined route; a followed redirect notice replaces it with the destination's (#145). */
  #route: SessionRoute;
  readonly #joinRoute: SessionRoute;
  #contentRevision: bigint;
  readonly #enterZone: ((zoneId: number) => bigint) | null;
  /** The transfer last followed: its notice is never followed twice. */
  #followed: bigint | null = null;
  /** The claimed character's global ID, claimed again at a redirect's destination. */
  #characterId: string | null = null;
  readonly #connector: SessionConnector;
  readonly #clock: SessionClock;
  readonly #activityProtocol: ActivityProtocol | null;
  #phase: Phase = { kind: "idle" };
  #link: Link | null = null;
  /** The welcome of the current session: its player, epoch and one-time resume token. */
  #welcome: Welcome | null = null;
  #durable = false;
  #finished = false;
  #resolveCharacterId: (localId: string) => Promise<string>;
  #outbox = new SequencedOutbox();
  /** The zone's sheets as last received (#544); every command echoes their revisions. */
  #sheets = new ProjectionDecoder();
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
    this.#joinRoute = options.route;
    this.#contentRevision = options.contentRevision;
    this.#enterZone = options.enterZone ?? null;
    this.#connector = options.connector;
    this.#resolveCharacterId = options.resolveCharacterId ?? ((id) => Promise.resolve(id));
    this.#clock = options.clock ?? browserClock;
    this.#activityProtocol = options.activityProtocol ?? null;
  }

  join(character: JoinCharacter = DEFAULT_JOIN_CHARACTER): Promise<number> {
    if (this.#phase.kind !== "idle" && this.#phase.kind !== "failed") {
      return Promise.reject(new Error("Already in the world; leave before joining again."));
    }
    this.#teardown();
    this.#character = character;
    // Every entry starts at the configured route, in the page's presentation of its zone.
    this.#route = this.#joinRoute;
    this.#followed = null;
    if (this.#enterZone) {
      this.#contentRevision = this.#enterZone(this.#route.zoneId);
    }
    return new Promise<number>((resolve, reject) => {
      const cancelTimeout = this.#clock.after(CONNECT_TIMEOUT_MS, () =>
        this.#abortJoin(new Error("The zone host did not admit this player within ten seconds.")));
      const phase: Phase = { kind: "joining", resolve, reject, cancelTimeout };
      this.#phase = phase;
      // Projections decode and commands encode in `mmorpg-wasm` (#541), the same module the local world loads.
      void loadWasm().then(() => {
        if (this.#phase !== phase) {
          return;
        }
        try {
          this.#open(this.#route.url);
        } catch (error) {
          this.#abortJoin(new Error(`Could not connect to the zone host: ${describe(error)}`));
        }
      }, (error: unknown) => this.#abortJoin(new Error(`Could not load the wire module: ${describe(error)}`)));
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
      case "travelling":
        phase.cancelTimeout();
        break;
      case "connected":
      case "failed":
        break;
    }
    this.#teardown(phase.kind === "connected");
    this.#phase = { kind: "idle" };
  }

  sendCommand(command: WorldCommand): number | null {
    this.#reportFailure();
    if (this.#finished) { return null; }
    const phase = this.#phase;
    if (phase.kind === "reconnecting" || phase.kind === "travelling") {
      // Intent made during an interruption or a zone change is dropped, never replayed after it.
      return null;
    }
    if (phase.kind !== "connected") {
      throw new Error("Join the world before sending commands.");
    }
    const payload = this.#encodeCommand(command);
    if (command.kind === "move") {
      this.#facing = command.facing;
    }
    const sequence = this.#outbox.submit(payload, deliveryOf(command));
    this.#flush();
    return sequence;
  }

  advance(): readonly ZoneSnapshot[] {
    if (this.#phase.kind === "connected" && !this.#finished) {
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
    const kind = this.#phase.kind;
    return kind === "reconnecting" || kind === "travelling" ? kind : "connected";
  }

  /** Opens a session to `url` and makes it the current link; callbacks of replaced links are ignored. */
  #open(url: string): void {
    let link: Link | null = null;
    const connection = this.#connector(url, (datagram) => {
      if (link) {
        this.#receive(link, datagram);
      }
    });
    const opened: Link = { connection, reassembler: new SnapshotReassembler(), welcome: null, admitting: false };
    link = opened;
    this.#link = opened;
    void connection.welcome.then(
      (bytes) => this.#welcomed(opened, bytes),
      (error: unknown) => this.#linkFailed(opened, new Error(describe(error))),
    );
    void connection.closed.then((end) => this.#ended(opened, end));
  }

  #closeLink(release = false): void {
    const link = this.#link;
    this.#link = null;
    if (link && release && this.#durable && link.connection.control) {
      // The bounded exchange owns this retiring connection until persistence
      // succeeds or fails; this source no longer receives its projections.
      void link.connection.control(new Uint8Array([1, 2])).then(
        () => link.connection.close(),
        () => link.connection.close(),
      );
    } else {
      link?.connection.close();
    }
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
      link.admitting = Boolean(link.connection.control);
      void this.#claimCharacter(link).then(() => {
        if (link !== this.#link) { return; }
        link.admitting = false;
        if (!this.#durable) {
          this.#outbox.submit(this.#classChoice(), "reliable");
        }
        try { this.#flush(); } catch (error) {
          this.#linkFailed(link, new Error(describe(error)));
        }
        this.#classSequence = this.#outbox.awaiting;
      }, (error: unknown) => this.#linkFailed(link, new Error(describe(error))));
      return;
    }
    if (phase.kind === "travelling") {
      link.welcome = welcome;
      this.#welcome = welcome;
      link.admitting = true;
      void this.#claimArrival(link).then(() => {
        if (link !== this.#link) { return; }
        link.admitting = false;
        this.#resetHistory = true;
        // The destination applies only commands after the cutover: the sequence carries on, and the
        // character starts there stopped, keeping its facing.
        this.#outbox.restart(this.#encodeCommand({ kind: "move", forward: 0, strafe: 0, facing: this.#facing }));
        try { this.#flush(); } catch (error) {
          this.#fail(new Error(describe(error)));
        }
      }, (error: unknown) => this.#linkFailed(link, new Error(describe(error))));
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
    const stop = this.#encodeCommand({ kind: "move", forward: 0, strafe: 0, facing: this.#facing });
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
    if (link !== this.#link || !welcome || link.admitting) {
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
      if (!this.#activityProtocol && isRedirectNotice(frame.payload)) {
        this.#follow(frame);
      } else {
        this.#project(welcome, frame);
      }
    } catch (error) {
      this.#linkFailed(link, error instanceof Error ? error : new Error(describe(error)));
    }
  }

  /**
   * Follows a redirect notice (#145, TECH-007): the source retired the character, so this source
   * connects to the destination's route and claims the same character there. A notice for another
   * zone or character, back to this zone, of an already followed transfer, or naming a route of a
   * zone other than its destination is refused, and the source's route is never used again.
   */
  #follow(frame: SnapshotFrame): void {
    const notice = decodeRedirectNotice(frame.payload);
    if (this.#phase.kind !== "connected" || !this.#durable || notice.zoneId !== this.#route.zoneId ||
        notice.tick !== frame.tick || notice.destination === this.#route.zoneId || notice.transferId === this.#followed) {
      throw new Error("The zone host sent an unexpected redirect notice.");
    }
    const route = parseSessionRoute(notice.route);
    if (route.zoneId !== notice.destination) {
      throw new Error(`The redirect route ${route.url} does not lead to zone ${notice.destination}.`);
    }
    if (!this.#enterZone) {
      throw new Error(`This page cannot follow the character into zone ${notice.destination}.`);
    }
    this.#followed = notice.transferId;
    this.#closeLink();
    this.#welcome = null;
    this.#route = route;
    this.#contentRevision = this.#enterZone(notice.destination);
    this.#history.reset();
    // Another zone sends its own sheets.
    this.#sheets = new ProjectionDecoder();
    this.#latest = null;
    this.#received = [];
    this.#resumeAvailable = true;
    const cancelTimeout = this.#clock.after(FOLLOW_TIMEOUT_MS, () =>
      this.#fail(new Error(`Zone ${notice.destination} did not take the character within twenty seconds.`)));
    this.#phase = { kind: "travelling", cancelTimeout };
    this.#open(route.url);
  }

  /** Validates and presents one verified snapshot frame of the current session. */
  #project(welcome: Welcome, frame: SnapshotFrame): void {
    const activity = this.#activityProtocol?.decodeProjection(frame.payload);
    const snapshot = activity ? activity.snapshot : this.#sheets.decode(frame.payload);
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
    this.#finished = activity?.finished ?? false;
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
    } else if (phase.kind === "travelling") {
      // The character went live at the destination.
      phase.cancelTimeout();
      this.#phase = { kind: "connected" };
    }
    if (this.#finished) {
      // Retain the final projection for result presentation, with no more input or reconnects.
      this.#closeLink();
      this.#welcome = null;
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
    } else if (phase.kind === "travelling") {
      phase.cancelTimeout();
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
    if (this.#finished) { return; }
    const link = this.#link;
    if (!link) {
      return;
    }
    for (const datagram of this.#outbox.poll(this.#clock.now())) {
      link.connection.send(datagram);
    }
  }

  /** Forgets the session: closes the link and clears presentation and sequencing. */
  #teardown(release = false): void {
    this.#closeLink(release);
    this.#welcome = null;
    this.#outbox = new SequencedOutbox();
    this.#sheets = new ProjectionDecoder();
    this.#history.reset();
    this.#latest = null;
    this.#latestAt = 0;
    this.#facing = 0;
    this.#resumeAvailable = true;
    this.#resetHistory = false;
    this.#received = [];
    this.#classSequence = null;
    this.#classAcknowledged = false;
    this.#durable = false;
    this.#finished = false;
  }

  async #claimCharacter(link: Link): Promise<void> {
    if (this.#activityProtocol || !link.connection.control) { return; }
    const capability = await link.connection.control(new Uint8Array([1, 0]));
    if (!capability.accepted) { return; }
    if (!sameBytes(capability.payload, new Uint8Array([1, 1]))) {
      throw new Error("The zone host has an unsupported character service.");
    }
    if (link !== this.#link) { return; }
    this.#characterId = await this.#resolveCharacterId(this.#character.id);
    const claimed = await link.connection.control(this.#claimRequest(this.#characterId));
    if (!claimed.accepted || !sameBytes(claimed.payload, new Uint8Array([1, 1]))) {
      throw new Error("The zone host refused this character. It may already be in play.");
    }
    if (link !== this.#link) { return; }
    this.#durable = true;
    this.#classAcknowledged = true;
  }

  #claimRequest(characterId: string): Uint8Array {
    const { classId, sex } = classChoiceCodes(this.#character);
    const id = new TextEncoder().encode(characterId);
    const request = new Uint8Array(3 + id.length);
    request.set([1, 1, 1 + classId * 2 + sex]);
    request.set(id, 3);
    return request;
  }

  /**
   * Claims the followed character at a redirect's destination (#145). The destination refuses it
   * until the source retired the character, so the claim repeats until the follow times out.
   */
  async #claimArrival(link: Link): Promise<void> {
    const control = link.connection.control?.bind(link.connection);
    if (!control || this.#characterId === null) {
      throw new Error("The destination zone host has no character service.");
    }
    const capability = await control(new Uint8Array([1, 0]));
    if (!capability.accepted || !sameBytes(capability.payload, new Uint8Array([1, 1]))) {
      throw new Error("The destination zone host has no supported character service.");
    }
    const request = this.#claimRequest(this.#characterId);
    while (link === this.#link) {
      const claimed = await control(request);
      if (claimed.accepted && sameBytes(claimed.payload, new Uint8Array([1, 1]))) {
        return;
      }
      await new Promise<void>((resolve) => this.#clock.after(CLAIM_RETRY_MS, resolve));
    }
  }

  #encodeCommand(command: WorldCommand): Uint8Array {
    if (this.#activityProtocol) {
      return this.#activityProtocol.encodeCommand(command);
    }
    // Combo inputs report the newest projection they were made against, so the zone can make up
    // for their travel time within its bounded grace (#415); it still judges them on arrival.
    const observed = this.#latest?.tick;
    const payload = observed !== undefined && isTimingSensitive(command)
      ? encodeObservedCommand(command, observed)
      : encodeCommand(command);
    // Every command echoes the sheet revisions held, so the zone stops resending them (#544).
    return this.#sheets.acknowledge(payload);
  }

  #classChoice(): Uint8Array {
    if (this.#activityProtocol) {
      return this.#activityProtocol.initialCommand(this.#character);
    }
    const { classId, sex } = classChoiceCodes(this.#character);
    return encodeCommand({ kind: "choose-class", classId, sex });
  }
}
