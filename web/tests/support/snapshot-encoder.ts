import { entityKindCode, type EntityRef } from "../../src/entity-ref";
import { EMOTES, type AuraState, type CastState, type ZoneEvent, type ZoneSnapshot } from "../../src/replication";

const FIXED_BYTES = 105;
/** Bag 64, equipment 12 and stat totals 8. */
const SHEET_BYTES = 84;
const AURA_KINDS = ["damage-over-time", "heal-over-time", "absorb", "root", "snare", "stun", "haste"];
const CLASSES = ["warden", "ranger", "arcanist"];
const RESOURCES = ["rage", "focus", "mana"];
const EVENT_BYTES = 14;
const ENTITY_BYTES = 21;
const ERROR_CODES = [
  "no-target", "out-of-range", "target-dead", "not-attackable", "you-are-dead", "not-dead", "invalid-target", "too-many-intents", "invalid-inventory-move", "inventory-full", "invalid-loot", "not-loot-owner", "empty-loot", "money-overflow",
  "no-class", "not-learned", "not-ready", "not-enough-resource", "stunned", "already-casting", "invalid-class",
  "not-equippable", "invalid-vendor", "not-enough-money", "chat-throttled", "invalid-quest", "quest-log-full",
  "quest-incomplete",
];
const QUEST_MARKERS = ["available", "in-progress", "complete"];

function flagByte(flags: readonly boolean[]): number {
  return flags.reduce((byte, set, bit) => byte | (set ? 1 << bit : 0), 0);
}

/**
 * Test-only player-visible snapshot v14 encoder (docs/PROTOCOL.md); Rust owns the real one.
 * Like it, positions must fit i16 and velocities saturate to i8.
 */
export function encodeTestSnapshot(snapshot: ZoneSnapshot): Uint8Array {
  let lootBytes = 0;
  if (snapshot.loot !== null) {
    lootBytes = 16 + (snapshot.loot.item === null ? 1 : 5) + (snapshot.loot.questItem === null ? 1 : 5);
  }
  const quests = snapshot.quests;
  if ((quests === null) !== (snapshot.inventory === null)) {
    throw new Error("The quest sheet travels exactly with the bag");
  }
  const questBytes = quests === null ? 0 : 4 + 1 + 4 * quests.entries.length + 1 + 3 * quests.markers.length;
  const lists = 3 * snapshot.cooldowns.length + 6 * (snapshot.auras.length + snapshot.targetDetail.auras.length);
  const chat = snapshot.chat ?? [];
  const chatText = (line: (typeof chat)[number]) =>
    line.channel === "emote" ? null : new TextEncoder().encode(line.text);
  const chatBytes = chat.reduce((total, line) => total + 6 + (chatText(line)?.length ?? 0), 0);
  const size = FIXED_BYTES + lists + (snapshot.inventory === null ? 0 : SHEET_BYTES) + questBytes + lootBytes + EVENT_BYTES * snapshot.events.length + chatBytes + ENTITY_BYTES * snapshot.entities.length;
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  let offset = 0;
  const u8 = (value: number) => { view.setUint8(offset, value); offset += 1; };
  const u16 = (value: number) => { view.setUint16(offset, value); offset += 2; };
  const u32 = (value: number) => { view.setUint32(offset, value); offset += 4; };
  const u64 = (value: bigint) => { view.setBigUint64(offset, value); offset += 8; };
  const entity = (value: EntityRef | null) => {
    u8(value === null ? 0 : entityKindCode(value.kind));
    u32(value?.id ?? 0);
  };
  u8(14);
  u8(2);
  u16(14);
  u32(snapshot.zoneId);
  u64(snapshot.tick);
  u64(snapshot.contentRevision);
  u32(snapshot.acknowledgedSequence);
  u32(snapshot.viewerId);
  const viewer = snapshot.viewer;
  u32(viewer.health);
  u32(viewer.maxHealth);
  u8(viewer.level);
  u32(viewer.experience);
  u32(viewer.experienceToNextLevel);
  u32(viewer.copper);
  u8(flagByte([viewer.dead, viewer.inCombat, viewer.autoAttacking]));
  entity(viewer.target);
  const choice = viewer.classChoice;
  u8(choice === null ? 0 : 1 + CLASSES.indexOf(choice.classId) * 2 + (choice.sex === "female" ? 0 : 1));
  u8(viewer.resource === null ? 0 : RESOURCES.indexOf(viewer.resource.kind) + 1);
  u16(viewer.resource?.value ?? 0);
  u16(viewer.resource?.max ?? 0);
  u16(viewer.globalCooldown);
  const cast = (value: CastState | null) => {
    u8(value?.ability ?? 0);
    u8(value?.channel ? 1 : 0);
    u16(value?.elapsed ?? 0);
    u16(value?.total ?? 0);
  };
  const auras = (values: readonly AuraState[]) => {
    u8(values.length);
    for (const aura of values) {
      u8(aura.ability);
      u8(AURA_KINDS.indexOf(aura.kind) + 1);
      u16(aura.remaining);
      u16(aura.amount);
    }
  };
  cast(viewer.cast);
  u16(viewer.damage.min);
  u16(viewer.damage.max);
  u8(snapshot.cooldowns.length);
  for (const cooldown of snapshot.cooldowns) {
    u8(cooldown.ability);
    u16(cooldown.remaining);
  }
  auras(snapshot.auras);
  entity(snapshot.targetOfTarget);
  cast(snapshot.targetDetail.cast);
  auras(snapshot.targetDetail.auras);
  u64(snapshot.inventoryRevision);
  u8(snapshot.inventory === null ? 0 : 1);
  if (snapshot.inventory !== null) {
    if (snapshot.inventory.length !== 16) {
      throw new Error("Inventory must have sixteen slots");
    }
    for (const slot of snapshot.inventory) {
      u16(slot?.itemId ?? 0);
      u16(slot?.quantity ?? 0);
    }
    if (snapshot.equipment?.length !== 6 || snapshot.stats === null) {
      throw new Error("A sheet needs six equipment slots and stat totals");
    }
    for (const item of snapshot.equipment) {
      u16(item ?? 0);
    }
    const stats = snapshot.stats;
    for (const total of [stats.stamina, stats.strength, stats.agility, stats.intellect]) {
      u16(total);
    }
    u32(quests!.completed);
    u8(quests!.entries.length);
    for (const entry of quests!.entries) {
      u8(entry.quest);
      entry.progress.forEach(u8);
    }
    u8(quests!.markers.length);
    for (const marker of quests!.markers) {
      u16(marker.npc);
      u8(QUEST_MARKERS.indexOf(marker.marker) + 1);
    }
  }
  u8(snapshot.loot === null ? 0 : 1);
  if (snapshot.loot !== null) {
    u32(snapshot.loot.creatureId);
    u64(snapshot.loot.diedAt);
    u32(snapshot.loot.money);
    for (const stack of [snapshot.loot.item, snapshot.loot.questItem]) {
      u8(stack === null ? 0 : 1);
      if (stack !== null) {
        u16(stack.itemId);
        u16(stack.quantity);
      }
    }
  }
  u8(snapshot.events.length);
  for (const event of snapshot.events) {
    encodeEvent(event, u8, u16, entity);
  }
  u8(chat.length);
  for (const line of chat) {
    u32(line.speaker);
    if (line.channel === "emote") {
      u8(2);
      u8(EMOTES.indexOf(line.emote) + 1);
      continue;
    }
    const text = chatText(line)!;
    u8(line.channel === "say" ? 0 : 1);
    u8(text.length);
    bytes.set(text, offset);
    offset += text.length;
  }
  u16(snapshot.entities.length);
  for (const record of snapshot.entities) {
    u8(entityKindCode(record.kind));
    u32(record.entityId);
    u16(record.appearance);
    for (const component of record.position) {
      if (component < -32_768 || component > 32_767) {
        throw new Error("position does not fit i16");
      }
      view.setInt16(offset, component);
      offset += 2;
    }
    for (const component of record.velocity) {
      view.setInt8(offset, Math.max(-128, Math.min(127, component)));
      offset += 1;
    }
    u16(record.facing);
    u8(record.level);
    u8(record.healthPercent);
    const flags = record.flags;
    u8(flagByte([flags.dead, flags.inCombat, flags.hostile, flags.attackable, flags.tappedByOther, flags.evading, flags.targetsViewer, flags.lootable]));
  }
  return bytes;
}

