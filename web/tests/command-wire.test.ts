import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { encodeCommand, type Axis, type WorldCommand } from "../src/command-wire";
import { entityKindFromCode } from "../src/entity-ref";
import { EMOTES } from "../src/replication";

const fixture = readFileSync(new URL("../../fixtures/protocol/commands-v9.hex", import.meta.url), "utf8");

function axis(raw: string | undefined): Axis {
  const value = Number(raw);
  if (value !== -1 && value !== 0 && value !== 1) {
    throw new Error(`Bad fixture axis ${raw}`);
  }
  return value;
}

function command(fields: readonly string[]): WorldCommand {
  const [name, first, second, third] = fields;
  switch (name) {
    case "move":
      return { kind: "move", forward: axis(first), strafe: axis(second), facing: Number(third) };
    case "move_item":
      return { kind: "move-item", source: Number(first), destination: Number(second), quantity: Number(third) };
    case "loot": {
      if (second === undefined) {
        throw new Error("Missing claim death tick");
      }
      return { kind: "loot", creatureId: Number(first), diedAt: BigInt(second) };
    }
    case "jump":
      return { kind: "jump" };
    case "select_target": {
      const kind = entityKindFromCode(Number(first));
      return { kind: "select-target", target: kind === null ? null : { kind, id: Number(second) } };
    }
    case "start_attack":
      return { kind: "start-attack" };
    case "stop_attack":
      return { kind: "stop-attack" };
    case "release_spirit":
      return { kind: "release-spirit" };
    case "use_ability": {
      const kind = entityKindFromCode(Number(second));
      return { kind: "use-ability", ability: Number(first), target: kind === null ? null : { kind, id: Number(third) } };
    }
    case "cancel_cast":
      return { kind: "cancel-cast" };
    case "choose_class":
      return { kind: "choose-class", classId: Number(first), sex: Number(second) };
    case "equip_item":
      return { kind: "equip-item", bagSlot: Number(first) };
    case "unequip_item":
      return { kind: "unequip-item", equipmentSlot: Number(first) };
    case "buy_item":
      return { kind: "buy-item", npc: Number(first), offer: Number(second), quantity: Number(third) };
    case "chat":
      return { kind: "chat", channel: first === "yell" ? "yell" : "say", text: Buffer.from(second ?? "", "hex").toString("utf8") };
    case "sell_item":
      return { kind: "sell-item", npc: Number(first), bagSlot: Number(second), quantity: Number(third) };
    case "emote":
      return { kind: "emote", emote: EMOTES[Number(first) - 1]! };
    case "accept_quest":
      return { kind: "accept-quest", npc: Number(first), quest: Number(second) };
    case "complete_quest":
      return { kind: "complete-quest", npc: Number(first), quest: Number(second), choice: Number(third) };
    case "abandon_quest":
      return { kind: "abandon-quest", quest: Number(first) };
    default:
      throw new Error(`Unknown fixture command ${name}`);
  }
}

function fixtureCommands(): { hex: string; command: WorldCommand }[] {
  return fixture
    .split("\n")
    .filter((line) => line.length > 0 && !line.startsWith("#"))
    .map((line) => {
      const [hex, ...fields] = line.split(" ");
      if (!hex) {
        throw new Error("Empty fixture line");
      }
      return { hex, command: command(fields) };
    });
}

describe("Rust/browser command contract", () => {
  test("encodes the same golden bytes as mmorpg-protocol", () => {
    const commands = fixtureCommands();
    const kinds = new Set(commands.map(({ command }) => command.kind));
    expect([...kinds].sort()).toEqual([
      "abandon-quest", "accept-quest", "buy-item", "cancel-cast", "chat", "choose-class", "complete-quest", "emote", "equip-item", "jump",
      "loot", "move", "move-item", "release-spirit", "select-target", "sell-item", "start-attack", "stop-attack", "unequip-item",
      "use-ability",
    ]);
    expect(commands.filter(({ command }) => command.kind === "move").length).toBeGreaterThanOrEqual(4);
    expect(commands.filter(({ command }) => command.kind === "select-target").length).toBe(4);
    for (const { hex, command } of commands) {
      expect(Buffer.from(encodeCommand(command)).toString("hex")).toBe(hex);
    }
  });

  test("rejects intent the Rust decoder would reject", () => {
    for (const invalid of [
      { kind: "move", forward: 2, strafe: 0, facing: 0 },
      { kind: "move", forward: 0, strafe: -2, facing: 0 },
      { kind: "move", forward: 0, strafe: 0, facing: 65_536 },
      { kind: "move", forward: 0, strafe: 0, facing: -1 },
      { kind: "move", forward: 0, strafe: 0, facing: 1.5 },
      { kind: "select-target", target: { kind: "creature", id: -1 } },
      { kind: "select-target", target: { kind: "creature", id: 2 ** 32 } },
      { kind: "select-target", target: { kind: "npc", id: 0.5 } },
      { kind: "loot", creatureId: -1, diedAt: 0n },
      { kind: "loot", creatureId: 2 ** 32, diedAt: 0n },
      { kind: "loot", creatureId: 1, diedAt: -1n },
      { kind: "loot", creatureId: 1, diedAt: 1n << 64n },
      { kind: "use-ability", ability: 256, target: null },
      { kind: "use-ability", ability: -1, target: null },
      { kind: "use-ability", ability: 1, target: { kind: "creature", id: 2 ** 32 } },
      { kind: "emote", emote: "dance" },
      { kind: "choose-class", classId: 0, sex: 256 },
      { kind: "choose-class", classId: 1.5, sex: 0 },
      { kind: "accept-quest", npc: -1, quest: 1 },
      { kind: "accept-quest", npc: 1, quest: 256 },
      { kind: "complete-quest", npc: 1, quest: 1, choice: 256 },
      { kind: "abandon-quest", quest: -1 },
      { kind: "equip-item", bagSlot: 256 },
      { kind: "equip-item", bagSlot: -1 },
      { kind: "unequip-item", equipmentSlot: 0.5 },
    ]) {
      expect(() => encodeCommand(invalid as WorldCommand)).toThrow();
    }
  });
});
