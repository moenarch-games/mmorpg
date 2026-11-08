import type { Axis } from "../command-wire";
import type { EntityRef } from "../entity-ref";
import type { MovementInput } from "./movement-outbox";
import { yawFromRadians } from "../replication";

export type CameraMode = { kind: "free-look" } | { kind: "focused"; target: EntityRef };
export type PointerOwner = "camera" | "interface" | "gesture";

/** Pointer ownership is explicit; focus and held gestures supersede free mouse-look. */
export function pointerOwner(mode: CameraMode, captured: boolean, gesture: boolean): PointerOwner {
  if (gesture) { return "gesture"; }
  return mode.kind === "free-look" && captured ? "camera" : "interface";
}

/**
 * Third-person shoulder camera around the local character, mirroring the native
 * client's `OrbitCamera`. The eye never sinks below the ground under it and
 * focused tracking only changes presentation and the facing movement intent.
 */
const PIVOT_HEIGHT_METRES = 0.6;
/** The eye stays at least this far above the ground under it. */
export const GROUND_CLEARANCE_METRES = 0.35;
/** Smooth zoom covers about 1 − e^(−rate × seconds) of the remaining distance. */
const ZOOM_RATE_PER_SECOND = 12;
export const MIN_DISTANCE_METRES = 2;
export const MAX_DISTANCE_METRES = 30;
const MIN_PITCH_RADIANS = 0.05;
const MAX_PITCH_RADIANS = 1.45;
export const ORBIT_RADIANS_PER_PIXEL = 0.006;
/** Each wheel line changes the distance by this factor. */
const ZOOM_STEP = 0.88;
const SHOULDER_OFFSET_METRES = 0.65;
const FOCUS_TURN_RATE = 10;
const FOCUS_FRAMING_RATE = 7;
const FOCUS_FRAMING_METRES = 3;
/** Browser pixel scrolling: this many pixels count as one wheel line. */
export const PIXELS_PER_WHEEL_LINE = 40;

export type Vec3 = [number, number, number];
export type CameraView = { eye: Vec3; target: Vec3 };
/** An axis-aligned box in metres. */
export type CameraBox = { min: Vec3; max: Vec3 };

/** How far in front of the first blocking surface a pulled-in eye stays, in metres. */
const SIGHT_CLEARANCE_METRES = 0.3;

/**
 * The view with its eye pulled in toward the target until the sight line clears every box (a
 * camera boom, #552): inside the Battle Royale fortress the eye stays under roofs and in front
 * of walls instead of looking at their backs. Boxes that contain the target are ignored.
 */
export function clearSightLine(view: CameraView, boxes: readonly CameraBox[]): CameraView {
  const { eye, target } = view;
  const ray = [0, 1, 2].map((axis) => eye[axis]! - target[axis]!) as Vec3;
  const length = Math.hypot(...ray);
  if (length < 1e-6) { return view; }
  let nearest = 1;
  for (const box of boxes) {
    let enter = 0;
    let exit = 1;
    for (let axis = 0; axis < 3 && enter <= exit; axis += 1) {
      const origin = target[axis]!;
      const step = ray[axis]!;
      const [low, high] = [box.min[axis]!, box.max[axis]!];
      if (Math.abs(step) < 1e-9) {
        if (origin < low || origin > high) { exit = -1; }
        continue;
      }
      const [a, b] = [(low - origin) / step, (high - origin) / step];
      enter = Math.max(enter, Math.min(a, b));
      exit = Math.min(exit, Math.max(a, b));
    }
    const containsTarget = target.every((value, axis) => value >= box.min[axis]! && value <= box.max[axis]!);
    if (enter <= exit && !containsTarget && enter < nearest) { nearest = enter; }
  }
  if (nearest >= 1) { return view; }
  const t = Math.max(0, nearest - SIGHT_CLEARANCE_METRES / length);
  return { eye: [0, 1, 2].map((axis) => target[axis]! + ray[axis]! * t) as Vec3, target };
}

export class OrbitCamera {
  /** World yaw the camera looks along: 0 toward +Z, increasing toward +X. */
  #yaw = 0;
  #invertVerticalLook = false;
  #pitch = 0.42;
  #distance = 12;
  #targetDistance = 12;
  #framing: Vec3 = [0, 0, 0];

  /** The distance the view uses now; it approaches `targetDistance` in `update`. */
  get distance(): number {
    return this.#distance;
  }

  get targetDistance(): number {
    return this.#targetDistance;
  }

  setLookInversion(invert: boolean): void {
    this.#invertVerticalLook = invert;
  }

  /** Mouse drag: moving right turns the view right; moving down raises the eye. */
  orbit(dxPixels: number, dyPixels: number): void {
    if (!Number.isFinite(dxPixels) || !Number.isFinite(dyPixels)) {
      return;
    }
    const turn = dxPixels * ORBIT_RADIANS_PER_PIXEL;
    const fullTurn = 2 * Math.PI;
    this.#yaw = (((this.#yaw - turn) % fullTurn) + fullTurn) % fullTurn;
    this.#pitch = Math.min(MAX_PITCH_RADIANS, Math.max(MIN_PITCH_RADIANS, this.#pitch + dyPixels * ORBIT_RADIANS_PER_PIXEL * (this.#invertVerticalLook ? -1 : 1)));
  }

