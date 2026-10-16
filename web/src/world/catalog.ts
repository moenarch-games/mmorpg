/**
 * The content catalog from the WASM `catalog()` export (format
 * `mmorpg.catalog` v6): names and presentation facts for the IDs that
 * projections carry. Creature templates by template ID, NPCs by NPC ID,
 * areas by area ID, items (with equipment slot and stats) by item ID,
 * classes by wire value, abilities by ability ID and quests by quest ID.
 * Combat numbers stay on the server.
 */
export type CreatureFamily = "wolf" | "boar" | "vermin" | "marauder" | "mirefin" | "redbrand";
export type CreatureBehaviour = "aggressive" | "neutral";
export type NpcRole = "quest_giver" | "vendor" | "spirit_healer" | "guard";

export type CreatureTemplate = {
  id: number;
  name: string;
  family: CreatureFamily;
  behaviour: CreatureBehaviour;
  minLevel: number;
  maxLevel: number;
  elite: boolean;
  /** Collision box in units. */
  halfExtents: readonly [number, number, number];
};

export type NpcRecord = { id: number; name: string; role: NpcRole; level: number };

export type EquipmentSlotName = "mainHand" | "offHand" | "head" | "chest" | "legs" | "feet";
/** Equipment slot names by wire index (0 main hand … 5 feet). */
export const EQUIPMENT_SLOT_NAMES: readonly EquipmentSlotName[] = ["mainHand", "offHand", "head", "chest", "legs", "feet"];
export type ItemStats = { stamina: number; strength: number; agility: number; intellect: number };
export type ItemRecord = {
  id: number;
  name: string;
  maxStack: number;
  /** The equipment slot the item fits, or null for plain bag items. */
  slot: EquipmentSlotName | null;
  /** Attributes added while equipped. */
  stats: ItemStats;
  /** Copper a vendor pays for one unit. */
  sellPrice: number;
};

/** One unit of `item` costs `price` copper; the offer's array index is its wire index. */
export type VendorOffer = { item: number; price: number };
export type VendorRecord = { npc: number; offers: readonly VendorOffer[] };

export type ClassName = "warden" | "ranger" | "arcanist";
export type ClassRecord = { id: number; name: ClassName; resource: "rage" | "focus" | "mana" };

export type AbilityRecord = {
  id: number;
  name: string;
  /** A class name, or `creature`. */
  user: ClassName | "creature";
  /** Unlock level for classes. */
  level: number;
  cost: number;
  /** 0 for instants. */
  castTicks: number;
  channel: boolean;
  cooldown: number;
  /** Aura kind wire code (1 damage over time … 7 haste), or null. */
  aura: number | null;
};

/** `target` is a creature template (kill), an item (collect, dropped by template `source`), an NPC (talk) or an area (explore). */
export type QuestObjectiveRecord =
  | { kind: "kill" | "talk" | "explore"; target: number; count: number }
  | { kind: "collect"; target: number; source: number; count: number };

export type QuestRecord = {
  id: number;
  name: string;
  /** What the giver says. */
  text: string;
  giver: number;
  ender: number;
  prerequisite: number | null;
  /** Objective order is the wire progress index. */
  objectives: readonly QuestObjectiveRecord[];
  experience: number;
  copper: number;
  /** Item IDs; the reward choice is the index into this list. */
  choices: readonly number[];
};

export type ContentCatalog = {
  itemCatalogRevision: bigint;
  items: ReadonlyMap<number, ItemRecord>;
  contentRevision: bigint;
  /** 16 lower-case hex digits. */
  contentFingerprint: string;
  creatureTemplates: ReadonlyMap<number, CreatureTemplate>;
  npcs: ReadonlyMap<number, NpcRecord>;
  areas: ReadonlyMap<number, string>;
  classes: ReadonlyMap<number, ClassRecord>;
  abilities: ReadonlyMap<number, AbilityRecord>;
  /** Vendor stock by NPC ID. */
  vendors: ReadonlyMap<number, VendorRecord>;
  /** Quests by quest ID, in ID order. */
  quests: ReadonlyMap<number, QuestRecord>;
};

