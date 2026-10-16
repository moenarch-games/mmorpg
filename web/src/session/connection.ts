/**
 * The transport seam of the online world source: one WebTransport session to
 * a zone host, reduced to what the game-server session protocol uses. The
 * browser adapter is `webTransportConnector`; tests use a protocol-faithful
 * in-memory host.
 */
export type SessionConnection = {
  /**
   * Every byte of the first unidirectional stream the host opens: its
   * welcome. Rejects when the session cannot be established, for example
   * because the host refused the admission or the resume.
   */
  readonly welcome: Promise<Uint8Array>;
  /** Settles once when the session ends, for whatever reason. Never rejects. */
  readonly closed: Promise<SessionEnd>;
  /** Queues one datagram. Datagrams may be lost; a closed session drops them. */
  send(datagram: Uint8Array): void;
  /** Closes the session from this side; idempotent. */
  close(): void;
};

/**
 * How a session ended. Browsers report a host's application close and a lost
 * path alike, as `lost`; only a WebTransport session close carries a code.
 */
export type SessionEnd =
  | { kind: "closed"; code: number; reason: string }
  | { kind: "lost"; message: string };

/**
 * Opens a session to `url`; `receive` gets every datagram the host sends on
 * it. Throws when the browser cannot open WebTransport sessions at all.
 */
export type SessionConnector = (url: string, receive: (datagram: Uint8Array) => void) => SessionConnection;

/** Wall-clock milliseconds and timers for the source; tests substitute a manual clock. */
export type SessionClock = {
  now(): number;
  /** Runs `callback` after `milliseconds`; the returned function cancels it. */
  after(milliseconds: number, callback: () => void): () => void;
};

export const browserClock: SessionClock = {
  now: () => performance.now(),
  after(milliseconds, callback) {
    const handle = setTimeout(callback, milliseconds);
    return () => clearTimeout(handle);
  },
};
