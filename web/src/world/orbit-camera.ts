import type { Axis } from "../command-wire";
import type { EntityRef } from "../entity-ref";
import { wasm } from "../wasm";
import type { MovementInput } from "./movement-outbox";

export type CameraMode = { kind: "free-look" } | { kind: "focused"; target: EntityRef };
export type PointerOwner = "camera" | "interface" | "gesture";

/** Pointer ownership is explicit; focus and held gestures supersede free mouse-look. */
export function pointerOwner(mode: CameraMode, captured: boolean, gesture: boolean): PointerOwner {
  if (gesture) { return "gesture"; }
  return mode.kind === "free-look" && captured ? "camera" : "interface";
}

/** Browser pixel scrolling: this many pixels count as one wheel line. */
export const PIXELS_PER_WHEEL_LINE = 40;

export type Vec3 = [number, number, number];
export type CameraView = { eye: Vec3; target: Vec3 };
/** An axis-aligned box in metres. */
export type CameraBox = { min: Vec3; max: Vec3 };

function viewOf(values: Float64Array): CameraView {
  return { eye: [values[0]!, values[1]!, values[2]!], target: [values[3]!, values[4]!, values[5]!] };
}

/**
 * The view with its eye pulled in toward the target until the sight line clears every box (a
 * camera boom, #552): inside the Battle Royale fortress the eye stays under roofs and in front
 * of walls instead of looking at their backs. Boxes that contain the target are ignored. The rule
 * is the shared camera's (`mmorpg-client-model`, #1058).
 */
export function clearSightLine(view: CameraView, boxes: readonly CameraBox[]): CameraView {
  const flat = new Float64Array(boxes.length * 6);
  boxes.forEach((box, index) => { flat.set(box.min, index * 6); flat.set(box.max, index * 6 + 3); });
  const values = Float64Array.from([...view.eye, ...view.target]);
  wasm.clearSightLine(values, flat);
  return viewOf(values);
}

/**
 * The browser's adapter of the shared third-person shoulder camera (`mmorpg-client-model`'s
 * `camera`, #1058, which the native client adapts too): look and zoom limits, smooth zoom, enemy
 * focus following and the eye's ground floor are its rules; this only forwards input and reads the
 * view. Angles are radians (0 toward +Z, increasing toward +X), lengths metres.
 */
export class OrbitCamera {
  #camera: wasm.OrbitCamera | null = null;
  #invertVerticalLook = false;
  readonly #view = new Float64Array(6);
  readonly #focus = new Float64Array(3);
  readonly #opponent = new Float64Array(3);

  /** Frees the WASM camera; this camera is not used afterwards. */
  dispose(): void {
    this.#camera?.free();
    this.#camera = null;
  }

  /** The shared camera, created on first use: the page builds its view before WASM loads. */
  get #rules(): wasm.OrbitCamera {
    if (!this.#camera) {
      this.#camera = new wasm.OrbitCamera();
      this.#camera.setLook(1, this.#invertVerticalLook);
    }
    return this.#camera;
  }

  /** The distance the view uses now; it approaches `targetDistance` in `update`. */
  get distance(): number {
    return this.#rules.distance;
  }

  get targetDistance(): number {
    return this.#rules.targetDistance;
  }

  setLookInversion(invert: boolean): void {
    this.#invertVerticalLook = invert;
    this.#camera?.setLook(1, invert);
  }

  /** Mouse drag: moving right turns the view right; moving down raises the eye. */
  orbit(dxPixels: number, dyPixels: number): void {
    this.#rules.orbit(dxPixels, dyPixels);
  }

  /** Positive wheel lines zoom in; the view follows smoothly in `update`. */
  zoom(lines: number): void {
    this.#rules.zoom(lines);
  }

  /** A pinch: spreading the fingers by `scale` brings the camera that much closer. */
  zoomBy(scale: number): void {
    this.#rules.zoomBy(scale);
  }

  /** Advances smooth zoom; `instant` (reduced motion) jumps to the target. */
  update(deltaSeconds: number, instant = false): void {
    this.#rules.update(deltaSeconds, instant);
  }

  /** Follow a projected opponent without snapping when focus is acquired, switched or cleared. */
  track(self: Vec3, opponent: Vec3 | null, deltaSeconds: number): void {
    this.#focus.set(self);
    if (opponent) { this.#opponent.set(opponent); }
    this.#rules.track(this.#focus, opponent ? this.#opponent : NO_POINT, deltaSeconds);
  }

  /** Turns the view to a heading in radians (0 toward +Z, increasing toward +X). */
  lookAlong(heading: number): void {
    this.#rules.lookAlong(heading);
  }

  /** The view's heading in radians: 0 toward +Z, increasing toward +X. */
  get heading(): number {
    // Before its first input (and before WASM loads) the camera looks along its starting heading.
    return this.#camera?.heading ?? 0;
  }

  /** The heading a character takes while moving under this camera. */
  facing(): number {
    return this.#camera?.facing() ?? 0;
  }

  /**
   * Eye and look-at point for a focus at the character's body centre, in metres. With
   * `groundAt`, the eye is raised to stay above the ground at its own position; nothing else
   * collides.
   */
  view(focus: Vec3, groundAt?: (x: number, z: number) => number): CameraView {
    const values = this.#view;
    this.#rules.view(focus[0], focus[1], focus[2], values);
    if (groundAt) { wasm.raiseEye(values, groundAt(values[0]!, values[2]!)); }
    return viewOf(values);
  }
}

const NO_POINT = new Float64Array(0);

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
