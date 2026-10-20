/** Existing game-server reliable-control envelope, independent of command sequences. */
export const MAX_CONTROL_PAYLOAD_BYTES = 4096;
export type ControlReply = { accepted: boolean; payload: Uint8Array };

export function encodeControlRequest(payload: Uint8Array): Uint8Array {
  if (payload.length > MAX_CONTROL_PAYLOAD_BYTES) {
    throw new Error("Control request exceeds the protocol limit.");
  }
  const bytes = new Uint8Array(8 + payload.length);
  bytes.set([71, 83, 67, 84, 1, 1, payload.length >> 8, payload.length & 255]);
  bytes.set(payload, 8);
  return bytes;
}

export function decodeControlReply(bytes: Uint8Array): ControlReply {
  if (bytes.length < 8 || bytes[0] !== 71 || bytes[1] !== 83 || bytes[2] !== 67 || bytes[3] !== 84 ||
      bytes[4] !== 1 || (bytes[5] !== 2 && bytes[5] !== 3)) {
    throw new Error("The zone host sent invalid control data.");
  }
  const length = bytes[6]! * 256 + bytes[7]!;
  if (length > MAX_CONTROL_PAYLOAD_BYTES || bytes.length !== 8 + length) {
    throw new Error("The zone host sent an invalid control length.");
  }
  return { accepted: bytes[5] === 2, payload: bytes.slice(8) };
}
