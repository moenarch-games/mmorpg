/**
 * Command wire version 9, mirrored from `mmorpg-protocol` (docs/PROTOCOL.md).
 * `fixtures/protocol/commands-v9.hex` holds both encoders to the same bytes.
 * The session supplies player identity and sequence separately.
 */
import { entityKindCode, isU32, type EntityRef } from "./entity-ref";
import { chatTextError, EMOTES, type EmoteName } from "./replication";

export type Axis = -1 | 0 | 1;

export type WorldCommand =
  | { kind: "move"; forward: Axis; strafe: Axis; facing: number }
  | { kind: "loot"; creatureId: number; diedAt: bigint }
  | { kind: "jump" }
  /** Selects a unit, or clears the selection with `null`. */
  | { kind: "select-target"; target: EntityRef | null }
  | { kind: "start-attack" }
  | { kind: "stop-attack" }
  | { kind: "release-spirit" }
  | { kind: "move-item"; source: number; destination: number; quantity: number }
  /** A class ability at `target`, or at the current selection with `null`. */
  | { kind: "use-ability"; ability: number; target: EntityRef | null }
  | { kind: "cancel-cast" }
  /** Class 0 Warden, 1 Ranger, 2 Arcanist; sex 0 female, 1 male. The zone refuses invalid values. */
  | { kind: "choose-class"; classId: number; sex: number }
  /** Equips the item in a bag slot, swapping any item already in its equipment slot. */
  | { kind: "equip-item"; bagSlot: number }
  /** Equipment slot 0 main hand, 1 off hand, 2 head, 3 chest, 4 legs, 5 feet. */
  | { kind: "unequip-item"; equipmentSlot: number }
  /** Buys `quantity` units of the vendor NPC's offer at index `offer`. */
  | { kind: "buy-item"; npc: number; offer: number; quantity: number }
  /** Sells `quantity` units from one of the player's own bag slots to the vendor NPC. */
  | { kind: "sell-item"; npc: number; bagSlot: number; quantity: number }
  /** Says (20 m) or yells (60 m) one line of 1–80 UTF-8 bytes; the zone rate-limits speakers. */
  | { kind: "chat"; channel: "say" | "yell"; text: string }
  | { kind: "emote"; emote: EmoteName }
  /** Accepts `quest` from its giver `npc`; the zone checks reach and availability. */
  | { kind: "accept-quest"; npc: number; quest: number }
  /** Turns `quest` in at its ender `npc`, taking reward `choice` (0 when it offers none). */
  | { kind: "complete-quest"; npc: number; quest: number; choice: number }
  | { kind: "abandon-quest"; quest: number };

const COMMAND_WIRE_VERSION = 9;
const MOVE_TAG = 1;
const JUMP_TAG = 2;
const SELECT_TARGET_TAG = 3;
const START_ATTACK_TAG = 4;
const STOP_ATTACK_TAG = 5;
const RELEASE_SPIRIT_TAG = 6;
const MOVE_ITEM_TAG = 7;
const LOOT_TAG = 8;
const USE_ABILITY_TAG = 9;
const CANCEL_CAST_TAG = 10;
const CHOOSE_CLASS_TAG = 11;
const EQUIP_ITEM_TAG = 12;
const UNEQUIP_ITEM_TAG = 13;
const BUY_ITEM_TAG = 14;
const SELL_ITEM_TAG = 15;
const CHAT_TAG = 16;
const EMOTE_TAG = 17;
const ACCEPT_QUEST_TAG = 18;
const COMPLETE_QUEST_TAG = 19;
const ABANDON_QUEST_TAG = 20;
const YAW_STEPS = 65_536;

function isAxis(value: number): value is Axis {
  return value === -1 || value === 0 || value === 1;
}

function isU8(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 255;
}

/** Writes a 5-byte entity reference at `offset`; `null` is kind 0 with ID 0. */
function writeEntity(view: DataView, offset: number, target: EntityRef | null): void {
  if (target === null) {
    return;
  }
  if (!isU32(target.id)) {
    throw new Error("Target IDs must be u32.");
  }
  view.setUint8(offset, entityKindCode(target.kind));
  view.setUint32(offset + 1, target.id);
}

