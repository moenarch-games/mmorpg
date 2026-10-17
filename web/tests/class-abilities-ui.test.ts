import { describe, expect, test } from "bun:test";
import type { AuraState, CastState, ViewerState, ZoneEvent } from "../src/replication";
import type { AbilityRecord, type ContentCatalog } from "../src/world/catalog";
import { auraPolarity, undescribedAbilities } from "../src/world/units/ability-presentation";
import { cooldownSeconds, formatRemaining, slotState, sweepDegrees, targetDistanceUnits, tooltipText } from "../src/world/units/ability-state";
import { auraLabel, auraTimer, sortAuras } from "../src/world/units/aura-display";
import { errorText } from "../src/world/units/combat-hud";
import { castBarView } from "../src/world/units/cast-bar";
import { combatTexts, damageIsPeriodic, viewerInterrupted } from "../src/world/units/combat-text";
import { HEALTHY_VIEWER, NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots";

const ABILITIES: AbilityRecord[] = [
  { id: 1, name: "Heroic Strike", user: "warden", level: 1, cost: 15, castTicks: 0, channel: false, cooldown: 0, aura: null },
  { id: 2, name: "Shield Bash", user: "warden", level: 2, cost: 10, castTicks: 0, channel: false, cooldown: 360, aura: 6 },
  { id: 9, name: "Firebolt", user: "arcanist", level: 1, cost: 25, castTicks: 60, channel: false, cooldown: 0, aura: null },
  { id: 10, name: "Frost Nova", user: "arcanist", level: 2, cost: 30, castTicks: 0, channel: false, cooldown: 600, aura: 4 },
  { id: 11, name: "Arcane Barrier", user: "arcanist", level: 4, cost: 40, castTicks: 0, channel: false, cooldown: 900, aura: 3 },
  { id: 12, name: "Blizzard", user: "arcanist", level: 6, cost: 80, castTicks: 180, channel: true, cooldown: 0, aura: null },
];
const catalog: ContentCatalog = {
  itemCatalogRevision: 1n, items: new Map(), contentRevision: 6n, contentFingerprint: "0".repeat(16),
  creatureTemplates: new Map(), npcs: new Map(), areas: new Map(),
  classes: new Map([[0, { id: 0, name: "warden", resource: "rage" }], [2, { id: 2, name: "arcanist", resource: "mana" }]]),
  abilities: new Map(ABILITIES.map((ability) => [ability.id, ability])),
};
const [strike, bash, firebolt, nova, barrier, blizzard] = ABILITIES as [AbilityRecord, AbilityRecord, AbilityRecord, AbilityRecord, AbilityRecord, AbilityRecord];

const wolf = { kind: "creature", id: 108 } as const;
const me = { kind: "player", id: 1 } as const;

function viewer(fields: Partial<ViewerState> = {}): ViewerState {
  return { ...HEALTHY_VIEWER, level: 4, classChoice: { classId: "arcanist", sex: "male" }, resource: { kind: "mana", value: 100, max: 176 }, target: wolf, ...fields };
}

function snapshot(fields: Partial<Parameters<typeof testSnapshot>[0]> = {}, self: Partial<ViewerState> = {}, wolfAt = 1_000) {
  return testSnapshot({
    zoneId: 1, tick: 5n, contentRevision: 6n, acknowledgedSequence: 1, viewerId: 1, viewer: viewer(self),
    entities: [playerEntity(1, [0, 90, 0]), { ...playerEntity(108, [wolfAt, 90, 0]), kind: "creature", flags: { ...NO_FLAGS, attackable: true } }],
    ...fields,
  });
}

describe("slot state", () => {
  test("a ready, learned slot in range has no flags", () => {
    const state = slotState(firebolt, snapshot(), 1_000);
    expect(state).toMatchObject({ learned: true, resourceShort: false, outOfRange: false, casting: false, cooldown: null });
  });

  test("not enough resource tints the slot; a free ability never does", () => {
    expect(slotState(firebolt, snapshot({}, { resource: { kind: "mana", value: 24, max: 176 } }), 1_000).resourceShort).toBe(true);
    expect(slotState(firebolt, snapshot({}, { resource: { kind: "mana", value: 25, max: 176 } }), 1_000).resourceShort).toBe(false);
  });

  test("range follows the ability: ranged 30 m, melee 2.5 m, self-centred never", () => {
    expect(slotState(firebolt, snapshot(), 3_000).outOfRange).toBe(false);
    expect(slotState(firebolt, snapshot(), 3_001).outOfRange).toBe(true);
    expect(slotState(strike, snapshot(), 250).outOfRange).toBe(false);
    expect(slotState(strike, snapshot(), 251).outOfRange).toBe(true);
    expect(slotState(nova, snapshot(), 9_000).outOfRange).toBe(false);
    expect(slotState(firebolt, snapshot(), null).outOfRange).toBe(false);
  });

  test("an unlearned ability shows its unlock level", () => {
    const state = slotState(blizzard, snapshot(), 100);
    expect(state).toMatchObject({ learned: false, unlockLevel: 6 });
    expect(slotState(barrier, snapshot(), 100).learned).toBe(true);
  });

  test("the casting slot is flagged", () => {
    const cast: CastState = { ability: 9, elapsed: 10, total: 60, channel: false };
    expect(slotState(firebolt, snapshot({}, { cast }), 1_000).casting).toBe(true);
    expect(slotState(nova, snapshot({}, { cast }), 1_000).casting).toBe(false);
  });

  test("the longer of cooldown and global cooldown is shown", () => {
    const cooling = snapshot({ cooldowns: [{ ability: 10, remaining: 300 }] }, { globalCooldown: 40 });
    expect(slotState(nova, cooling, 0).cooldown).toEqual({ kind: "cooldown", remaining: 300, total: 600 });
    expect(slotState(firebolt, cooling, 0).cooldown).toEqual({ kind: "gcd", remaining: 40, total: 45 });
    const shortCooldown = snapshot({ cooldowns: [{ ability: 10, remaining: 20 }] }, { globalCooldown: 40 });
    expect(slotState(nova, shortCooldown, 0).cooldown).toEqual({ kind: "gcd", remaining: 40, total: 45 });
  });

  test("target distance is the XZ centre distance, null without a visible target", () => {
    expect(targetDistanceUnits(snapshot())).toBe(1_000);
    expect(targetDistanceUnits(snapshot({}, { target: null }))).toBeNull();
    // A selected target outside the interest radius is omitted from the projection: out of reach.
    expect(targetDistanceUnits(snapshot({}, { target: { kind: "creature", id: 999 } }))).toBe(Number.POSITIVE_INFINITY);
    expect(slotState(firebolt, snapshot({}, { target: { kind: "creature", id: 999 } }), Number.POSITIVE_INFINITY).outOfRange).toBe(true);
    expect(targetDistanceUnits(snapshot({}, { target: me }))).toBeNull();
  });
});

describe("sweep and labels", () => {
  test("the sweep covers a full turn at the start and shrinks to nothing", () => {
    expect(sweepDegrees(600, 600)).toBe(360);
    expect(sweepDegrees(300, 600)).toBe(180);
    expect(sweepDegrees(150, 600)).toBe(90);
    expect(sweepDegrees(0, 600)).toBe(0);
    expect(sweepDegrees(10, 0)).toBe(0);
    expect(sweepDegrees(900, 600)).toBe(360);
  });

  test("seconds show only while at least 2 s remain", () => {
    expect(cooldownSeconds(59)).toBeNull();
    expect(cooldownSeconds(60)).toBe(2);
    expect(cooldownSeconds(61)).toBe(3);
    expect(cooldownSeconds(360)).toBe(12);
  });

  test("remaining time reads as seconds, then minutes", () => {
    expect(formatRemaining(1)).toBe("1s");
    expect(formatRemaining(31)).toBe("2s");
    expect(formatRemaining(300)).toBe("10s");
    expect(formatRemaining(1_800)).toBe("1m");
    expect(formatRemaining(2_000)).toBe("2m");
  });
});

describe("tooltips", () => {
  test("a cast with a cost and range", () => {
    const tip = tooltipText(firebolt, catalog, 4);
    expect(tip.title).toBe("Firebolt");
    expect(tip.lines).toEqual(["25 Mana", "30 m range", "2 sec cast"]);
    expect(tip.description.length).toBeGreaterThan(10);
    expect(tip.note).toBeNull();
  });

  test("an instant melee ability with a cooldown", () => {
    expect(tooltipText(bash, catalog, 2).lines).toEqual(["10 Rage", "Melee range", "Instant", "12 sec cooldown"]);
    expect(tooltipText(strike, catalog, 1).lines).toEqual(["15 Rage", "Melee range", "Instant"]);
  });

  test("a channel and a self-centred ability", () => {
    expect(tooltipText(blizzard, catalog, 6).lines).toEqual(["80 Mana", "30 m range", "6 sec channel"]);
    expect(tooltipText(nova, catalog, 2).lines).toEqual(["30 Mana", "Self", "Instant", "20 sec cooldown"]);
  });

  test("an unlearned ability names its unlock level", () => {
    expect(tooltipText(blizzard, catalog, 4).note).toBe("Unlocks at level 6");
  });

  test("every catalog ability has presentation facts", () => {
    expect(undescribedAbilities(catalog)).toEqual([]);
  });
});

describe("auras", () => {
  const aura = (ability: number, kind: AuraState["kind"], remaining: number, amount = 0): AuraState => ({ ability, kind, remaining, amount });

  test("buffs and debuffs split by kind, longest first, ties by ability", () => {
    const sorted = sortAuras([
      aura(10, "root", 100), aura(11, "absorb", 200, 40), aura(3, "heal-over-time", 200, 30),
      aura(6, "damage-over-time", 400, 30), aura(8, "haste", 50, 40),
    ]);
    expect(sorted.buffs.map((entry) => entry.ability)).toEqual([3, 11, 8]);
    expect(sorted.debuffs.map((entry) => entry.ability)).toEqual([6, 10]);
  });

  test("polarity by kind", () => {
    expect(["heal-over-time", "absorb", "haste"].map((kind) => auraPolarity(kind as AuraState["kind"]))).toEqual(["buff", "buff", "buff"]);
    expect(["damage-over-time", "root", "snare", "stun"].map((kind) => auraPolarity(kind as AuraState["kind"]))).toEqual(["debuff", "debuff", "debuff", "debuff"]);
  });

  test("labels name the ability, its effect and the time left", () => {
    expect(auraLabel(aura(11, "absorb", 600, 60), catalog)).toBe("Arcane Barrier: Absorbs 60 damage (20s)");
    expect(auraLabel(aura(10, "root", 31), catalog)).toBe("Frost Nova: Rooted (2s)");
    expect(auraLabel(aura(2, "snare", 30, 50), catalog)).toBe("Shield Bash: Slowed by 50% (1s)");
    expect(auraTimer(aura(11, "absorb", 1_800))).toBe("1m");
  });
});

describe("cast bars", () => {
  test("a cast fills as it runs", () => {
    expect(castBarView({ ability: 9, elapsed: 15, total: 60, channel: false }, catalog)).toEqual({ name: "Firebolt", fill: 0.25, remaining: "1.5", channel: false });
  });

  test("a channel drains backwards", () => {
    expect(castBarView({ ability: 12, elapsed: 45, total: 180, channel: true }, catalog)).toEqual({ name: "Blizzard", fill: 0.75, remaining: "4.5", channel: true });
    expect(castBarView({ ability: 12, elapsed: 0, total: 180, channel: true }, catalog).fill).toBe(1);
  });
});

describe("combat text", () => {
  const dealt = (amount: number, critical = false): ZoneEvent => ({ kind: "damage-dealt", source: me, target: wolf, amount, critical });

  test("heals are green, absorbs say Absorb and interrupts are named", () => {
    const events: ZoneEvent[] = [
      { kind: "healed", source: me, target: me, ability: 3, amount: 9 },
      { kind: "absorbed", source: wolf, target: me, amount: 4 },
      { kind: "interrupted", source: wolf, target: me, ability: 9 },
      { kind: "interrupted", source: me, target: wolf, ability: 13 },
    ];
    expect(combatTexts(snapshot({ events }))).toEqual([
      { kind: "heal", text: "+9" }, { kind: "absorb", text: "Absorb" },
      { kind: "interrupt", text: "Interrupted" }, { kind: "interrupt", text: "Interrupt" },
    ]);
    expect(viewerInterrupted(snapshot({ events }))).toBe(true);
    // A target bandaging itself shows its heal; a creature that is not the target does not.
    const bandage: ZoneEvent = { kind: "healed", source: wolf, target: wolf, ability: 14, amount: 6 };
    expect(combatTexts(snapshot({ events: [bandage] }))).toEqual([{ kind: "heal", text: "+6" }]);
    expect(combatTexts(snapshot({ events: [bandage] }, { target: null }))).toEqual([]);
    expect(viewerInterrupted(snapshot({ events: events.slice(0, 2) }))).toBe(false);
  });

  test("damage over time and channel pulses are periodic; swings and spells are not", () => {
    const dot: AuraState = { ability: 6, kind: "damage-over-time", remaining: 300, amount: 30 };
    const used: ZoneEvent = { kind: "ability-used", source: me, target: wolf, ability: 9 };
    expect(combatTexts(snapshot({ events: [dealt(7)], targetDetail: { cast: null, auras: [dot] } }))).toEqual([{ kind: "periodic", text: "7" }]);
    expect(combatTexts(snapshot({ events: [used, dealt(12)], targetDetail: { cast: null, auras: [dot] } }))).toEqual([{ kind: "damage", text: "12" }]);
    expect(combatTexts(snapshot({ events: [dealt(7)], targetDetail: { cast: null, auras: [dot] } }, { autoAttacking: true }))).toEqual([{ kind: "damage", text: "7" }]);
    const channel: CastState = { ability: 12, elapsed: 30, total: 180, channel: true };
    expect(damageIsPeriodic(snapshot({ events: [dealt(5)] }, { cast: channel }))).toBe(true);
    // Events carry no provenance: a swing during a channel or a sting tick beside auto-shots is ambiguous.
    expect(damageIsPeriodic(snapshot({ events: [dealt(5)] }, { cast: channel, autoAttacking: true }))).toBe(false);
    expect(combatTexts(snapshot({ events: [dealt(20, true)] }))).toEqual([{ kind: "critical", text: "20!" }]);
  });

  test("damage taken and misses", () => {
    const events: ZoneEvent[] = [
      { kind: "damage-taken", source: wolf, target: me, amount: 6, critical: false },
      { kind: "miss", source: me, target: wolf },
      { kind: "miss", source: wolf, target: me },
    ];
    expect(combatTexts(snapshot({ events }))).toEqual([{ kind: "taken", text: "-6" }, { kind: "miss", text: "Miss" }]);
  });
});

describe("error messages", () => {
  const codes = [
    "no-target", "out-of-range", "target-dead", "not-attackable", "you-are-dead", "not-dead", "invalid-target", "too-many-intents",
    "invalid-inventory-move", "inventory-full", "invalid-loot", "not-loot-owner", "empty-loot", "money-overflow",
    "no-class", "not-learned", "not-ready", "not-enough-resource", "stunned", "already-casting", "invalid-class",
  ] as const;

  test("every code reads as a sentence", () => {
    for (const code of codes) {
      expect(errorText(code, snapshot())).toMatch(/^[A-Z].*[.]$/);
    }
  });

  test("a resource shortage names the class resource", () => {
    expect(errorText("not-enough-resource", snapshot())).toBe("Not enough mana.");
    expect(errorText("not-enough-resource", snapshot({}, { resource: { kind: "rage", value: 0, max: 100 } }))).toBe("Not enough rage.");
    expect(errorText("not-enough-resource", snapshot({}, { resource: null }))).toBe("Not enough resource.");
  });
});
