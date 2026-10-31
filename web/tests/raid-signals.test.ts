import { describe, expect, test } from "bun:test";
import { smokeRate } from "../src/world/raid-smoke";
import { aftermathOpen, reachableAidTask } from "../src/world/raid-aid";
import { forewarningLine, prepSpots, raidNoticeText, reachablePrepStep } from "../src/world/raid-prep";
import { aftermathPlea, raidWarning } from "../src/world/units/quest-panel";

/** A signal without forewarning (#532). */
const quiet = { signs: false, readiness: 0, scouted: false };

describe("night raid signals (#530)", () => {
  test("smoke thickens as the barn loses integrity and stops once it is restored", () => {
    expect(smokeRate(null)).toBe(0);
    const intact = smokeRate({ ...quiet, phase: 1, integrity: 100, aid: 0 });
    const burning = smokeRate({ ...quiet, phase: 3, integrity: 20, aid: 0 });
    expect(intact).toBeGreaterThan(0);
    expect(burning).toBeGreaterThan(intact);
    // After the raid the damaged barn still smokes while it recovers, less than under attack.
    const recovering = smokeRate({ ...quiet, phase: 0, integrity: 20, aid: 0 });
    expect(recovering).toBeGreaterThan(0);
    expect(recovering).toBeLessThan(burning);
    expect(smokeRate({ ...quiet, phase: 0, integrity: 100, aid: 0 })).toBe(0);
  });

  test("residents warn of the assault in their own voice", () => {
    expect(raidWarning("Farmer Osric Mill")).toStartWith("Farmer Osric Mill: ");
  });
});

describe("night raid aftermath aid (#531)", () => {
  const tasks = [{ x: 8_000, z: 3_450, npc: 0 }, { x: 5_700, z: 4_500, npc: 9 }];

  test("interact finds the open task within the zone's 3 m reach", () => {
    const open = { ...quiet, phase: 0, integrity: 0, aid: 0b11 };
    expect(reachableAidTask(open, tasks, 5_700, 4_250)).toBe(1);
    expect(reachableAidTask(open, tasks, 5_700, 4_150)).toBeNull();
    expect(reachableAidTask({ ...open, aid: 0b01 }, tasks, 5_700, 4_500)).toBeNull();
    expect(reachableAidTask(null, tasks, 8_000, 3_450)).toBeNull();
  });

  test("residents ask for help only while aid tasks are open after the raid", () => {
    expect(aftermathOpen({ ...quiet, phase: 0, integrity: 40, aid: 0b100 })).toBe(true);
    expect(aftermathOpen({ ...quiet, phase: 0, integrity: 90, aid: 0 })).toBe(false);
    expect(aftermathOpen({ ...quiet, phase: 2, integrity: 40, aid: 0 })).toBe(false);
    expect(aftermathPlea("Farmer Osric Mill")).toStartWith("Farmer Osric Mill: ");
  });
});

describe("night raid forewarning and preparation (#532)", () => {
  const spots = prepSpots([6_250, 1_750, 9, 7_250, 4_300, 8_750, 4_700])!;
  const lookout = { x: 2_900, z: 2_600 };
  const evening = { phase: 0, integrity: 100, aid: 0, signs: true, readiness: 0, scouted: false };

  test("interact reads the tracks, reports them to the lookout, then braces the barn", () => {
    expect(spots.braces).toHaveLength(2);
    expect(reachablePrepStep(evening, spots, 6_250, 1_600, lookout)).toBe(0);
    expect(reachablePrepStep(evening, spots, 6_250, 1_400, lookout)).toBeNull();
    expect(reachablePrepStep(evening, spots, 2_900, 2_800, lookout)).toBeNull();
    const scouted = { ...evening, scouted: true };
    expect(reachablePrepStep(scouted, spots, 2_900, 2_800, lookout)).toBe(1);
    expect(reachablePrepStep(scouted, spots, 7_250, 4_300, lookout)).toBeNull();
    const warned = { ...scouted, readiness: 0b001 };
    expect(reachablePrepStep(warned, spots, 7_250, 4_350, lookout)).toBe(2);
    expect(reachablePrepStep({ ...warned, readiness: 0b011 }, spots, 7_250, 4_350, lookout)).toBeNull();
    expect(reachablePrepStep({ ...warned, readiness: 0b011 }, spots, 8_750, 4_650, lookout)).toBe(3);
    expect(reachablePrepStep({ ...quiet, phase: 1, integrity: 90, aid: 0 }, spots, 6_250, 1_750, lookout)).toBeNull();
  });

  test("guards speak of the signs, and the notices never claim a saved barn that fell", () => {
    expect(forewarningLine("Greyhaven Guard", evening, true)).toContain("Could be nothing");
    expect(forewarningLine("Greyhaven Guard", { ...evening, scouted: true }, true)).toContain("report");
    expect(forewarningLine("Greyhaven Guard", { ...evening, readiness: 1 }, false)).toContain("brace");
    expect(raidNoticeText("fell")).toContain("broke into the barn");
    expect(raidNoticeText("held")).toContain("barn stands");
    expect(raidWarning("Farmer Osric Mill", true)).toContain("warned");
  });
});
