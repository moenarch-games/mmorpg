import { expect, test } from "bun:test";
import { decodeControlReply, encodeControlRequest } from "../src/session/control-wire";

test("character controls use the existing bounded game-server envelope", () => {
  expect([...encodeControlRequest(new Uint8Array([1, 0]))]).toEqual([71, 83, 67, 84, 1, 1, 0, 2, 1, 0]);
  expect(decodeControlReply(new Uint8Array([71, 83, 67, 84, 1, 2, 0, 2, 1, 1]))).toEqual({ accepted: true, payload: new Uint8Array([1, 1]) });
  expect(decodeControlReply(new Uint8Array([71, 83, 67, 84, 1, 3, 0, 0])).accepted).toBe(false);
  expect(() => encodeControlRequest(new Uint8Array(4097))).toThrow();
  expect(() => decodeControlReply(new Uint8Array([71, 83, 67, 84, 1, 2, 0, 1]))).toThrow();
});
