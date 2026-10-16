import type { SessionConnection, SessionConnector, SessionEnd } from "./connection";
import { WELCOME_BYTES } from "./frames";

/** Stops a misbehaving host from streaming an unbounded "welcome"; one extra byte proves trailing data. */
const MAX_WELCOME_STREAM_BYTES = WELCOME_BYTES + 1;

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Marks a promise whose rejection another consumer observes (or that is
 * deliberately abandoned, like a cancelled join's welcome) as handled, so a
 * dropped session never surfaces as an unhandled rejection.
 */
function handled<T>(promise: Promise<T>): Promise<T> {
  promise.catch(() => undefined);
  return promise;
}

async function readWelcome(transport: WebTransport): Promise<Uint8Array> {
  await transport.ready;
  const streams = transport.incomingUnidirectionalStreams.getReader() as ReadableStreamDefaultReader<ReadableStream<Uint8Array>>;
  const first = await streams.read();
  streams.releaseLock();
  if (first.done) {
    throw new Error("The zone host closed the session before its welcome.");
  }
  const reader = first.value.getReader();
  const bytes = new Uint8Array(MAX_WELCOME_STREAM_BYTES);
  let length = 0;
  for (;;) {
    const { value, done } = await reader.read();
    if (done) {
      return bytes.slice(0, length);
    }
    if (length + value.byteLength > MAX_WELCOME_STREAM_BYTES) {
      await reader.cancel();
      throw new Error("The zone host's welcome is longer than the protocol allows.");
    }
    bytes.set(value, length);
    length += value.byteLength;
  }
}

async function pumpDatagrams(transport: WebTransport, receive: (datagram: Uint8Array) => void): Promise<void> {
  await transport.ready;
  const reader = transport.datagrams.readable.getReader() as ReadableStreamDefaultReader<Uint8Array>;
  for (;;) {
    const { value, done } = await reader.read();
    if (done) {
      return;
    }
    receive(value);
  }
}

/**
 * The browser WebTransport adapter. With a certificate hash the browser
 * trusts exactly that self-signed development certificate (ECDSA, valid at
 * most 14 days, as the WebTransport specification requires); without one the
 * host needs a publicly trusted certificate.
 */
export function webTransportConnector(certificateHash: Uint8Array | null): SessionConnector {
  return (url, receive): SessionConnection => {
    if (typeof WebTransport === "undefined") {
      throw new Error("This browser does not support WebTransport, which online play needs.");
    }
    const options: WebTransportOptions = certificateHash
      ? { serverCertificateHashes: [{ algorithm: "sha-256", value: certificateHash.slice() }] }
      : {};
    const transport = new WebTransport(url, options);
    const welcome = handled(readWelcome(transport));
    const closed = transport.closed.then(
      (info): SessionEnd => ({ kind: "closed", code: info.closeCode ?? 0, reason: info.reason ?? "" }),
      (error: unknown): SessionEnd => ({ kind: "lost", message: describe(error) }),
    );
    // The pump ends with the session; `closed` reports why.
    handled(pumpDatagrams(transport, receive));
    let writer: WritableStreamDefaultWriter<Uint8Array> | null = null;
    let open = true;
    return {
      welcome,
      closed,
      send(datagram) {
        if (!open) {
          return;
        }
        try {
          writer ??= transport.datagrams.writable.getWriter() as WritableStreamDefaultWriter<Uint8Array>;
          // Datagrams are unreliable by contract; a failed write means the session is closing.
          handled(writer.write(datagram));
        } catch {
          // The session is not established or already closed; `closed` reports it.
        }
      },
      close() {
        if (!open) {
          return;
        }
        open = false;
        try {
          transport.close({ closeCode: 0, reason: "client left" });
        } catch {
          // Closing a session that failed to open throws; it is closed either way.
        }
      },
    };
  };
}