function encodeEvent(
  event: ZoneEvent,
  u8: (value: number) => void,
  u16: (value: number) => void,
  entity: (value: EntityRef | null) => void,
): void {
  switch (event.kind) {
    case "damage-dealt":
    case "damage-taken":
      u8(event.kind === "damage-dealt" ? 1 : 2);
      u8(event.critical ? 1 : 0);
      entity(event.source);
      entity(event.target);
      u16(event.amount);
      return;
    case "miss":
    case "evade":
      u8(event.kind === "miss" ? 3 : 5);
      u8(0);
      entity(event.source);
      entity(event.target);
      u16(0);
      return;
    case "died":
      u8(4);
      u8(0);
      entity(event.killer);
      entity(event.entity);
      u16(0);
      return;
    case "error":
      u8(6);
      u8(0);
      entity(null);
      entity(event.target);
      u16(ERROR_CODES.indexOf(event.code) + 1);
      return;
    case "cast-started":
    case "ability-used":
      u8(event.kind === "cast-started" ? 7 : 8);
      u8(event.ability);
      entity(event.source);
      entity(event.target);
      u16(event.kind === "cast-started" ? event.ticks : 0);
      return;
    case "healed":
    case "aura-applied":
    case "aura-removed":
      u8(event.kind === "healed" ? 9 : event.kind === "aura-applied" ? 10 : 11);
      u8(event.ability);
      entity(event.source);
      entity(event.target);
      u16(event.kind === "healed" ? event.amount : event.kind === "aura-applied" ? event.ticks : 0);
      return;
    case "interrupted":
      u8(12);
      u8(event.ability);
      entity(event.source);
      entity(event.target);
      u16(0);
      return;
    case "absorbed":
      u8(13);
      u8(0);
      entity(event.source);
      entity(event.target);
      u16(event.amount);
      return;
    case "quest-progress":
    case "quest-completed":
      u8(event.kind === "quest-progress" ? 14 : 15);
      u8(event.quest);
      entity(null);
      entity(null);
      u16(event.kind === "quest-progress" ? (event.objective << 8) | event.count : 0);
      return;
  }
}
