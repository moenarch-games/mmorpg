import * as THREE from "three";
import { CLIP_DETAIL, ClipDetail } from "../../../src/world/clip-detail";

// The `ambient-clips` measurement (#1095, #1120): `web/tests/clip-detail.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. Ambient creatures and settlement prop clips sample only in
// view and within `farMetres`, every frame near the eye and every `every`th frame further out.

export const RADIUS = 1;

/** Frames each scale point of `ambient-clips` runs. */
export const WARM_CLIP_FRAMES = 64;

/** A camera at the origin looking down -z. */
export function camera(): THREE.PerspectiveCamera {
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
export function placements(placed: number, near: number, middle: number): [number, number, number][] {
  return Array.from({ length: placed }, (_, index): [number, number, number] => {
    if (index < near) { return [index * 0.1 - near * 0.05, 0, -10]; }
    if (index < near + middle) { return [(index - near) * 0.5 - middle * 0.25, 0, -100]; }
    return index % 2 === 0 ? [index, 0, 50] : [0, 0, -(CLIP_DETAIL.farMetres + 50 + index)];
  });
}

/**
 * Runs `frames` frames and returns each frame's sampled count and its visible-near and visible-middle counts, and which
 * clips sampled at all.
 */
export function clipFrames(placed: number, near: number, middle: number, frames: number, animate = true) {
  const at = placements(placed, near, middle);
  const detail = new ClipDetail(placed);
  const view = camera();
  const sampled: number[] = [];
  const visibleNear: number[] = [];
  const visibleMiddle: number[] = [];
  const ever = new Set<number>();
  for (let frame = 0; frame < frames; frame += 1) {
    detail.begin(view, animate);
    at.forEach(([x, y, z], index) => { if (detail.sample(index, x, y, z, RADIUS)) { ever.add(index); } });
    visibleNear.push(detail.work.visibleNear);
    visibleMiddle.push(detail.work.visibleMiddle);
    sampled.push(detail.work.sampled);
  }
  return { sampled, visibleNear, visibleMiddle, ever, detail };
}

/**
 * One `ambient-clips` scale point: the most clips any warm frame (after the first) samples over `WARM_CLIP_FRAMES`
 * frames. Throws when the placement does not produce the declared visible counts, so the evidence measures the point it
 * names.
 */
export function largestClipsSampledPerFrame(placed: number, near: number, middle: number): number {
  const { sampled, visibleNear, visibleMiddle } = clipFrames(placed, near, middle, WARM_CLIP_FRAMES);
  if (visibleNear.some((count) => count !== near) || visibleMiddle.some((count) => count !== middle)) {
    throw new Error(`ambient-clips: placing ${placed} clips did not show ${near} near and ${middle} middle ones`);
  }
  return Math.max(...sampled.slice(1));
}