  /** Positive wheel lines zoom in; the view follows smoothly in `update`. */
  zoom(lines: number): void {
    if (Number.isFinite(lines)) {
      this.#targetDistance = Math.min(MAX_DISTANCE_METRES, Math.max(MIN_DISTANCE_METRES, this.#targetDistance * ZOOM_STEP ** lines));
    }
  }

  /** A pinch: spreading the fingers by `scale` brings the camera that much closer. */
  zoomBy(scale: number): void {
    if (Number.isFinite(scale) && scale > 0) {
      this.#targetDistance = Math.min(MAX_DISTANCE_METRES, Math.max(MIN_DISTANCE_METRES, this.#targetDistance / scale));
    }
  }

  /** Advances smooth zoom; `instant` (reduced motion) jumps to the target. */
  update(deltaSeconds: number, instant = false): void {
    const remaining = this.#targetDistance - this.#distance;
    const blend = instant || !Number.isFinite(deltaSeconds) ? 1 : 1 - Math.exp(-ZOOM_RATE_PER_SECOND * Math.max(0, deltaSeconds));
    this.#distance += remaining * blend;
    if (Math.abs(this.#targetDistance - this.#distance) < 1e-3) {
      this.#distance = this.#targetDistance;
    }
  }

  /** Follow a projected opponent without snapping when focus is acquired, switched or cleared. */
  track(self: Vec3, opponent: Vec3 | null, deltaSeconds: number): void {
    const dt = Math.max(0, Number.isFinite(deltaSeconds) ? deltaSeconds : 0);
    if (opponent) {
      const desired = Math.atan2(opponent[0] - self[0], opponent[2] - self[2]);
      const shortest = Math.atan2(Math.sin(desired - this.#yaw), Math.cos(desired - this.#yaw));
      this.#yaw = (this.#yaw + shortest * (1 - Math.exp(-FOCUS_TURN_RATE * dt)) + 2 * Math.PI) % (2 * Math.PI);
    }
    const dx = opponent ? opponent[0] - self[0] : 0;
    const dz = opponent ? opponent[2] - self[2] : 0;
    const scale = Math.min(1, FOCUS_FRAMING_METRES / Math.max(1e-6, Math.hypot(dx, dz)));
    const desired: Vec3 = opponent ? [dx * scale * 0.35, (opponent[1] - self[1]) * 0.2, dz * scale * 0.35] : [0, 0, 0];
    const blend = 1 - Math.exp(-FOCUS_FRAMING_RATE * dt);
    this.#framing = this.#framing.map((value, axis) => value + (desired[axis]! - value) * blend) as Vec3;
  }

  /** Turns the view to a heading in radians (0 toward +Z, increasing toward +X). */
  lookAlong(heading: number): void {
    if (Number.isFinite(heading)) {
      const fullTurn = 2 * Math.PI;
      this.#yaw = ((heading % fullTurn) + fullTurn) % fullTurn;
    }
  }

  /** The view's heading in radians: 0 toward +Z, increasing toward +X. */
  get heading(): number {
    return this.#yaw;
  }

  /** The heading a character takes while moving under this camera. */
  facing(): number {
    return yawFromRadians(this.#yaw);
  }

  /**
   * Eye and look-at point for a focus at the character's body centre, in
   * metres. With `groundAt`, the eye is raised to stay above the ground at
   * its own position; nothing else collides.
   */
  view(focus: Vec3, groundAt?: (x: number, z: number) => number): CameraView {
    const target: Vec3 = [focus[0] + this.#framing[0], focus[1] + PIVOT_HEIGHT_METRES + this.#framing[1], focus[2] + this.#framing[2]];
    const horizontal = this.#distance * Math.cos(this.#pitch);
    const eye: Vec3 = [
      focus[0] - Math.sin(this.#yaw) * horizontal + Math.cos(this.#yaw) * SHOULDER_OFFSET_METRES,
      focus[1] + PIVOT_HEIGHT_METRES + this.#distance * Math.sin(this.#pitch),
      focus[2] - Math.cos(this.#yaw) * horizontal - Math.sin(this.#yaw) * SHOULDER_OFFSET_METRES,
    ];
    if (groundAt) {
      const floor = groundAt(eye[0], eye[2]) + GROUND_CLEARANCE_METRES;
      if (Number.isFinite(floor) && eye[1] < floor) {
        eye[1] = floor;
      }
    }
    return { eye, target };
  }
}

/**
 * Held movement as semantic axes: run/backpedal, strafe, whether any movement is held, and the
 * held swim ascent (Jump) or descent (#274); an absent `rise` means neither.
 */
export type HeldIntent = { forward: Axis; strafe: Axis; steering: boolean; rise?: Axis };

export const IDLE_INTENT: HeldIntent = { forward: 0, strafe: 0, steering: false };

/**
 * Touch and uncaptured legacy drag fallback; captured mouse-look and enemy focus
 * use `followLook` and retain character-local A/D strafing.
 */
export type DragMode = "none" | "orbit" | "turn";

/**
 * Movement intent. While the camera owns look or tracks a focused opponent,
 * the character follows its heading even when idle; A/D remain local strafes.
 */
export function movementInput(
  held: HeldIntent,
  cameraFacing: number,
  lastFacing: number,
  jumps: number,
  drag: DragMode = "none",
  followLook = false,
): MovementInput {
  const facing = followLook || drag === "turn" ? cameraFacing : drag === "orbit" ? lastFacing : held.steering ? cameraFacing : lastFacing;
  return { forward: held.forward, strafe: held.strafe, facing, jumps, ...(held.rise === undefined ? {} : { rise: held.rise }) };
}
