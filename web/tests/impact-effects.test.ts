import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { impactFrameAt, parseImpactAtlas } from "../src/world/impact-effects";

const atlas = (name: string) => parseImpactAtlas(JSON.parse(readFileSync(new URL(`../public/effects/${name}/atlas.json`, import.meta.url), "utf8")));

describe("authored impact effects", () => {
  test("samples the shipped puff sequence by fixed age and expires it", () => {
    const puff = atlas("puff.strong");
    expect(puff.durationMs).toBe(600);
    expect(impactFrameAt(puff, -1)).toBe(-1);
    expect(impactFrameAt(puff, 0)).toBe(0);
    expect(impactFrameAt(puff, 59)).toBe(0);
    expect(impactFrameAt(puff, 60)).toBe(1);
    expect(impactFrameAt(puff, 250)).toBe(3);
    expect(impactFrameAt(puff, 599)).toBe(5);
    expect(impactFrameAt(puff, 600)).toBe(-1);
    for (const name of ["puff.subtle", "ring.strong", "ring.subtle"]) { expect(atlas(name).frames.length).toBe(6); }
  });
});
