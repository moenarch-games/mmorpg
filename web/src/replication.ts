/** Player-visible protocol v14 only. Canonical recovery state never enters rendering. */
import { entityKindFromCode, sameEntity, type EntityKind, type EntityRef } from "./entity-ref";

export type { EntityKind, EntityRef } from "./entity-ref";
export type Vector3 = readonly [number, number, number];

/** Presentation flags of a visible unit (wire bits 0–7). */
export type EntityFlags = {
  dead: boolean;
  inCombat: boolean;
  /** An aggressive creature. */
  hostile: boolean;
  /** The viewer may attack it (living creatures). */
  attackable: boolean;
  /** A creature tapped by another player. */
  tappedByOther: boolean;
  evading: boolean;
  /** Its own target is the viewer. */
  targetsViewer: boolean;
  lootable: boolean;
};

export type EntityState = {
  kind: EntityKind;
  entityId: number;
  /** Creature template ID, NPC ID, or for players 0 without a class and otherwise `1 + class × 2 + sex`. */
  appearance: number;
  position: Vector3;
  /** Presentation velocity in units per tick, saturated to the i8 wire range. */
  velocity: Vector3;
  /** u16 yaw: 65 536 steps per turn, 0 faces +Z, increasing turns toward +X. */
  facing: number;
  level: number;
  /** Rounded up, so living units never show 0. */
  healthPercent: number;
  flags: EntityFlags;
};

export type ClassId = "warden" | "ranger" | "arcanist";
export type ResourceKind = "rage" | "focus" | "mana";
export type ClassChoice = { classId: ClassId; sex: "female" | "male" };
export type ResourceState = { kind: ResourceKind; value: number; max: number };
/** A cast or channel in progress: `elapsed` of `total` ticks. */
export type CastState = { ability: number; elapsed: number; total: number; channel: boolean };
export type AuraKind = "damage-over-time" | "heal-over-time" | "absorb" | "root" | "snare" | "stun" | "haste";
/** `amount`: total of a damage/heal over time, remaining shield, snare/haste percent, 0 otherwise. */
export type AuraState = { ability: number; kind: AuraKind; remaining: number; amount: number };
export type CooldownState = { ability: number; remaining: number };

/** The viewer's own exact state. */
export type ViewerState = {
  copper: number;
  experience: number;
  experienceToNextLevel: number;
  health: number;
  maxHealth: number;
  level: number;
  dead: boolean;
  inCombat: boolean;
  autoAttacking: boolean;
  target: EntityRef | null;
  classChoice: ClassChoice | null;
  /** Present exactly when the viewer has a class. */
  resource: ResourceState | null;
  cast: CastState | null;
  globalCooldown: number;
  /** Inclusive melee damage range, the equipment bonus included. */
  damage: { min: number; max: number };
};

/** The sums of the equipped items' attributes. */
export type StatTotals = { stamina: number; strength: number; agility: number; intellect: number };

export type ErrorCode =
  | "no-target"
  | "out-of-range"
  | "target-dead"
  | "not-attackable"
  | "you-are-dead"
  | "not-dead"
  | "invalid-target"
  | "too-many-intents"
  | "invalid-inventory-move"
  | "inventory-full"
  | "invalid-loot"
  | "not-loot-owner"
  | "empty-loot"
  | "money-overflow"
  | "no-class"
  | "not-learned"
  | "not-ready"
  | "not-enough-resource"
  | "stunned"
  | "already-casting"
  | "invalid-class"
  | "not-equippable"
  | "invalid-vendor"
  | "not-enough-money"
  | "chat-throttled"
  | "invalid-quest"
  | "quest-log-full"
  | "quest-incomplete";

/** Feedback the viewer received in the projection's tick; cosmetic and lossy. */
export type ZoneEvent =
  | { kind: "damage-dealt" | "damage-taken"; source: EntityRef; target: EntityRef; amount: number; critical: boolean }
  | { kind: "miss" | "evade"; source: EntityRef; target: EntityRef }
  | { kind: "died"; entity: EntityRef; killer: EntityRef | null }
  | { kind: "error"; code: ErrorCode; target: EntityRef | null }
  | { kind: "cast-started"; source: EntityRef; target: EntityRef | null; ability: number; ticks: number }
  | { kind: "ability-used"; source: EntityRef; target: EntityRef | null; ability: number }
  | { kind: "healed"; source: EntityRef; target: EntityRef; ability: number; amount: number }
  | { kind: "aura-applied"; source: EntityRef; target: EntityRef; ability: number; ticks: number }
  | { kind: "aura-removed"; source: EntityRef; target: EntityRef; ability: number }
  | { kind: "interrupted"; source: EntityRef | null; target: EntityRef; ability: number }
  | { kind: "absorbed"; source: EntityRef; target: EntityRef; amount: number }
  /** Objective `objective` (its index) of `quest` reached `count`. */
  | { kind: "quest-progress"; quest: number; objective: number; count: number }
  /** The viewer turned `quest` in. */
  | { kind: "quest-completed"; quest: number };

