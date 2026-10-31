import { describe, expect, test } from "bun:test";
import { smokeRate } from "../src/world/raid-smoke";
import { aftermathOpen, reachableAidTask } from "../src/world/raid-aid";
import { aftermathPlea, raidWarning } from "../src/world/units/quest-panel";

describe("night raid signals (#530)", () => {
  test("smoke thickens as the barn loses integrity and stops once it is restored", () => {
    expect(smokeRate(null)).toBe(0);
    const intact = smokeRate({ phase: 1, integrity: 100, aid: 0 });
    const burning = smokeRate({ phase: 3, integrity: 20, aid: 0 });
    expect(intact).toBeGreaterThan(0);
    expect(burning).toBeGreaterThan(intact);
    // After the raid the damaged barn still smokes while it recovers, less than under attack.
    const recovering = smokeRate({ phase: 0, integrity: 20, aid: 0 });
    expect(recovering).toBeGreaterThan(0);
    expect(recovering).toBeLessThan(burning);
    expect(smokeRate({ phase: 0, integrity: 100, aid: 0 })).toBe(0);
  });

  test("residents warn of the assault in their own voice", () => {
    expect(raidWarning("Farmer Osric Mill")).toStartWith("Farmer Osric Mill: ");
  });
});

describe("night raid aftermath aid (#531)", () => {
  const tasks = [{ x: 8_000, z: 3_450, npc: 0 }, { x: 5_700, z: 4_500, npc: 9 }];

  test("interact finds the open task within the zone's 3 m reach", () => {
    const open = { phase: 0, integrity: 0, aid: 0b11 };
    expect(reachableAidTask(open, tasks, 5_700, 4_250)).toBe(1);
    expect(reachableAidTask(open, tasks, 5_700, 4_150)).toBeNull();
    expect(reachableAidTask({ ...open, aid: 0b01 }, tasks, 5_700, 4_500)).toBeNull();
    expect(reachableAidTask(null, tasks, 8_000, 3_450)).toBeNull();
  });

  test("residents ask for help only while aid tasks are open after the raid", () => {
    expect(aftermathOpen({ phase: 0, integrity: 40, aid: 0b100 })).toBe(true);
    expect(aftermathOpen({ phase: 0, integrity: 90, aid: 0 })).toBe(false);
    expect(aftermathOpen({ phase: 2, integrity: 40, aid: 0 })).toBe(false);
    expect(aftermathPlea("Farmer Osric Mill")).toStartWith("Farmer Osric Mill: ");
  });
});