const FORMAT = "mmorpg.catalog";
const VERSION = 6;
const OBJECTIVE_KINDS = ["kill", "collect", "talk", "explore"] as const;
const CLASS_NAMES: readonly ClassName[] = ["warden", "ranger", "arcanist"];
const RESOURCES = ["rage", "focus", "mana"] as const;
const USERS: readonly (ClassName | "creature")[] = [...CLASS_NAMES, "creature"];
const FAMILIES: readonly CreatureFamily[] = ["wolf", "boar", "vermin", "marauder", "mirefin", "redbrand"];
const BEHAVIOURS: readonly CreatureBehaviour[] = ["aggressive", "neutral"];
const ROLES: readonly NpcRole[] = ["quest_giver", "vendor", "spirit_healer", "guard"];

type Json = Record<string, unknown>;

function fail(message: string): never {
  throw new Error(`Invalid catalog: ${message}.`);
}

function object(value: unknown, keys: readonly string[], name: string): Json {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${name} must be an object`);
  }
  const record = value as Json;
  if (Object.keys(record).length !== keys.length || keys.some((key) => !Object.hasOwn(record, key))) {
    fail(`${name} must have exactly ${keys.join(", ")}`);
  }
  return record;
}

function int(value: unknown, name: string, min: number, max: number): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < min || value > max) {
    fail(`${name} must be an integer in [${min}, ${max}]`);
  }
  return value;
}

function list(value: unknown, name: string, maximum: number): readonly unknown[] {
  if (!Array.isArray(value) || value.length > maximum) {
    fail(`${name} must be an array of at most ${maximum} entries`);
  }
  return value;
}

function text(value: unknown, name: string): string {
  if (typeof value !== "string" || value.trim().length === 0 || value.length > 64) {
    fail(`${name} must be non-empty text`);
  }
  return value;
}

function oneOf<T extends string>(value: unknown, options: readonly T[], name: string): T {
  const match = options.find((option) => option === value);
  if (match === undefined) {
    fail(`${name} must be one of ${options.join(", ")}`);
  }
  return match;
}

/** Indexes records by ID; duplicate IDs fail closed. */
function byId<T extends { id: number }>(records: readonly T[], name: string): ReadonlyMap<number, T> {
  const map = new Map(records.map((record) => [record.id, record]));
  if (map.size !== records.length) {
    fail(`${name} IDs must be unique`);
  }
  return map;
}

/** Strictly decodes the export at the WASM boundary; unknown shapes fail closed. */
export function decodeCatalog(json: string): ContentCatalog {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    fail("not JSON");
  }
  const root = object(parsed, [
    "format", "version", "contentRevision", "contentFingerprint", "creatureTemplates", "npcs", "areas", "itemCatalogRevision", "items",
    "classes", "abilityCatalogRevision", "abilities", "vendorCatalogRevision", "vendors", "questCatalogRevision", "quests",
  ], "export");
  if (root.format !== FORMAT || root.version !== VERSION) {
    fail(`unsupported format ${String(root.format)} v${String(root.version)}`);
  }
  if (typeof root.contentRevision !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(root.contentRevision)) {
    fail("content revision must be a canonical decimal u64");
  }
  const contentRevision = BigInt(root.contentRevision);
  if (contentRevision > 0xffff_ffff_ffff_ffffn) {
    fail("content revision exceeds u64");
  }
  if (typeof root.contentFingerprint !== "string" || !/^[0-9a-f]{16}$/.test(root.contentFingerprint)) {
    fail("content fingerprint must be 16 lower-case hex digits");
  }
  const templates = list(root.creatureTemplates, "creature templates", 256).map((value, index) => {
    const name = `creature template ${index}`;
    const record = object(value, [
      "id", "name", "family", "behaviour", "minLevel", "maxLevel", "elite", "halfExtents",
    ], name);
    const minLevel = int(record.minLevel, `${name} minimum level`, 1, 255);
    const maxLevel = int(record.maxLevel, `${name} maximum level`, minLevel, 255);
    const extents = list(record.halfExtents, `${name} half extents`, 3);
    if (extents.length !== 3 || typeof record.elite !== "boolean") {
      fail(`${name} needs three half extents and an elite flag`);
    }
    const [x = 0, y = 0, z = 0] = extents.map((extent, axis) => int(extent, `${name} half extent ${axis}`, 1, 10_000));
    return {
      id: int(record.id, `${name} id`, 0, 0xffff),
      name: text(record.name, `${name} name`),
      family: oneOf(record.family, FAMILIES, `${name} family`),
      behaviour: oneOf(record.behaviour, BEHAVIOURS, `${name} behaviour`),
      minLevel,
      maxLevel,
      elite: record.elite,
      halfExtents: [x, y, z] as const,
    };
  });
  const npcs = list(root.npcs, "npcs", 256).map((value, index) => {
    const name = `npc ${index}`;
    const record = object(value, ["id", "name", "role", "level"], name);
    return {
      id: int(record.id, `${name} id`, 0, 0xffff),
      name: text(record.name, `${name} name`),
      role: oneOf(record.role, ROLES, `${name} role`),
      level: int(record.level, `${name} level`, 1, 255),
    };
  });
  const areas = list(root.areas, "areas", 64).map((value, index) => {
    const record = object(value, ["id", "name"], `area ${index}`);
    return { id: int(record.id, `area ${index} id`, 0, 0xffff), name: text(record.name, `area ${index} name`) };
  });
  if (root.itemCatalogRevision !== "3") {
    fail("unsupported item catalog revision");
  }
  const items = list(root.items, "items", 256).map((value, index): ItemRecord => {
    const name = `item ${index}`;
    const record = object(value, ["id", "name", "maxStack", "slot", "stats", "sellPrice"], name);
    const stats = object(record.stats, ["stamina", "strength", "agility", "intellect"], `${name} stats`);
    return {
      id: int(record.id, `${name} id`, 1, 0xffff),
      name: text(record.name, `${name} name`),
      maxStack: int(record.maxStack, `${name} stack limit`, 1, 0xffff),
      slot: record.slot === null ? null : oneOf(record.slot, EQUIPMENT_SLOT_NAMES, `${name} slot`),
      stats: {
        stamina: int(stats.stamina, `${name} stamina`, 0, 255),
        strength: int(stats.strength, `${name} strength`, 0, 255),
        agility: int(stats.agility, `${name} agility`, 0, 255),
        intellect: int(stats.intellect, `${name} intellect`, 0, 255),
      },
      sellPrice: int(record.sellPrice, `${name} sale value`, 0, 0xffff_ffff),
    };
  });
  const itemsById = byId(items, "item");
  if (root.vendorCatalogRevision !== "0" && root.vendorCatalogRevision !== "2") {
    fail("unsupported vendor catalog revision");
  }
  const vendors = list(root.vendors, "vendors", 256).map((value, index): VendorRecord & { id: number } => {
    const name = `vendor ${index}`;
    const record = object(value, ["npc", "offers"], name);
    const npc = int(record.npc, `${name} npc`, 0, 0xffff_ffff);
    if (npcs.find((candidate) => candidate.id === npc)?.role !== "vendor") {
      fail(`${name} must name a vendor npc`);
    }
    const offers = list(record.offers, `${name} offers`, 8).map((offer, slot) => {
      const entry = object(offer, ["item", "price"], `${name} offer ${slot}`);
      const item = int(entry.item, `${name} offer ${slot} item`, 1, 0xffff);
      if (!itemsById.has(item)) {
        fail(`${name} offer ${slot} names an unknown item`);
      }
      return { item, price: int(entry.price, `${name} offer ${slot} price`, 1, 0xffff_ffff) };
    });
    if (offers.length === 0) {
      fail(`${name} must offer at least one item`);
    }
    return { id: npc, npc, offers };
  });
  if (root.vendorCatalogRevision === "0" && vendors.length > 0) {
    fail("content without a vendor catalog cannot list vendors");
  }
  const classes = list(root.classes, "classes", 3).map((value, index) => {
    const record = object(value, ["id", "name", "resource"], `class ${index}`);
    return {
      id: int(record.id, `class ${index} id`, 0, 2),
      name: oneOf(record.name, CLASS_NAMES, `class ${index} name`),
      resource: oneOf(record.resource, RESOURCES, `class ${index} resource`),
    };
  });
  if (root.abilityCatalogRevision !== "1") {
    fail("unsupported ability catalog revision");
  }
  const abilities = list(root.abilities, "abilities", 255).map((value, index) => {
    const name = `ability ${index}`;
    const record = object(value, [
      "id", "name", "user", "level", "cost", "castTicks", "channel", "cooldown", "aura",
    ], name);
    if (typeof record.channel !== "boolean") {
      fail(`${name} channel must be a flag`);
    }
    return {
      id: int(record.id, `${name} id`, 1, 255),
      name: text(record.name, `${name} name`),
      user: oneOf(record.user, USERS, `${name} user`),
      level: int(record.level, `${name} level`, 1, 255),
      cost: int(record.cost, `${name} cost`, 0, 0xffff),
      castTicks: int(record.castTicks, `${name} cast ticks`, 0, 0xffff),
      channel: record.channel,
      cooldown: int(record.cooldown, `${name} cooldown`, 0, 0xffff),
      aura: record.aura === null ? null : int(record.aura, `${name} aura`, 1, 7),
    };
  });
  if (root.questCatalogRevision !== "0" && root.questCatalogRevision !== "1") {
    fail("unsupported quest catalog revision");
  }
  const npcsById = byId(npcs, "npc");
  const quests = list(root.quests, "quests", 32).map((value, index): QuestRecord => {
    const name = `quest ${index}`;
    const record = object(value, [
      "id", "name", "text", "giver", "ender", "prerequisite", "objectives", "experience", "copper", "choices",
    ], name);
    const npc = (field: unknown, label: string) => {
      const id = int(field, `${name} ${label}`, 0, 0xffff);
      if (npcsById.get(id)?.role !== "quest_giver") {
        fail(`${name} ${label} must be a quest giver`);
      }
      return id;
    };
    if (typeof record.text !== "string" || record.text.trim().length === 0 || record.text.length > 240) {
      fail(`${name} text must be non-empty text`);
    }
    const objectives = list(record.objectives, `${name} objectives`, 3).map((entry, slot): QuestObjectiveRecord => {
      const label = `${name} objective ${slot}`;
      const objective = object(entry, ["kind", "target", "source", "count"], label);
      const kind = oneOf(objective.kind, OBJECTIVE_KINDS, `${label} kind`);
      const target = int(objective.target, `${label} target`, 0, 0xffff_ffff);
      const count = int(objective.count, `${label} count`, 1, 255);
      if (kind === "collect") {
        if (!itemsById.has(target)) {
          fail(`${label} names an unknown item`);
        }
        return { kind, target, source: int(objective.source, `${label} source`, 0, 0xffff), count };
      }
      if (objective.source !== null) {
        fail(`${label} has a source only when it collects`);
      }
      return { kind, target, count };
    });
    if (objectives.length === 0) {
      fail(`${name} needs an objective`);
    }
    const choices = list(record.choices, `${name} choices`, 4).map((item, slot) => {
      const id = int(item, `${name} choice ${slot}`, 1, 0xffff);
      if (!itemsById.has(id)) {
        fail(`${name} choice ${slot} names an unknown item`);
      }
      return id;
    });
    return {
      id: int(record.id, `${name} id`, 1, 32),
      name: text(record.name, `${name} name`),
      text: record.text,
      giver: npc(record.giver, "giver"),
      ender: npc(record.ender, "ender"),
      prerequisite: record.prerequisite === null ? null : int(record.prerequisite, `${name} prerequisite`, 1, 32),
      objectives,
      experience: int(record.experience, `${name} experience`, 0, 0xffff_ffff),
      copper: int(record.copper, `${name} copper`, 0, 0xffff_ffff),
      choices,
    };
  });
  if (root.questCatalogRevision === "0" && quests.length > 0) {
    fail("content without a quest catalog cannot list quests");
  }
  return {
    quests: byId(quests, "quest"),
    classes: byId(classes, "class"),
    abilities: byId(abilities, "ability"),
    itemCatalogRevision: 3n,
    items: itemsById,
    vendors: new Map([...byId(vendors, "vendor")].map(([id, { npc, offers }]) => [id, { npc, offers }])),
    contentRevision,
    contentFingerprint: root.contentFingerprint,
    creatureTemplates: byId(templates, "creature template"),
    npcs: npcsById,
    areas: new Map([...byId(areas, "area")].map(([id, area]) => [id, area.name])),
  };
}