/** Item catalog revision 3, validated against the core's immutable stack limits. */
export type InventorySlot = { itemId: number; quantity: number } | null;

export type LootView = {
  creatureId: number;
  diedAt: bigint;
  money: number;
  item: Exclude<InventorySlot, null> | null;
  /** One unit of a quest item, dropped while a quest of the viewer needs it. */
  questItem: Exclude<InventorySlot, null> | null;
};

/** An active quest and the current progress of each objective slot (collect counts included). */
export type QuestEntry = { quest: number; progress: readonly [number, number, number] };
/** `available` is `!`, `complete` is `?` and `in-progress` is a grey `?`. */
export type QuestMarkerKind = "available" | "in-progress" | "complete";
export type QuestMarker = { npc: number; marker: QuestMarkerKind };
/** The viewer's quests: turned-in quests (bit `id - 1`), the log and the markers of quest NPCs. */
export type QuestSheet = {
  completed: number;
  entries: readonly QuestEntry[];
  markers: readonly QuestMarker[];
};

export type ZoneSnapshot = {
  zoneId: number;
  tick: bigint;
  contentRevision: bigint;
  acknowledgedSequence: number;
  /** The player this projection is addressed to ("self"). */
  viewerId: number;
  viewer: ViewerState;
  /** Running cooldowns in ability order. */
  cooldowns: readonly CooldownState[];
  /** The viewer's auras in slot order. */
  auras: readonly AuraState[];
  /** The viewer's target's own target. */
  targetOfTarget: EntityRef | null;
  /** The viewer's target's cast and auras; empty without a living visible target. */
  targetDetail: { cast: CastState | null; auras: readonly AuraState[] };
  inventoryRevision: bigint;
  /** Complete self bag when present; null means retain prior state, never empty. */
  inventory: readonly InventorySlot[] | null;
  /**
   * Item IDs by equipment slot (0 main hand … 5 feet), null when empty; present exactly with the bag
   * under the same revision.
   */
  equipment: readonly (number | null)[] | null;
  /** The equipment's stat totals, present exactly with the equipment. */
  stats: StatTotals | null;
  /** Quest log, turned-in quests and NPC markers, present exactly with the bag. */
  quests: QuestSheet | null;
  /** Complete eligible selected corpse sheet; null means no current sheet. */
  loot: LootView | null;
  events: readonly ZoneEvent[];
  /** Chat lines the viewer heard this tick, in delivery order; cosmetic and lossy like events. */
  chat: readonly ChatLine[];
  /** Priority order: the viewer, its target, then nearest first. */
  entities: readonly EntityState[];
};

/** A line the viewer heard: player `speaker` said or yelled `text`. */
/** The core emote vocabulary (#69), in wire order: wire ID = index + 1. */
export const EMOTES = ["wave", "bow", "cheer", "laugh", "point"] as const;
export type EmoteName = (typeof EMOTES)[number];

export type ChatLine =
  | { speaker: number; channel: "say" | "yell"; text: string }
  | { speaker: number; channel: "emote"; emote: EmoteName };

/**
 * The core chat text rule: 1–80 UTF-8 bytes, at least one non-whitespace character and no control
 * characters. Returns null when `text` is not a valid line.
 */
export function chatTextError(text: string): string | null {
  const length = new TextEncoder().encode(text).length;
  if (length === 0 || length > MAX_CHAT_BYTES) {
    return "Chat lines are 1 to 80 bytes.";
  }
  if (/\p{Cc}/u.test(text)) {
    return "Chat lines cannot contain control characters.";
  }
  // Rust's `str::trim` removes Unicode White_Space, which differs from JavaScript's `trim` (U+FEFF).
  if (/^\p{White_Space}*$/u.test(text)) {
    return "Chat lines cannot be blank.";
  }
  return null;
}

function decodeChatText(bytes: Uint8Array): string {
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    throw new Error("Chat text is not UTF-8");
  }
  if (chatTextError(text) !== null) {
    throw new Error("Invalid chat text");
  }
  return text;
}

