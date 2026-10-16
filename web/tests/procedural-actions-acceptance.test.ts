/** Independent retained #27 contract; authored against 6548f85 before implementation.
 * Events below are observation facts, never client combat decisions. Model assertions
 * deliberately inspect changing rendered transforms, not just an action tracker.
 */
import { describe, expect, test } from "bun:test";
import type { EntityRef, EntityState, ZoneEvent } from "../src/replication";
import { decodeCatalog } from "../src/world/catalog";
import { characterLook } from "../src/world/humanoid";
import { UNIT_MODELS, UnitAnimators, unitIdentity, type UnitContext } from "../src/world/unit-nodes";
import { SpellEffects, allEffectNodes, type Anchor } from "../src/world/units/spell-effects";
import { CombatHud } from "../src/world/units/combat-hud";
import { combatTexts } from "../src/world/units/combat-text";
import { catalogJson } from "./support/catalog";
import { NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots";

const me: EntityRef = { kind: "player", id: 1 };
const other: EntityRef = { kind: "player", id: 2 };
const wolf: EntityRef = { kind: "creature", id: 101 };
const caster: EntityRef = { kind: "creature", id: 102 };
const templates = [
  { id: 1, name: "Wolf", family: "wolf", behaviour: "aggressive", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [40, 45, 40] },
  { id: 2, name: "Mirefin", family: "mirefin", behaviour: "aggressive", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [35, 80, 35] },
  { id: 3, name: "Bandit", family: "redbrand", behaviour: "aggressive", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [30, 90, 30] },
];
const catalog = decodeCatalog(JSON.stringify({ ...catalogJson(), creatureTemplates: templates }));
const creature = (id: number, appearance: number): EntityState => ({ ...playerEntity(id, [600, 80, 0]), kind: "creature", appearance, flags: { ...NO_FLAGS, attackable: true } });
const entities = [playerEntity(1, [0, 90, 0]), playerEntity(2, [300, 90, 0]), creature(101, 1), creature(102, 2), creature(103, 3)];
const rest = { forward: 0, right: 0, vertical: 0, moveWeight: 0, airWeight: 0, turnRate: 0, stridePhase: 0, shufflePhase: 0, time: 0 };
const anchor = (entity: EntityRef): Anchor | null => entities.some((e) => e.kind === entity.kind && e.entityId === entity.id)
  ? { x: entity.id === 1 ? 0 : 6, feetY: 0, centreY: 1, z: 0, yaw: 0 } : null;
function snapshot(events: ZoneEvent[] = [], tick = 10n, units = entities) {
  return testSnapshot({ zoneId: 1, tick, contentRevision: 6n, acknowledgedSequence: 0, viewerId: 1, entities: units, events });
}
function receive(effects: SpellEffects, events: ZoneEvent[], now = 1, tick = 10n, units = entities) {
  const view = snapshot(events, tick, units);
  effects.spawn(view, now, anchor);
  effects.sync(view, now, anchor);
  return view;
}
function rendered(entity: EntityState, effects: SpellEffects, now: number, deathProgress?: number, classId: "warden" | "ranger" | "arcanist" = "warden") {
  const context: UnitContext = {
    unitsPerMetre: 100, playerHalfHeightUnits: 90, viewerId: 1,
    viewerLook: characterLook({ classId, sex: "male" }, null), catalog, viewerTarget: null,
    actionFor: (ref) => effects.actionFor(ref, now),
    viewerAction: effects.actionFor(me, now),
  };
  return UNIT_MODELS[entity.kind].nodes({ id: unitIdentity(entity), entity,
    placement: { x: 0, feetY: 0, z: 0, yawRadians: 0 }, locomotion: rest, context,
    ...(deathProgress === undefined ? {} : { deathProgress }),
  });
}
const transforms = (nodes: ReturnType<typeof rendered>) => nodes.map((node) => ({ id: node.id, transform: node.transform }));

// Public observation seams required by this contract: AttackPerformed / DamageObserved
// map to attack-performed / damage-observed. No wire tag, byte offset or storage strategy is prescribed.
describe("truthful per-actor procedural actions", () => {
  for (const source of [me, other, wolf, caster]) {
    test(`${source.kind} ${source.id}: actual ordinary attempt plays through its own rendered model`, () => {
      const effects = new SpellEffects();
      const unit = entities.find((e) => e.kind === source.kind && e.entityId === source.id)!;
      const idle = transforms(rendered(unit, effects, 0));
      receive(effects, [{ kind: "attack-performed", source, target: source.kind === "creature" ? me : wolf }]);
      const begin = transforms(rendered(unit, effects, 1));
      const active = transforms(rendered(unit, effects, 1.12));
      const settled = transforms(rendered(unit, effects, 2));
      expect(active).not.toEqual(begin);
      expect(active).not.toEqual(idle);
      expect(settled).toEqual(idle);
      const uninvolved = source.id === 2 ? me : other;
      expect(effects.actionFor(uninvolved, 1.12)).toBeNull();
    });
  }
  for (const [source, ability] of [[me, 1], [me, 5], [me, 9], [other, 9], [caster, 13], [{ kind: "creature", id: 103 }, 14]] as const) {
    test(`accepted ability ${ability} animates actor ${source.id} through model nodes`, () => {
      const effects = new SpellEffects();
      const unit = entities.find((e) => e.kind === source.kind && e.entityId === source.id)!;
      const classId = ability === 5 ? "ranger" : ability === 9 ? "arcanist" : "warden";
      const idle = transforms(rendered(unit, effects, 0, undefined, classId));
      if (source.id === 1 && ability === 5) {
        expect(rendered(unit, effects, 0, undefined, classId).some((node) => node.id.endsWith("bow-stave") && node.geometry.kind === "mesh")).toBe(true);
      }
      receive(effects, [{ kind: "ability-used", source, target: ability === 14 ? source : wolf, ability }]);
      expect(transforms(rendered(unit, effects, 1.12, undefined, classId))).not.toEqual(idle);
      expect(transforms(rendered(unit, effects, 2, undefined, classId))).toEqual(idle);
    });
  }
  for (const [source, ability, ticks] of [[other, 9, 60], [caster, 13, 45], [{ kind: "creature", id: 103 }, 14, 60]] as const) {
    test(`visible cast ${ability} holds and interrupt clears actor ${source.id}`, () => {
      const effects = new SpellEffects();
      receive(effects, [{ kind: "cast-started", source, target: wolf, ability, ticks }]);
      expect(effects.actionFor(source, 1.5)?.hold).toBe(true);
      receive(effects, [{ kind: "interrupted", source: me, target: source, ability }], 1.6, 11n);
      expect(effects.actionFor(source, 2)).toBeNull();
    });
  }
  test("generic damage, periodic/channel pulses and refusal never invent ordinary attack motion", () => {
    const effects = new SpellEffects();
    receive(effects, [
      { kind: "damage-dealt", source: me, target: wolf, amount: 4, critical: false },
      { kind: "damage-observed", source: other, target: wolf, amount: 3, critical: false },
      { kind: "error", code: "out-of-range", target: wolf },
    ]);
    expect(effects.actionFor(me, 1.12)).toBeNull();
    expect(effects.actionFor(other, 1.12)).toBeNull();
    expect(effects.actionFor(wolf, 1.12)).toBeNull();
  });
  test("ranged ability retains draw/cast identity instead of becoming a melee swing", () => {
    const effects = new SpellEffects();
    receive(effects, [{ kind: "ability-used", source: other, target: wolf, ability: 5 }]);
    expect(effects.actionFor(other, 1.12)?.pose).toBe("draw");
  });
  test("death clears a live actor action even when the same projection carries an attempt", () => {
    const effects = new SpellEffects();
    const dead = entities.map(e => e.kind === other.kind && e.entityId === other.id ? { ...e, flags: { ...e.flags, dead: true }, healthPercent: 0 } : e);
    receive(effects, [{ kind: "attack-performed", source: other, target: wolf }, { kind: "died", entity: other, killer: wolf }], 1, 10n, dead);
    expect(effects.actionFor(other, 1.1)).toBeNull();
  });
  test("duplicate received tick cannot restart a transient action", () => {
    const effects = new SpellEffects();
    const events: ZoneEvent[] = [{ kind: "attack-performed", source: other, target: wolf }];
    receive(effects, events);
    receive(effects, events, 2, 10n);
    expect(effects.actionFor(other, 2)).toBeNull();
  });
  test("interest exit and session clear discard actor and effect state", () => {
    const effects = new SpellEffects();
    receive(effects, [{ kind: "cast-started", source: caster, target: me, ability: 13, ticks: 45 }]);
    receive(effects, [], 1.1, 11n, [entities[0]!]);
    expect(effects.actionFor(caster, 1.2)).toBeNull();
    receive(effects, [{ kind: "attack-performed", source: other, target: wolf }], 2, 12n);
    effects.clear();
    expect(effects.actionFor(other, 2.1)).toBeNull();
    expect(effects.active(2.1)).toEqual([]);
    // Reconnect can legitimately begin at an earlier tick: clear resets deduplication.
    receive(effects, [{ kind: "attack-performed", source: other, target: wolf }], 3, 1n);
    expect(effects.actionFor(other, 3.1)).not.toBeNull();
  });
});

describe("confirmed target impacts and participant semantics", () => {
  test("observed positive damage produces bounded changing target-local impact, without crediting the viewer", () => {
    const effects = new SpellEffects();
    const view = receive(effects, [{ kind: "damage-observed", source: other, target: wolf, amount: 4, critical: true }]);
    expect(combatTexts(view)).toEqual([]);
    const status = { textContent: "" }, feedback = { textContent: "" };
    new CombatHud(status, feedback).update(view, catalog, 1);
    expect(status.textContent).not.toContain("Attacking");
    expect(feedback.textContent ?? "").not.toMatch(/you (hit|deal|take)/i);
    const start = allEffectNodes(effects.active(1.02), 1.02, anchor);
    const middle = allEffectNodes(effects.active(1.12), 1.12, anchor);
    expect(start.length).toBeGreaterThan(2); // richer target feedback than a single projectile core
    expect(start.length).toBeLessThanOrEqual(64);
    expect(middle).not.toEqual(start);
    expect(start.every((node) => {
      const at = node.transform?.translation;
      return at && Math.hypot(at[0] - 6, at[1] - 1, at[2]) < 3;
    })).toBe(true);
    expect(effects.active(3)).toEqual([]);
  });
  test("a miss has attempt motion but no confirmed impact; refused and zero damage have neither impact", () => {
    const effects = new SpellEffects();
    receive(effects, [{ kind: "attack-performed", source: other, target: wolf }, { kind: "miss", source: other, target: wolf }]);
    expect(effects.actionFor(other, 1.12)).not.toBeNull();
    expect(effects.active(1.12)).toEqual([]);
    receive(effects, [{ kind: "error", code: "out-of-range", target: wolf }, { kind: "damage-observed", source: other, target: wolf, amount: 0, critical: false }], 2, 11n);
    expect(effects.active(2.1)).toEqual([]);
  });
  test("own damage feedback remains once, even if a neutral copy is present", () => {
    const effects = new SpellEffects();
    const view = receive(effects, [{ kind: "damage-dealt", source: me, target: wolf, amount: 4, critical: false }]);
    expect(combatTexts(view)).toEqual([{ kind: "damage", text: "4" }]);
    expect(effects.active(1.1).length).toBeGreaterThan(0);
  });
  test("many confirmed impacts remain bounded and missing target geometry draws nothing", () => {
    const effects = new SpellEffects();
    for (let i = 0; i < 100; i++) {
      receive(effects, [{ kind: "damage-observed", source: other, target: wolf, amount: 1, critical: false }], 1, BigInt(i));
    }
    expect(effects.active(1.1).length).toBeLessThanOrEqual(24);
    expect(allEffectNodes(effects.active(1.1), 1.1, () => null)).toEqual([]);
  });
});

describe("life-state playback overrides actions", () => {
  for (const original of [entities[0]!, entities[1]!, entities[2]!, entities[3]!, entities[4]!]) {
    test(`${original.kind} ${original.entityId}: live-to-dead transition, stable corpse, no action resurrection`, () => {
      const animators = new UnitAnimators();
      const effects = new SpellEffects();
      expect(animators.deathProgress(original, 0, false)).toBeNull();
      const dead = { ...original, healthPercent: 0, flags: { ...original.flags, dead: true } };
      const start = animators.deathProgress(dead, 0, false)!;
      const middle = animators.deathProgress(dead, 0.15, false)!;
      const end = animators.deathProgress(dead, 5, false)!;
      expect(start).toBe(0);
      expect(middle).toBeGreaterThan(start);
      expect(middle).toBeLessThan(1);
      expect(end).toBe(1);
      const beginning = transforms(rendered(dead, effects, 1, start));
      const during = transforms(rendered(dead, effects, 1.15, middle));
      const corpse = transforms(rendered(dead, effects, 6, end));
      expect(during).not.toEqual(beginning);
      expect(corpse).not.toEqual(during);
      receive(effects, [{ kind: "attack-performed", source: { kind: original.kind, id: original.entityId }, target: wolf }]);
      expect(transforms(rendered(dead, effects, 1.12, 1))).toEqual(corpse);
      expect(animators.deathProgress(dead, 5, false)).toBe(1);
      expect(animators.deathProgress(original, 0, false)).toBeNull();
    });
  }
  test("initial corpse hydration and reduced motion snap; retain and clear forget death history", () => {
    const animators = new UnitAnimators();
    const alive = entities[0]!;
    const dead = { ...alive, flags: { ...NO_FLAGS, dead: true }, healthPercent: 0 };
    expect(animators.deathProgress(dead, 0, false)).toBe(1);
    animators.deathProgress(alive, 0, false);
    expect(animators.deathProgress(dead, 0, true)).toBe(1);
    animators.retain(new Set());
    expect(animators.deathProgress(dead, 0, false)).toBe(1);
    animators.clear();
    expect(animators.deathProgress(dead, 0, false)).toBe(1);
  });
  test("legitimate NPC idle remains modelled and does not inherit another actor's action", () => {
    const npc: EntityState = { ...playerEntity(1, [0, 90, 0]), kind: "npc", appearance: 1 };
    const effects = new SpellEffects();
    const idle = rendered(npc, effects, 0);
    expect(idle.length).toBeGreaterThan(5);
    receive(effects, [{ kind: "attack-performed", source: me, target: wolf }]);
    expect(rendered(npc, effects, 1.12)).toEqual(idle);
  });
});
