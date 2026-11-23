import { expect, test } from "bun:test";
import * as THREE from "three";
import { CLIP_DETAIL, ClipDetail } from "../src/world/clip-detail";

// The `ambient-clips` budget (#1095, `.performance/contract.json`): ambient creatures and settlement prop clips sample
// only in view and within `farMetres`, every frame near the eye and every `every`th frame further out, so the clips
// sampled per frame follow what is visible and near, not how many are placed.

const RADIUS = 1;

/** A camera at the origin looking down -z. */
function camera(): THREE.PerspectiveCamera {
  const view = new THREE.PerspectiveCamera(60, 16 / 9, 0.1, 2000);
  view.position.set(0, 0, 0);
  view.lookAt(0, 0, -1);
  view.updateMatrixWorld(true);
  return view;
}

/**
 * `near` clips 10 m ahead, `middle` ones 100 m ahead, then the rest placed behind the camera (off-screen) and far ahead
 * (beyond `farMetres`), `placed` in all.
 */
function placements(placed: number, near: number, middle: number): [number, number, number][] {
  return Array.from({ length: placed }, (_, index): [number, number, number] => {
    if (index < near) { return [index * 0.1 - near * 0.05, 0, -10]; }
    if (index < near + middle) { return [(index - near) * 0.5 - middle * 0.25, 0, -100]; }
    return index % 2 === 0 ? [index, 0, 50] : [0, 0, -(CLIP_DETAIL.farMetres + 50 + index)];
  });
}

/** Runs `frames` frames and returns each frame's sampled count, and which clips sampled at all. */
function run(placed: number, near: number, middle: number, frames: number, animate = true) {
  const at = placements(placed, near, middle);
  const detail = new ClipDetail(placed);
  const view = camera();
  const sampled: number[] = [];
  const ever = new Set<number>();
  for (let frame = 0; frame < frames; frame += 1) {
    detail.begin(view, animate);
    at.forEach(([x, y, z], index) => { if (detail.sample(index, x, y, z, RADIUS)) { ever.add(index); } });
    expect(detail.work.visibleNear).toBe(near);
    expect(detail.work.visibleMiddle).toBe(middle);
    sampled.push(detail.work.sampled);
  }
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
  const largest = [64, 256, 1024].map((placed) => Math.max(...run(placed, 4, 12, 64).sampled.slice(1)));
  expect(new Set(largest).size).toBe(1);
  expect(largest[0]!).toBeLessThanOrEqual(4 + Math.ceil(12 / CLIP_DETAIL.every));
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