export const TICK_HZ = 30;
export const UNITS_PER_METRE = 100;
const YAW_STEPS = 65_536;
const WIRE_VERSION = 14;
const SCHEMA_VERSION = 14;
const PLAYER_SCOPE = 2;
/** One datagram: the measured 1 161-byte floor minus the 20-byte session header and 64 bytes of margin. */
const MAX_PROJECTION_BYTES = 1_077;
/**
 * Common prefix 16, revision 8, acknowledged sequence 4, viewer 4, self 27, self abilities 20 (class,
 * resource, global cooldown, cast, melee damage range, two list counts), target 12 (target-of-target,
 * its cast, aura count), sheet revision 8/presence 1, loot presence 1, event, chat and entity counts.
 * The sheet (bag, equipment, stat totals, quests) and the corpse sheet follow their presence bytes.
 */
const FIXED_BYTES = 105;
const ENTITY_BYTES = 21;
const MAX_EVENTS = 16;
/** (1 077 − 105) / 21: records that fit without events, sheets, cooldowns or auras. */
const MAX_ENTITIES = 46;
const MAX_COOLDOWNS = 4;
const MAX_AURAS = 8;
export const GLOBAL_COOLDOWN_TICKS = 45;
const BUFFER_CAPACITY = 32;

const ERROR_CODES: readonly ErrorCode[] = [
  "no-target", "out-of-range", "target-dead", "not-attackable", "you-are-dead", "not-dead", "invalid-target",
  "too-many-intents", "invalid-inventory-move", "inventory-full",
  "invalid-loot", "not-loot-owner", "empty-loot", "money-overflow",
  "no-class", "not-learned", "not-ready", "not-enough-resource", "stunned", "already-casting", "invalid-class",
  "not-equippable", "invalid-vendor", "not-enough-money", "chat-throttled", "invalid-quest", "quest-log-full",
  "quest-incomplete",
];
const MAX_QUESTS = 32;
const MAX_QUEST_LOG = 10;
const MAX_QUEST_OBJECTIVES = 3;
const MAX_QUEST_NPCS = 8;
const QUEST_MARKERS: readonly QuestMarkerKind[] = ["available", "in-progress", "complete"];
const MAX_CHAT = 4;
const MAX_CHAT_BYTES = 80;
const EQUIPMENT_SLOTS = 6;

type ItemShape = { maxStack: number; slot: number | null; stats: readonly [number, number, number, number] };
const bagItem = (maxStack: number): ItemShape => ({ maxStack, slot: null, stats: [0, 0, 0, 0] });
const gear = (slot: number, ...stats: [number, number, number, number]): ItemShape => ({ maxStack: 1, slot, stats });
/**
 * Item catalog revision 3 by ID: stack limit, equipment slot index and stamina/strength/agility/intellect;
 * `local-zone-wasm.test.ts` holds it to the WASM catalog export.
 */
export const ITEM_SHAPES: ReadonlyMap<number, ItemShape> = new Map([
  [1, bagItem(20)],
  [2, gear(0, 0, 2, 2, 0)],
  [3, gear(0, 1, 3, 0, 0)],
  [4, gear(0, 0, 0, 0, 4)],
  [5, gear(1, 2, 0, 0, 0)],
  [6, gear(2, 1, 0, 0, 2)],
  [7, gear(3, 2, 0, 0, 0)],
  [8, gear(4, 1, 0, 0, 0)],
  [9, gear(5, 1, 0, 2, 0)],
  [10, bagItem(10)],
]);
const CLASSES: readonly ClassId[] = ["warden", "ranger", "arcanist"];
const RESOURCES: readonly ResourceKind[] = ["rage", "focus", "mana"];
const AURA_KINDS: readonly AuraKind[] = ["damage-over-time", "heal-over-time", "absorb", "root", "snare", "stun", "haste"];

type AbilityShape = { castTicks: number; channel: boolean; cooldown: number; aura: AuraKind | null; auraTicks: number };
const instant = (cooldown: number, aura: AuraKind | null = null, auraTicks = 0): AbilityShape =>
  ({ castTicks: 0, channel: false, cooldown, aura, auraTicks });
/**
 * Ability catalog revision 1 by ID, the wire facts the decoder validates (cast time, channel, cooldown
 * and aura); `local-zone-wasm.test.ts` holds it to the WASM catalog export.
 */