export function encodeCommand(command: WorldCommand): Uint8Array {
  switch (command.kind) {
    case "move": {
      if (!isAxis(command.forward) || !isAxis(command.strafe)) {
        throw new Error("Movement components must be -1, 0 or 1.");
      }
      if (!Number.isInteger(command.facing) || command.facing < 0 || command.facing >= YAW_STEPS) {
        throw new Error("Facing must be a u16 yaw.");
      }
      const payload = new Uint8Array(6);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, MOVE_TAG);
      view.setInt8(2, command.forward);
      view.setInt8(3, command.strafe);
      view.setUint16(4, command.facing);
      return payload;
    }
    case "move-item": {
      if (![command.source, command.destination].every((slot) => Number.isInteger(slot) && slot >= 0 && slot <= 255)
        || !Number.isInteger(command.quantity) || command.quantity < 0 || command.quantity > 65_535) {
        throw new Error("Bag move fields must fit u8 slots and a u16 quantity.");
      }
      const payload = new Uint8Array(6);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, MOVE_ITEM_TAG);
      view.setUint8(2, command.source);
      view.setUint8(3, command.destination);
      view.setUint16(4, command.quantity);
      return payload;
    }
    case "loot": {
      if (!isU32(command.creatureId) || command.diedAt < 0n || command.diedAt > 0xffff_ffff_ffff_ffffn) {
        throw new Error("Loot claims require a u32 creature and u64 death tick.");
      }
      const payload = new Uint8Array(14);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, LOOT_TAG);
      view.setUint32(2, command.creatureId);
      view.setBigUint64(6, command.diedAt);
      return payload;
    }
    case "jump":
      return Uint8Array.of(COMMAND_WIRE_VERSION, JUMP_TAG);
    case "select-target": {
      const payload = new Uint8Array(7);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, SELECT_TARGET_TAG);
      writeEntity(view, 2, command.target);
      return payload;
    }
    case "use-ability": {
      if (!isU8(command.ability)) {
        throw new Error("Ability IDs must be u8.");
      }
      const payload = new Uint8Array(8);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, USE_ABILITY_TAG);
      view.setUint8(2, command.ability);
      writeEntity(view, 3, command.target);
      return payload;
    }
    case "cancel-cast":
      return Uint8Array.of(COMMAND_WIRE_VERSION, CANCEL_CAST_TAG);
    case "choose-class": {
      if (!isU8(command.classId) || !isU8(command.sex)) {
        throw new Error("Class choices must fit u8 fields.");
      }
      return Uint8Array.of(COMMAND_WIRE_VERSION, CHOOSE_CLASS_TAG, command.classId, command.sex);
    }
    case "buy-item":
      return encodeTrade(BUY_ITEM_TAG, command.npc, command.offer, command.quantity);
    case "sell-item":
      return encodeTrade(SELL_ITEM_TAG, command.npc, command.bagSlot, command.quantity);
    case "emote": {
      const code = EMOTES.indexOf(command.emote) + 1;
      if (code === 0) {
        throw new Error(`Unknown emote: ${command.emote}`);
      }
      return Uint8Array.of(COMMAND_WIRE_VERSION, EMOTE_TAG, code);
    }
    case "chat": {
      const problem = chatTextError(command.text);
      if (problem !== null) {
        throw new Error(problem);
      }
      const text = new TextEncoder().encode(command.text);
      return Uint8Array.from([COMMAND_WIRE_VERSION, CHAT_TAG, command.channel === "say" ? 0 : 1, text.length, ...text]);
    }
    case "accept-quest":
    case "complete-quest": {
      const choice = command.kind === "complete-quest" ? command.choice : 0;
      if (!isU32(command.npc) || !isU8(command.quest) || !isU8(choice)) {
        throw new Error("Quest commands need a u32 NPC, a u8 quest and a u8 choice.");
      }
      const payload = new Uint8Array(command.kind === "accept-quest" ? 7 : 8);
      const view = new DataView(payload.buffer);
      view.setUint8(0, COMMAND_WIRE_VERSION);
      view.setUint8(1, command.kind === "accept-quest" ? ACCEPT_QUEST_TAG : COMPLETE_QUEST_TAG);
      view.setUint32(2, command.npc);
      view.setUint8(6, command.quest);
      if (command.kind === "complete-quest") {
        view.setUint8(7, choice);
      }
      return payload;
    }
    case "abandon-quest":
      if (!isU8(command.quest)) {
        throw new Error("Quest IDs must be u8.");
      }
      return Uint8Array.of(COMMAND_WIRE_VERSION, ABANDON_QUEST_TAG, command.quest);
    case "equip-item":
      if (!isU8(command.bagSlot)) {
        throw new Error("Bag slots must be u8.");
      }
      return Uint8Array.of(COMMAND_WIRE_VERSION, EQUIP_ITEM_TAG, command.bagSlot);
    case "unequip-item":
      if (!isU8(command.equipmentSlot)) {
        throw new Error("Equipment slots must be u8.");
      }
      return Uint8Array.of(COMMAND_WIRE_VERSION, UNEQUIP_ITEM_TAG, command.equipmentSlot);
    case "start-attack":
      return Uint8Array.of(COMMAND_WIRE_VERSION, START_ATTACK_TAG);
    case "stop-attack":
      return Uint8Array.of(COMMAND_WIRE_VERSION, STOP_ATTACK_TAG);
    case "release-spirit":
      return Uint8Array.of(COMMAND_WIRE_VERSION, RELEASE_SPIRIT_TAG);
    default: {
      const unsupported: never = command;
      throw new Error(`Unsupported command ${String(unsupported)}.`);
    }
  }
}

/** Version, tag, NPC (u32), offer or bag slot (u8), quantity (u16). */
function encodeTrade(tag: number, npc: number, index: number, quantity: number): Uint8Array {
  if (!isU32(npc) || !isU8(index) || !Number.isInteger(quantity) || quantity < 0 || quantity > 0xffff) {
    throw new Error("Trades need a u32 NPC, a u8 offer or bag slot and a u16 quantity.");
  }
  const payload = new Uint8Array(9);
  const view = new DataView(payload.buffer);
  view.setUint8(0, COMMAND_WIRE_VERSION);
  view.setUint8(1, tag);
  view.setUint32(2, npc);
  view.setUint8(6, index);
  view.setUint16(7, quantity);
  return payload;
}
