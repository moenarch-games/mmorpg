import { expect, test } from "bun:test";
import { encodeCommand } from "../src/command-wire";
import { chatTextError, type ChatLine } from "../src/replication";
import { ChatLog, MAX_CHAT_LOG_LINES, parseChatInput } from "../src/world/units/chat-frame";
import { playerEntity, testSnapshot } from "./support/snapshots";

function projection(tick: number, chat: ChatLine[], fields = {}) {
  return testSnapshot({
    zoneId: 1, contentRevision: 4n, viewerId: 1, tick: BigInt(tick), acknowledgedSequence: 0,
    entities: [playerEntity(1, [0, 90, 0])], chat, ...fields,
  });
}

test("typed lines say by default, /y yells and invalid lines are explained", () => {
  expect(parseChatInput("Hail!")).toEqual({ command: { kind: "chat", channel: "say", text: "Hail!" } });
  expect(parseChatInput("/s Hail!")).toEqual({ command: { kind: "chat", channel: "say", text: "Hail!" } });
  expect(parseChatInput("/y To arms!")).toEqual({ command: { kind: "chat", channel: "yell", text: "To arms!" } });
  expect(parseChatInput("/YELL To arms!")).toEqual({ command: { kind: "chat", channel: "yell", text: "To arms!" } });
  expect(parseChatInput("/dance")).toEqual({
    error: "Use /s to say, /y to yell, or /wave, /bow, /cheer, /laugh, /point.",
  });
  expect(parseChatInput("/y   ")).toEqual({ error: "Chat lines are 1 to 80 bytes." });
  expect(parseChatInput("  ")).toEqual({ error: "Chat lines cannot be blank." });
  expect("error" in parseChatInput("x".repeat(81))).toBe(true);
});

test("the text rule matches core: bytes, control characters and Unicode whitespace", () => {
  expect(chatTextError("€".repeat(26))).toBeNull();
  expect(chatTextError("€".repeat(27))).not.toBeNull();
  expect(chatTextError("tab\there")).not.toBeNull();
  expect(chatTextError("  ")).not.toBeNull();
  // U+FEFF is not White_Space in Rust, so the line is valid there and here.
  expect(chatTextError("﻿")).toBeNull();
  expect(() => encodeCommand({ kind: "chat", channel: "say", text: "" })).toThrow();
  expect(Buffer.from(encodeCommand({ kind: "chat", channel: "yell", text: "hi" })).toString("hex")).toBe("0910010268" + "69");
});

test("the log names speakers, notes refusals and ignores stale or foreign projections", () => {
  const log = new ChatLog();
  expect(log.receive(projection(1, [{ speaker: 1, channel: "say", text: "Hail" }, { speaker: 3, channel: "yell", text: "Wolves!" }]))).toBe(true);
  expect(log.entries.map((entry) => entry.text)).toEqual(["You say: Hail", "Player 3 yells: Wolves!"]);
  expect(log.receive(projection(1, [{ speaker: 3, channel: "say", text: "again" }]))).toBe(false);
  expect(log.receive({ ...projection(2, [{ speaker: 3, channel: "say", text: "other" }]), viewerId: 2 })).toBe(false);
  log.receive(projection(2, [], { events: [{ kind: "error", code: "chat-throttled", target: null }] }));
  expect(log.entries.at(-1)).toMatchObject({ kind: "system", text: "You can speak or emote once per second." });
  for (let tick = 3; tick < 3 + MAX_CHAT_LOG_LINES; tick += 1) {
    log.receive(projection(tick, [{ speaker: 2, channel: "say", text: `line ${tick}` }]));
  }
  expect(log.entries.length).toBe(MAX_CHAT_LOG_LINES);
  expect(log.entries.at(-1)?.text).toBe(`Player 2 says: line ${2 + MAX_CHAT_LOG_LINES}`);
  log.reset();
  expect(log.entries).toEqual([]);
  expect(log.receive(projection(1, [{ speaker: 1, channel: "say", text: "fresh" }], { viewerId: 1 }))).toBe(true);
});

test("emote commands send typed emotes and received emotes read as actions", () => {
  expect(parseChatInput("/wave")).toEqual({ command: { kind: "emote", emote: "wave" } });
  expect(parseChatInput("/Bow ")).toEqual({ command: { kind: "emote", emote: "bow" } });
  // An emote takes no text; anything after it is not an emote.
  expect("error" in parseChatInput("/wave hello")).toBe(true);
  expect(Buffer.from(encodeCommand({ kind: "emote", emote: "point" })).toString("hex")).toBe("091105");

  const log = new ChatLog();
  log.receive(projection(1, [{ speaker: 1, channel: "emote", emote: "cheer" }, { speaker: 4, channel: "emote", emote: "laugh" }]));
  expect(log.entries.map(({ kind, text }) => [kind, text])).toEqual([
    ["emote", "You cheer."],
    ["emote", "Player 4 laughs."],
  ]);
});