export const ABILITY_SHAPES: ReadonlyMap<number, AbilityShape> = new Map([
  [1, instant(0)],
  [2, instant(360, "stun", 60)],
  [3, instant(900, "heal-over-time", 300)],
  [4, instant(180)],
  [5, instant(180)],
  [6, instant(0, "damage-over-time", 450)],
  [7, instant(360, "snare", 120)],
  [8, instant(900, "haste", 300)],
  [9, { castTicks: 60, channel: false, cooldown: 0, aura: null, auraTicks: 0 }],
  [10, instant(600, "root", 180)],
  [11, instant(900, "absorb", 600)],
  [12, { castTicks: 180, channel: true, cooldown: 0, aura: null, auraTicks: 0 }],
  [13, { castTicks: 45, channel: false, cooldown: 240, aura: null, auraTicks: 0 }],
  [14, { castTicks: 90, channel: false, cooldown: 600, aura: null, auraTicks: 0 }],
]);
/** Mana: 110 + 22 per level above 1; rage and focus: 100. */
function resourceMax(kind: ResourceKind, level: number): number {
  return kind === "mana" ? 110 + 22 * (level - 1) : 100;
}

class Reader {
  readonly #view: DataView;
  #offset = 0;

  constructor(view: DataView) {
    this.#view = view;
  }

  get offset(): number {
    return this.#offset;
  }

  #advance(width: number): number {
    const at = this.#offset;
    if (at + width > this.#view.byteLength) {
      throw new Error("Truncated snapshot");
    }
    this.#offset += width;
    return at;
  }

  u8(): number {
    return this.#view.getUint8(this.#advance(1));
  }

  i8(): number {
    return this.#view.getInt8(this.#advance(1));
  }

  u16(): number {
    return this.#view.getUint16(this.#advance(2));
  }

  i16(): number {
    return this.#view.getInt16(this.#advance(2));
  }

  u32(): number {
    return this.#view.getUint32(this.#advance(4));
  }

  u64(): bigint {
    return this.#view.getBigUint64(this.#advance(8));
  }

  bytes(length: number): Uint8Array {
    const at = this.#advance(length);
    return new Uint8Array(this.#view.buffer, this.#view.byteOffset + at, length);
  }

  entity(): EntityRef | null {
    const kind = entityKindFromCode(this.u8());
    const id = this.u32();
    if (kind === null) {
      if (id !== 0) {
        throw new Error("An absent entity must have ID 0");
      }
      return null;
    }
    return { kind, id };
  }

  flags(count: number): boolean[] {
    const byte = this.u8();
    if (byte >> count !== 0) {
      throw new Error("Reserved flag bits are set");
    }
    return Array.from({ length: count }, (_, bit) => (byte & (1 << bit)) !== 0);
  }
}

function decodeEvent(reader: Reader): ZoneEvent {
  const kind = reader.u8();
  const flags = reader.u8();
  const source = reader.entity();
  const target = reader.entity();
  const amount = reader.u16();
  const critical = (allowed: boolean): boolean => {
    if (flags === 0 || (allowed && flags === 1)) {
      return flags === 1;
    }
    throw new Error("Invalid event flags");
  };
  const pair = (): [EntityRef, EntityRef] => {
    if (source === null || target === null) {
      throw new Error("Malformed event record");
    }
    return [source, target];
  };
  const noAmount = () => {
    if (amount !== 0) {
      throw new Error("Malformed event record");
    }
  };
  switch (kind) {
    case 1:
    case 2: {
      const isCritical = critical(true);
      const [from, to] = pair();
      return { kind: kind === 1 ? "damage-dealt" : "damage-taken", source: from, target: to, amount, critical: isCritical };
    }
    case 3:
    case 5: {
      critical(false);
      const [from, to] = pair();
      noAmount();
      return { kind: kind === 3 ? "miss" : "evade", source: from, target: to };
    }
    case 4: {
      critical(false);
      noAmount();
      if (target === null) {
        throw new Error("Malformed event record");
      }
      return { kind: "died", entity: target, killer: source };
    }
    case 6: {
      critical(false);
      const code = ERROR_CODES[amount - 1];
      if (source !== null || code === undefined) {
        throw new Error("Unknown error code");
      }
      return { kind: "error", code, target };
    }
    case 7:
    case 8: {
      const ability = knownAbility(flags);
      if (source === null) {
        throw new Error("Malformed event record");
      }
      if (kind === 7) {
        return { kind: "cast-started", source, target, ability, ticks: amount };
      }
      noAmount();
      return { kind: "ability-used", source, target, ability };
    }
    case 9:
    case 10:
    case 11: {
      const ability = knownAbility(flags);
      const [from, to] = pair();
      if (kind === 9) {
        return { kind: "healed", source: from, target: to, ability, amount };
      }
      if (kind === 10) {
        return { kind: "aura-applied", source: from, target: to, ability, ticks: amount };
      }
      noAmount();
      return { kind: "aura-removed", source: from, target: to, ability };
    }
    case 12: {
      const ability = knownAbility(flags);
      if (target === null) {
        throw new Error("Malformed event record");
      }
      noAmount();
      return { kind: "interrupted", source, target, ability };
    }
    case 13: {
      critical(false);
      const [from, to] = pair();
      return { kind: "absorbed", source: from, target: to, amount };
    }
    case 14:
    case 15: {
      const quest = questId(flags);
      if (source !== null || target !== null) {
        throw new Error("Malformed event record");
      }
      if (kind === 15) {
        noAmount();
        return { kind: "quest-completed", quest };
      }
      const objective = amount >> 8;
      const count = amount & 0xff;
      if (objective >= MAX_QUEST_OBJECTIVES || count === 0) {
        throw new Error("Malformed event record");
      }
      return { kind: "quest-progress", quest, objective, count };
    }
    default:
      throw new Error("Unknown event kind");
  }
}

