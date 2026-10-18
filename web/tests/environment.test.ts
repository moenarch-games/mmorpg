import { describe, expect, test } from "bun:test";
import { DAY_SECONDS, ENVIRONMENT, ZONE_DAY_TICKS, hazeMix, mergeNearColors, mixColor, quantizeColor, shadeColor, skyAt, zoneSkySeconds } from "../src/world/environment";

const HEX = /^#[0-9a-f]{6}$/;

describe("environment style", () => {
  test("colour helpers stay valid colours", () => {
    expect(mixColor("#000000", "#ffffff", 0.5)).toBe("#808080");
    expect(mixColor("#102030", "#ffffff", Number.NaN)).toBe("#102030");
    expect(mixColor("#102030", "#ffffff", 7)).toBe("#ffffff");
    expect(shadeColor("#ff8000", 2)).toBe("#ffff00");
    expect(quantizeColor("#0b1f3d", 8)).toBe("#082040");
  });

  test("near-identical colours merge into the more common one, never farther than the distance", () => {
    const colors = [
      ...Array<string>(5).fill("#808080"),
      ...Array<string>(3).fill("#8c8c8c"), // 20.8 from the common grey: kept.
      "#848484", // 6.9 from the common grey, 13.9 from the lighter one.
      "#888888", // 13.9 from the common grey, 6.9 from the lighter one.
      "#80808c", // Exactly 12 from the common grey.
      "#a0a0a0", // Far from both: kept.
    ] as `#${string}`[];
    const merged = Object.fromEntries(mergeNearColors(colors, 12));
    expect(merged).toEqual({
      "#808080": "#808080",
      "#8c8c8c": "#8c8c8c",
      "#848484": "#808080",
      "#888888": "#8c8c8c",
      "#80808c": "#808080",
      "#a0a0a0": "#a0a0a0",
    });
    expect(Object.fromEntries(mergeNearColors([...colors].reverse(), 12))).toEqual(merged);
    // The rarer of two near colours gives way, whichever it is.
    expect(mergeNearColors(["#808080", "#848484", "#848484"], 12).get("#808080")).toBe("#848484");
  });

  test("haze grows with distance band by band", () => {
    let previous = -1;
    for (const distance of [0, 100, 240, 320, 400, 500, 900]) {
      const mix = hazeMix(ENVIRONMENT, distance);
      expect(mix).toBeGreaterThanOrEqual(previous);
      previous = mix;
    }
    expect(hazeMix(ENVIRONMENT, 0)).toBe(0);
  });

  test("the sky cycle is periodic and starts at the style's midday", () => {
    expect(skyAt(ENVIRONMENT, 0)).toEqual(ENVIRONMENT.sky);
    expect(skyAt(ENVIRONMENT, 1)).toEqual(ENVIRONMENT.sky);
    expect(skyAt(ENVIRONMENT, 0.25)).toEqual(skyAt(ENVIRONMENT, 1.25));
    for (const fraction of [0.1, 0.5, 0.9, Number.NaN]) {
      const stops = skyAt(ENVIRONMENT, fraction);
      for (const color of Object.values(stops)) {
        expect(color).toMatch(HEX);
      }
    }
    expect(skyAt(ENVIRONMENT, 0.5).zenith).not.toBe(ENVIRONMENT.sky.zenith);
    expect(DAY_SECONDS).toBe(1_200);
  });
});

test("the sky follows the zone clock: tick 0 is 08:00 and a zone day is one sky cycle (#158)", () => {
  expect(ZONE_DAY_TICKS).toBe(216_000);
  const hour = BigInt(ZONE_DAY_TICKS / 24);
  expect(zoneSkySeconds(4n * hour)).toBeCloseTo(0, 6); // noon is the cycle's zero
  expect(zoneSkySeconds(16n * hour)).toBeCloseTo(DAY_SECONDS / 2, 6); // midnight
  expect(zoneSkySeconds(0n)).toBeCloseTo(DAY_SECONDS * (5 / 6), 6);
  expect(zoneSkySeconds(BigInt(ZONE_DAY_TICKS))).toBeCloseTo(zoneSkySeconds(0n), 6);
});
