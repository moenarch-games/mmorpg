import { expect, test } from "bun:test";
import { encodeCommand } from "../src/command-wire";
import { wasm } from "../src/wasm";
import { playerEntity, testSnapshot } from "./support/snapshots";
import { PROTOCOL_BYTE } from "./support/wire";

// Chat parsing and the log are `mmorpg-client-model`'s (#543) and tested there; these cover the
// browser's boundary to them.

/** The protocol byte every command leads with, as hex. */
const P = PROTOCOL_BYTE.toString(16).padStart(2, "0");

test("a parsed line crosses as the command the wire encodes", () => {
  const yell = wasm.parseChatInput("/y hi");
  expect(yell).toEqual({ command: { kind: "chat", channel: "yell", text: "hi" } });
  expect("command" in yell && Buffer.from(encodeCommand(yell.command)).toString("hex")).toBe(`${P}10010268` + "69");
  const emote = wasm.parseChatInput("/point");
  expect("command" in emote && Buffer.from(encodeCommand(emote.command)).toString("hex")).toBe(`${P}1105`);
  expect(wasm.parseChatInput("  ")).toEqual({ error: "Chat text must not be blank." });
  expect(() => encodeCommand({ kind: "chat", channel: "say", text: "" })).toThrow();
});

test("the log reads a received projection once", () => {
  const log = new wasm.ChatLog();
  const projection = testSnapshot({
    zoneId: 1, contentRevision: 4n, viewerId: 1, tick: 1n, acknowledgedSequence: 0,
    entities: [playerEntity(1, [0, 90, 0])], chat: [{ speaker: 3, channel: "emote", emote: "laugh" }],
  });
  expect(log.receive(projection)).toBe(true);
  expect(log.receive(projection)).toBe(false);
  expect(log.entries()).toEqual([{ kind: "emote", text: "Player 3 laughs." }]);
  expect(log.added()).toBe(1);
  log.reset();
  expect(log.entries()).toEqual([]);
});