function questId(id: number): number {
  if (id < 1 || id > MAX_QUESTS) {
    throw new Error("Quest id is out of range");
  }
  return id;
}

/** Whether `quest` is turned in according to a completed mask. */
export function questCompleted(completed: number, quest: number): boolean {
  return quest >= 1 && quest <= MAX_QUESTS && ((completed >>> (quest - 1)) & 1) === 1;
}

/** Ordered in-range entries that are not also turned in; ordered distinct markers with known codes. */
function decodeQuests(reader: Reader): QuestSheet {
  const completed = reader.u32();
  const count = reader.u8();
  if (count > MAX_QUEST_LOG) {
    throw new Error("Quest log exceeds its capacity");
  }
  const entries: QuestEntry[] = [];
  for (let index = 0; index < count; index += 1) {
    const quest = questId(reader.u8());
    const progress = [reader.u8(), reader.u8(), reader.u8()] as const;
    const previous = entries.at(-1);
    if ((previous && previous.quest >= quest) || questCompleted(completed, quest)) {
      throw new Error("Inconsistent quest log");
    }
    entries.push({ quest, progress });
  }
  const markerCount = reader.u8();
  if (markerCount > MAX_QUEST_NPCS) {
    throw new Error("Quest sheet has too many markers");
  }
  const markers: QuestMarker[] = [];
  for (let index = 0; index < markerCount; index += 1) {
    const npc = reader.u16();
    const marker = QUEST_MARKERS[reader.u8() - 1];
    const previous = markers.at(-1);
    if (marker === undefined || (previous && previous.npc >= npc)) {
      throw new Error("Inconsistent quest marker");
    }
    markers.push({ npc, marker });
  }
  return { completed, entries, markers };
}

function knownAbility(id: number): number {
  if (!ABILITY_SHAPES.has(id)) {
    throw new Error("Unknown ability");
  }
  return id;
}

/** A cast names a catalog ability with its exact cast time and channel flag, and is in progress. */
function decodeCast(reader: Reader): CastState | null {
  const ability = reader.u8();
  const [channel = false] = reader.flags(1);
  const elapsed = reader.u16();
  const total = reader.u16();
  if (ability === 0) {
    if (channel || elapsed !== 0 || total !== 0) {
      throw new Error("Inconsistent cast state");
    }
    return null;
  }
  const shape = ABILITY_SHAPES.get(ability);
  if (!shape || shape.castTicks !== total || shape.channel !== channel || total === 0 || elapsed >= total) {
    throw new Error("Inconsistent cast state");
  }
  return { ability, elapsed, total, channel };
}

function decodeAuras(reader: Reader): AuraState[] {
  const count = reader.u8();
  if (count > MAX_AURAS) {
    throw new Error("Snapshot has too many auras");
  }
  const auras: AuraState[] = [];
  for (let index = 0; index < count; index += 1) {
    const ability = reader.u8();
    const kind = AURA_KINDS[reader.u8() - 1];
    const remaining = reader.u16();
    const amount = reader.u16();
    const shape = ABILITY_SHAPES.get(ability);
    if (!shape || kind === undefined || shape.aura !== kind || remaining === 0 || remaining > shape.auraTicks) {
      throw new Error("Inconsistent aura record");
    }
    auras.push({ ability, kind, remaining, amount });
  }
  return auras;
}

function decodeEntity(reader: Reader): EntityState {
  const kind = entityKindFromCode(reader.u8());
  if (kind === null) {
    throw new Error("Unknown entity kind");
  }
  const entityId = reader.u32();
  const appearance = reader.u16();
  const position: Vector3 = [reader.i16(), reader.i16(), reader.i16()];
  const velocity: Vector3 = [reader.i8(), reader.i8(), reader.i8()];
  const facing = reader.u16();
  const level = reader.u8();
  const healthPercent = reader.u8();
  if (healthPercent > 100) {
    throw new Error("Health percent exceeds 100");
  }
  const [dead = false, inCombat = false, hostile = false, attackable = false, tappedByOther = false, evading = false,
    targetsViewer = false, lootable = false] = reader.flags(8);
  if (lootable && (kind !== "creature" || !dead || healthPercent !== 0 || attackable || tappedByOther)) {
    throw new Error("Lootable entity flags are inconsistent");
  }
  return {
    kind, entityId, appearance, position, velocity, facing, level, healthPercent,
    flags: { dead, inCombat, hostile, attackable, tappedByOther, evading, targetsViewer, lootable },
  };
}

