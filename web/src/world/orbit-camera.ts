import type { Axis } from "../command-wire";
import type { MovementInput } from "./movement-outbox";
import { yawFromRadians } from "../replication";

/**
 * Third-person orbit camera around the local character, mirroring the native
 * client's `OrbitCamera`, with classic-MMO handling: the wheel zooms smoothly,
 * the eye never sinks below the ground under it (no other collision), a
 * left drag orbits freely and a right drag turns the character with the
 * view. Presentation only: its heading becomes movement intent, never an
 * authoritative position.
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
/** Browser pixel scrolling: this many pixels count as one wheel line. */
export const PIXELS_PER_WHEEL_LINE = 40;

export type Vec3 = [number, number, number];
export type CameraView = { eye: Vec3; target: Vec3 };

export class OrbitCamera {
  /** World yaw the camera looks along: 0 toward +Z, increasing toward +X. */
  #yaw = 0;
  #pitch = 0.36;
  #distance = 9;
  #targetDistance = 9;

  /** The distance the view uses now; it approaches `targetDistance` in `update`. */
  get distance(): number {
    return this.#distance;
  }

  get targetDistance(): number {
    return this.#targetDistance;
  }

  /** Mouse drag: moving right turns the view right; moving down raises the eye. */
  orbit(dxPixels: number, dyPixels: number): void {
    if (!Number.isFinite(dxPixels) || !Number.isFinite(dyPixels)) {
      return;
    }
    const turn = dxPixels * ORBIT_RADIANS_PER_PIXEL;
    const fullTurn = 2 * Math.PI;
    this.#yaw = (((this.#yaw - turn) % fullTurn) + fullTurn) % fullTurn;
    this.#pitch = Math.min(MAX_PITCH_RADIANS, Math.max(MIN_PITCH_RADIANS, this.#pitch + dyPixels * ORBIT_RADIANS_PER_PIXEL));
  }

  /** Positive wheel lines zoom in; the view follows smoothly in `update`. */
  zoom(lines: number): void {
    if (Number.isFinite(lines)) {
      this.#targetDistance = Math.min(MAX_DISTANCE_METRES, Math.max(MIN_DISTANCE_METRES, this.#targetDistance * ZOOM_STEP ** lines));
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
    const target: Vec3 = [focus[0], focus[1] + PIVOT_HEIGHT_METRES, focus[2]];
    const horizontal = this.#distance * Math.cos(this.#pitch);
    const eye: Vec3 = [
      target[0] - Math.sin(this.#yaw) * horizontal,
      target[1] + this.#distance * Math.sin(this.#pitch),
      target[2] - Math.cos(this.#yaw) * horizontal,
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

/** Held movement as semantic axes: run/backpedal, strafe, and whether any movement is held. */
export type HeldIntent = { forward: Axis; strafe: Axis; steering: boolean };

export const IDLE_INTENT: HeldIntent = { forward: 0, strafe: 0, steering: false };

/**
 * What a mouse drag does: `orbit` (left button) looks around freely and the
 * character keeps its own heading, even while running; `turn` (right
 * button) turns the character with the view, even while standing.
 */
export type DragMode = "none" | "orbit" | "turn";

/**
 * Movement intent from the held movement actions under the orbit camera. Without a drag,
 * the character faces the camera's heading while any movement is held
 * and keeps `lastFacing` otherwise, so orbiting while idle never turns it.
 */
export function movementInput(
  held: HeldIntent,
  cameraFacing: number,
  lastFacing: number,
  jumps: number,
  drag: DragMode = "none",
): MovementInput {
  const facing = drag === "turn" ? cameraFacing : drag === "orbit" ? lastFacing : held.steering ? cameraFacing : lastFacing;
  return { forward: held.forward, strafe: held.strafe, facing, jumps };
}
