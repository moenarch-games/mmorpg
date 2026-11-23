import { expect, test } from "bun:test";
import { CLIP_DETAIL, ClipDetail } from "../src/world/clip-detail";
import { RADIUS, WARM_CLIP_FRAMES, camera, clipFrames, largestClipsSampledPerFrame, placements } from "./support/work/ambient-clips";

// The `ambient-clips` budget (#1095, `.performance/contract.json`): ambient creatures and settlement prop clips sample
// only in view and within `farMetres`, every frame near the eye and every `every`th frame further out, so the clips
// sampled per frame follow what is visible and near, not how many are placed. `support/work/ambient-clips.ts` measures
// it; `scripts/work-evidence.ts` emits the same measurement (#1120).

/** Runs `frames` frames and returns each frame's sampled count, and which clips sampled at all. */
function run(placed: number, near: number, middle: number, frames: number, animate = true) {
  const { sampled, visibleNear, visibleMiddle, ever, detail } = clipFrames(placed, near, middle, frames, animate);
  expect(visibleNear.every((count) => count === near)).toBe(true);
  expect(visibleMiddle.every((count) => count === middle)).toBe(true);
  return { sampled, ever, detail };
}

test("off-screen and far clips never sample; near ones every frame, middle ones in staggered turns", () => {
  const { sampled, ever } = run(64, 4, 12, 30);
  // The first frame samples everything in view: nothing shows a stale pose.
  expect(sampled[0]).toBe(16);
  const middleTurn = Math.ceil(12 / CLIP_DETAIL.every);
  for (const count of sampled.slice(1)) { expect(count).toBeLessThanOrEqual(4 + middleTurn); }
  expect([...ever].every((index) => index < 16)).toBe(true);
});

test("the clips sampled per frame follow the visible-near count, not the placed count", () => {
  // Scale points of `ambient-clips`: the placed count grows, the visible ones stay.
  expect(WARM_CLIP_FRAMES).toBe(64);
  const largest = [64, 256, 1024].map((placed) => largestClipsSampledPerFrame(placed, 4, 12));
  expect(new Set(largest).size).toBe(1);
  expect(largest[0]!).toBeLessThanOrEqual(4 + Math.ceil(12 / CLIP_DETAIL.every));
});

test("the clips sampled per frame grow by one for each visible near clip", () => {
  // Scale points of `ambient-clips` that vary `visible_near` at 256 placed and 12 in the middle distance (#1120).
  const largest = [1, 2, 4].map((near) => largestClipsSampledPerFrame(256, near, 12));
  const middleTurn = Math.ceil(12 / CLIP_DETAIL.every);
  expect(largest).toEqual([1 + middleTurn, 2 + middleTurn, 4 + middleTurn]);
});

test("a clip coming back into view samples at once", () => {
  const detail = new ClipDetail(1);
  const view = camera();
  detail.begin(view, true);
  expect(detail.sample(0, 0, 0, -100, RADIUS)).toBe(true);
  // Behind the camera for a while.
  for (let frame = 0; frame < 10; frame += 1) {
    detail.begin(view, true);
    expect(detail.sample(0, 0, 0, 100, RADIUS)).toBe(false);
  }
  detail.begin(view, true);
  expect(detail.sample(0, 0, 0, -100, RADIUS)).toBe(true);
});

test("under reduced motion each visible clip samples its still pose once", () => {
  const { sampled } = run(64, 4, 12, 20, false);
  expect(sampled[0]).toBe(16);
  expect(sampled.slice(1).every((count) => count === 0)).toBe(true);
});

test("turning motion back on samples the visible clips again", () => {
  const at = placements(16, 4, 12);
  const detail = new ClipDetail(16);
  const view = camera();
  for (const animate of [false, false, true]) {
    detail.begin(view, animate);
    at.forEach(([x, y, z], index) => detail.sample(index, x, y, z, RADIUS));
  }
  expect(detail.work.sampled).toBe(16);
});