function validateStack(itemId: number, quantity: number): void {
  const limit = ITEM_SHAPES.get(itemId)?.maxStack ?? 0;
  if (quantity === 0 || quantity > limit) {
    throw new Error("Invalid inventory stack");
  }
}

/** Six slots of catalog items made for them, then stat totals that match them. */
function decodeEquipment(reader: Reader): { equipment: (number | null)[]; stats: StatTotals } {
  const equipment: (number | null)[] = [];
  const totals = [0, 0, 0, 0];
  for (let slot = 0; slot < EQUIPMENT_SLOTS; slot += 1) {
    const itemId = reader.u16();
    if (itemId === 0) {
      equipment.push(null);
      continue;
    }
    const shape = ITEM_SHAPES.get(itemId);
    if (shape?.slot !== slot) {
      throw new Error("Invalid equipment slot");
    }
    shape.stats.forEach((value, stat) => { totals[stat] = (totals[stat] ?? 0) + value; });
    equipment.push(itemId);
  }
  const [stamina, strength, agility, intellect] = [reader.u16(), reader.u16(), reader.u16(), reader.u16()];
  if ([stamina, strength, agility, intellect].some((value, stat) => value !== totals[stat])) {
    throw new Error("Stat totals do not match the equipment");
  }
  return { equipment, stats: { stamina, strength, agility, intellect } };
}

