import { describe, expect, test } from "bun:test";
import { smokeRate } from "../src/world/raid-smoke";
import { raidWarning } from "../src/world/units/quest-panel";

describe("night raid signals (#530)", () => {
  test("smoke thickens as the barn loses integrity and stops once it is restored", () => {
    expect(smokeRate(null)).toBe(0);
    const intact = smokeRate({ phase: 1, integrity: 100 });
    const burning = smokeRate({ phase: 3, integrity: 20 });
    expect(intact).toBeGreaterThan(0);
    expect(burning).toBeGreaterThan(intact);
    // After the raid the damaged barn still smokes while it recovers, less than under attack.
    const recovering = smokeRate({ phase: 0, integrity: 20 });
    expect(recovering).toBeGreaterThan(0);
    expect(recovering).toBeLessThan(burning);
    expect(smokeRate({ phase: 0, integrity: 100 })).toBe(0);
  });

  test("residents warn of the assault in their own voice", () => {
    expect(raidWarning("Farmer Osric Mill")).toStartWith("Farmer Osric Mill: ");
  });
});
