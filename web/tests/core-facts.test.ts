import { describe, expect, test } from "bun:test";
import { TICK_HZ } from "../src/replication";
import { ABILITY_PRESENTATION } from "../src/world/units/ability-presentation";
import { wasm } from "../src/wasm";
import { abilityFacts, type AbilityFacts, zoneConstants } from "../src/zone-constants";

/**
 * The browser states facts the core decides (#1159): its numbers come from `mmorpg-wasm`, and each
 * ability description is held to the ability's definition, so a core change cannot leave the
 * interface saying something false.
 */

const COUNT_WORDS: Record<string, number> = { one: 1, two: 2, three: 3, four: 4, five: 5 };

type Stated = Pick<AbilityFacts, "metres" | "seconds" | "percents" | "counts"> & {
  knockback: boolean;
  wallsStop: boolean;
  shieldsStop: boolean;
  needsSight: boolean;
  forcedMoveImmunity: boolean;
};

/** What a description claims, read from its prose. */
function stated(description: string): Stated {
  const numbers = (pattern: RegExp) => [...description.matchAll(pattern)].map((match) => Number(match[1]));
  const counts = [
    ...[...description.matchAll(/\bup to (one|two|three|four|five)\b/gi)].map((match) => COUNT_WORDS[match[1]!.toLowerCase()]!),
    ...(/\btwice\b/i.test(description) ? [2] : []),
    ...(/\bonly one\b/i.test(description) ? [1] : []),
  ];
  return {
    metres: numbers(/(\d+(?:\.\d+)?) m\b/g),
    seconds: [...numbers(/(\d+(?:\.\d+)?) s\b/g), ...(/\ba minute\b/.test(description) ? [60] : [])],
    percents: numbers(/(\d+)%/g),
    counts,
    knockback: /\b(knocks?|knocked|hurls?|hurling|thrown) (it |them )?back\b/i.test(description),
    wallsStop: /\bwalls (and raised shields )?stop\b|\buntil a wall\b|\bwalls and summoned terrain end it\b/i.test(description),
    shieldsStop: /\braised shields? stops?\b|\bor raised shield\b/i.test(description),
    needsSight: /\bwalls shield those behind them\b|\b(you|it) can see\b/i.test(description),
    forcedMoveImmunity: /\bimmune\b/i.test(description),
  };
}

describe("core facts the browser states", () => {
  test("the browser's tick rate and reaches are the core's", () => {
    expect(TICK_HZ).toBe(wasm.tickHz());
    const constants = zoneConstants();
    for (const reach of [constants.lootReachUnits, constants.vendorReachUnits, constants.questReachUnits, constants.gatherReachUnits, constants.aidReachUnits, constants.tracksReachUnits, constants.braceReachUnits]) {
      expect(reach).toBeGreaterThan(0);
    }
  });

  test("every ability description states only what its definition does", () => {
    const facts = abilityFacts();
    const wrong: string[] = [];
    for (const [id, { description }] of ABILITY_PRESENTATION) {
      const truth = facts.get(id);
      if (!truth) {
        wrong.push(`${id}: no catalog ability`);
        continue;
      }
      const claims = stated(description);
      const missing = (kind: string, said: readonly number[], held: readonly number[]) => {
        for (const value of said) {
          if (!held.some((fact) => Math.abs(fact - value) < 1e-9)) {
            wrong.push(`${id}: states ${value} ${kind}, the definition holds ${held.join(", ") || "none"}`);
          }
        }
      };
      missing("m", claims.metres, truth.metres);
      missing("s", claims.seconds, truth.seconds);
      missing("%", claims.percents, truth.percents);
      missing("times", claims.counts, truth.counts);
      // A description must mention knockback and forced-move immunity exactly when the ability has
      // them; the stopping and sight rules it may leave unsaid, but must not claim falsely.
      if (claims.knockback !== truth.knockback) { wrong.push(`${id}: knockback stated ${claims.knockback}, defined ${truth.knockback}`); }
      if (claims.forcedMoveImmunity !== truth.forcedMoveImmunity) { wrong.push(`${id}: immunity stated ${claims.forcedMoveImmunity}, defined ${truth.forcedMoveImmunity}`); }
      if (claims.wallsStop && !truth.wallsStop) { wrong.push(`${id}: says walls stop it`); }
      if (claims.shieldsStop && !truth.shieldsStop) { wrong.push(`${id}: says raised shields stop it`); }
      if (claims.needsSight && !truth.needsSight) { wrong.push(`${id}: says walls shield what lies behind them`); }
    }
    expect(wrong).toEqual([]);
  });

  test("the prose reader finds the facts it checks", () => {
    expect(stated("Hook your selected target and pull it to melee reach; walls stop it and a just-moved target is immune.")).toMatchObject({ wallsStop: true, forcedMoveImmunity: true, knockback: false });
    expect(stated("Gouges your target: interrupts its cast and stuns it without knocking it back.").knockback).toBe(false);
    expect(stated("Leap 8 m along your facing and land within 4 m, hurls back; up to three, twice, for 1.5 s and 40% faster.")).toMatchObject({ metres: [8, 4], seconds: [1.5], percents: [40], counts: [3, 2], knockback: true });
  });
});