export function decodeSnapshot(payload: Uint8Array): ZoneSnapshot {
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (view.byteLength > MAX_PROJECTION_BYTES) {
    throw new Error("Snapshot exceeds the projection byte budget");
  }
  if (view.byteLength < FIXED_BYTES) {
    throw new Error("Truncated snapshot");
  }
  if (view.getUint8(0) !== WIRE_VERSION || view.getUint16(2) !== SCHEMA_VERSION) {
    throw new Error("Unsupported snapshot version");
  }
  if (view.getUint8(1) !== PLAYER_SCOPE) {
    throw new Error("Expected player-visible snapshot");
  }
  const reader = new Reader(view);
  reader.u32();
  const zoneId = reader.u32();
  const tick = reader.u64();
  const contentRevision = reader.u64();
  const acknowledgedSequence = reader.u32();
  const viewerId = reader.u32();
  const health = reader.u32();
  const maxHealth = reader.u32();
  const level = reader.u8();
  const experience = reader.u32();
  const experienceToNextLevel = reader.u32();
  const copper = reader.u32();
  const [dead = false, inCombat = false, autoAttacking = false] = reader.flags(3);
  const target = reader.entity();
  if (level < 1 || level > 10
    || (level === 10 && (experience !== 0 || experienceToNextLevel !== 0))
    || (level < 10 && (experienceToNextLevel === 0 || experience >= experienceToNextLevel))
    || health > maxHealth || dead !== (health === 0)) {
    throw new Error("Inconsistent viewer state");
  }
  const classCode = reader.u8();
  if (classCode > 6) {
    throw new Error("Inconsistent viewer ability state");
  }
  const classChoice: ClassChoice | null = classCode === 0
    ? null
    : { classId: CLASSES[Math.floor((classCode - 1) / 2)] ?? "warden", sex: (classCode - 1) % 2 === 0 ? "female" : "male" };
  const resourceCode = reader.u8();
  const resourceValue = reader.u16();
  const resourceMaximum = reader.u16();
  const globalCooldown = reader.u16();
  const cast = decodeCast(reader);
  const damage = { min: reader.u16(), max: reader.u16() };
  if (damage.min > damage.max) {
    throw new Error("Inverted viewer damage range");
  }
  const cooldownCount = reader.u8();
  if (cooldownCount > MAX_COOLDOWNS) {
    throw new Error("Snapshot has too many cooldowns");
  }
  const cooldowns: CooldownState[] = [];
  for (let index = 0; index < cooldownCount; index += 1) {
    const ability = reader.u8();
    const remaining = reader.u16();
    const shape = ABILITY_SHAPES.get(ability);
    const previous = cooldowns.at(-1);
    if (!shape || remaining === 0 || remaining > shape.cooldown || (previous && previous.ability >= ability)) {
      throw new Error("Inconsistent cooldown record");
    }
    cooldowns.push({ ability, remaining });
  }
  const auras = decodeAuras(reader);
  let resource: ResourceState | null = null;
  if (classChoice === null) {
    if (resourceCode !== 0 || resourceValue !== 0 || resourceMaximum !== 0) {
      throw new Error("Inconsistent viewer resource");
    }
  } else {
    const kind = RESOURCES[CLASSES.indexOf(classChoice.classId)];
    if (kind === undefined || RESOURCES[resourceCode - 1] !== kind || resourceMaximum !== resourceMax(kind, level)
      || resourceValue > resourceMaximum) {
      throw new Error("Inconsistent viewer resource");
    }
    resource = { kind, value: resourceValue, max: resourceMaximum };
  }
  if (globalCooldown > GLOBAL_COOLDOWN_TICKS
    || (classChoice === null && (globalCooldown !== 0 || cast !== null || cooldowns.length > 0 || auras.length > 0))
    || (dead && (cast !== null || auras.length > 0))) {
    throw new Error("Inconsistent viewer ability state");
  }
  const targetOfTarget = reader.entity();
  const targetDetail = { cast: decodeCast(reader), auras: decodeAuras(reader) };
  if (target === null && (targetDetail.cast !== null || targetDetail.auras.length > 0)) {
    throw new Error("Target detail needs a target");
  }
  const inventoryRevision = reader.u64();
  if (inventoryRevision === 0n) {
    throw new Error("Inventory revision must be nonzero");
  }
  const [hasInventory = false] = reader.flags(1);
  let inventory: InventorySlot[] | null = null;
  let equipment: (number | null)[] | null = null;
  let stats: StatTotals | null = null;
  let quests: QuestSheet | null = null;
  if (hasInventory) {
    inventory = [];
    for (let slot = 0; slot < 16; slot += 1) {
      const itemId = reader.u16();
      const quantity = reader.u16();
      if (itemId === 0 && quantity === 0) {
        inventory.push(null);
      } else {
        validateStack(itemId, quantity);
        inventory.push({ itemId, quantity });
      }
    }
    ({ equipment, stats } = decodeEquipment(reader));
    quests = decodeQuests(reader);
  }
  const [hasLoot = false] = reader.flags(1);
  let loot: LootView | null = null;
  if (hasLoot) {
    const creatureId = reader.u32();
    const diedAt = reader.u64();
    const money = reader.u32();
    const stack = (): Exclude<InventorySlot, null> | null => {
      const [present = false] = reader.flags(1);
      if (!present) {
        return null;
      }
      const itemId = reader.u16();
      const quantity = reader.u16();
      validateStack(itemId, quantity);
      return { itemId, quantity };
    };
    const item = stack();
    const questItem = stack();
    // A quest drop is always one unit.
    if (dead || target?.kind !== "creature" || target.id !== creatureId || diedAt > tick
      || (money === 0 && item === null && questItem === null) || (questItem !== null && questItem.quantity !== 1)) {
      throw new Error("Inconsistent corpse loot sheet");
    }
    loot = { creatureId, diedAt, money, item, questItem };
  }
  const eventCount = reader.u8();
  if (eventCount > MAX_EVENTS) {
    throw new Error("Snapshot exceeds the event capacity");
  }
  const events: ZoneEvent[] = [];
  for (let index = 0; index < eventCount; index += 1) {
    events.push(decodeEvent(reader));
  }
  const chatCount = reader.u8();
  if (chatCount > MAX_CHAT) {
    throw new Error("Snapshot exceeds the chat capacity");
  }
  const chat: ChatLine[] = [];
  for (let index = 0; index < chatCount; index += 1) {
    const speaker = reader.u32();
    const channel = ["say", "yell", "emote"][reader.u8()] as ChatLine["channel"] | undefined;
    if (channel === undefined) {
      throw new Error("Unknown chat channel");
    }
    if (channel === "emote") {
      const emote = EMOTES[reader.u8() - 1];
      if (emote === undefined) {
        throw new Error("Unknown emote");
      }
      chat.push({ speaker, channel, emote });
    } else {
      chat.push({ speaker, channel, text: decodeChatText(reader.bytes(reader.u8())) });
    }
  }
  const count = reader.u16();
  if (count > MAX_ENTITIES || view.byteLength !== reader.offset + count * ENTITY_BYTES) {
    throw new Error("Invalid snapshot length or entity count");
  }
  const entities: EntityState[] = [];
  const identities = new Set<string>();
  for (let index = 0; index < count; index += 1) {
    const entity = decodeEntity(reader);
    const identity = `${entity.kind}:${entity.entityId}`;
    if (identities.has(identity)) {
      throw new Error("Duplicate entity identity");
    }
    identities.add(identity);
    entities.push(entity);
  }
  // Records arrive in relevance-priority order, and the viewer always leads.
  const first = entities[0];
  if (first?.kind !== "player" || first.entityId !== viewerId) {
    throw new Error("Projection must start with the viewer");
  }
  if (loot !== null) {
    const corpse = entities.find((entity) => entity.kind === "creature" && entity.entityId === loot.creatureId);
    if (!corpse?.flags.dead || !corpse.flags.lootable || corpse.flags.tappedByOther || corpse.healthPercent !== 0) {
      throw new Error("Inconsistent corpse loot sheet");
    }
  }
  return {
    zoneId, tick, contentRevision, acknowledgedSequence, viewerId,
    viewer: {
      copper, experience, experienceToNextLevel, health, maxHealth, level, dead, inCombat, autoAttacking, target,
      classChoice, resource, cast, globalCooldown, damage,
    },
    cooldowns, auras, targetOfTarget, targetDetail, inventoryRevision, inventory, equipment, stats, quests, loot, events,
    chat, entities,
  };
}

/** The projection's record of `entity`, if it is visible. */
export function findEntity(snapshot: ZoneSnapshot, entity: EntityRef | null): EntityState | undefined {
  return snapshot.entities.find((candidate) => sameEntity({ kind: candidate.kind, id: candidate.entityId }, entity));
}

/** Nearest u16 yaw for a presentation angle in radians (0 faces +Z, turning toward +X). */
export function yawFromRadians(radians: number): number {
  const turns = radians / (2 * Math.PI);
  return Math.round((turns - Math.floor(turns)) * YAW_STEPS) % YAW_STEPS;
}

/** Interpolates yaw along the shorter arc; the result stays in [0, 65 536). */
function interpolateFacing(from: number, to: number, alpha: number): number {
  const half = YAW_STEPS / 2;
  const delta = ((to - from + YAW_STEPS + half) % YAW_STEPS) - half;
  const facing = (from + delta * alpha) % YAW_STEPS;
  return facing < 0 ? facing + YAW_STEPS : facing;
}

/** Bounded presentation history. It never extrapolates gameplay or runs physics. */
export class SnapshotBuffer {
  #snapshots: ZoneSnapshot[] = [];

  /** Call on reconnect, handoff, or an authority-epoch change before accepting a new stream. */
  reset(): void {
    this.#snapshots = [];
  }

  push(snapshot: ZoneSnapshot): boolean {
    const latest = this.#snapshots.at(-1);
    if (latest) {
      if (
        snapshot.zoneId !== latest.zoneId ||
        snapshot.contentRevision !== latest.contentRevision ||
        snapshot.viewerId !== latest.viewerId
      ) {
        throw new Error("Snapshot stream changed without reset");
      }
      if (snapshot.tick <= latest.tick) {
        return false;
      }
      // After a long stall, snap to current state instead of interpolating a stale journey.
      if (snapshot.tick - latest.tick > BigInt(TICK_HZ * 30)) {
        this.reset();
      }
    }
    this.#snapshots.push(snapshot);
    if (this.#snapshots.length > BUFFER_CAPACITY) {
      this.#snapshots.shift();
    }
    return true;
  }

  sample(tick: bigint, fraction = 0): readonly EntityState[] {
    if (!Number.isFinite(fraction) || fraction < 0 || fraction >= 1) {
      throw new Error("Render tick fraction must be in [0, 1)");
    }
    let before = this.#snapshots[0];
    if (!before) {
      return [];
    }
    if (tick < before.tick) {
      return before.entities;
    }
    for (const after of this.#snapshots.slice(1)) {
      if (tick < after.tick) {
        const alpha = (Number(tick - before.tick) + fraction) / Number(after.tick - before.tick);
        const identity = (entity: EntityState) => `${entity.kind}:${entity.entityId}`;
        const nextEntities = new Map(after.entities.map((entity) => [identity(entity), entity]));
        return before.entities.map((entity) => {
          const next = nextEntities.get(identity(entity));
          // Appearance/disappearance happens at the authoritative sample tick.
          if (!next) {
            return entity;
          }
          const interpolate = (axis: 0 | 1 | 2) =>
            entity.position[axis] + (next.position[axis] - entity.position[axis]) * alpha;
          return {
            ...entity,
            position: [interpolate(0), interpolate(1), interpolate(2)],
            facing: interpolateFacing(entity.facing, next.facing, alpha),
          };
        });
      }
      before = after;
    }
    return before.entities;
  }
}
