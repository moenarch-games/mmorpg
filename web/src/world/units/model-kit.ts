import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { shadeColor } from "../environment";
import type { UnitPlacement } from "../humanoid";
import type { Color } from "../scenery";

/**
 * Shared pieces of the procedural creature and NPC models: joint frames for
 * forward kinematics, primitive parts and their renderer nodes, the corpse
 * pose and the ring under the viewer's target. Models are built in metres in
 * their own frame (feet at the origin, +Y up, +Z forward, +X left) and carry
 * no state, so one call per unit per frame yields every node.
 */
export type Vec3 = [number, number, number];
export type Quaternion = [number, number, number, number];

export const IDENTITY_Q: Quaternion = [0, 0, 0, 1];
export const CORPSE_SHADE = 0.45;
export const TARGET_RING: Color = "#f2d15f";

export function qMul(a: Quaternion, b: Quaternion): Quaternion {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

/** Rotation by `angle` about the X (0), Y (1) or Z (2) axis. */
export function qAxis(axis: 0 | 1 | 2, angle: number): Quaternion {
  const q: Quaternion = [0, 0, 0, Math.cos(angle / 2)];
  q[axis] = Math.sin(angle / 2);
  return q;
}

export function qRotate([x, y, z, w]: Quaternion, [vx, vy, vz]: Vec3): Vec3 {
  const cx = y * vz - z * vy;
  const cy = z * vx - x * vz;
  const cz = x * vy - y * vx;
  return [
    vx + 2 * (w * cx + y * cz - z * cy),
    vy + 2 * (w * cy + z * cx - x * cz),
    vz + 2 * (w * cz + x * cy - y * cx),
  ];
}

/** A joint frame in world space. */
export type Frame = { p: Vec3; q: Quaternion };

export function child(parent: Frame, offset: Vec3, rotation: Quaternion = IDENTITY_Q): Frame {
  const [ox, oy, oz] = qRotate(parent.q, offset);
  return { p: [parent.p[0] + ox, parent.p[1] + oy, parent.p[2] + oz], q: qMul(parent.q, rotation) };
}

/** Yaw about Y, then pitch about X (positive tips +Y toward +Z), then roll about Z, all local. */
export function tilt(pitch: number, roll = 0, yaw = 0): Quaternion {
  return qMul(qMul(qAxis(1, yaw), qAxis(0, pitch)), qAxis(2, roll));
}

export type Primitive =
  | { kind: "box"; size: Vec3 }
  | { kind: "sphere"; radius: number }
  | { kind: "cylinder"; radius: number; height: number };

export type Part = { name: string; primitive: Primitive; color: Color; frame: Frame };

export const box = (width: number, height: number, depth: number): Primitive => ({ kind: "box", size: [width, height, depth] });
export const ball = (radius: number): Primitive => ({ kind: "sphere", radius });
export const post = (radius: number, height: number): Primitive => ({ kind: "cylinder", radius, height });

export function addPart(parts: Part[], name: string, primitive: Primitive, color: Color, frame: Frame): void {
  parts.push({ name, primitive, color, frame });
}

/** Renderer nodes with stable IDs `${id}-${part}`, corpse-shaded when `dead`. */
export function partNodes(id: string, parts: readonly Part[], dead: boolean): RendererSceneNode[] {
  return parts.map(({ name, primitive, color, frame }) => ({
    id: `${id}-${name}`,
    geometry: primitive,
    color: dead ? shadeColor(color, CORPSE_SHADE) : color,
    transform: { translation: frame.p, rotationQuaternion: frame.q },
  }));
}

/**
 * The root frame of a unit: standing at its feet facing its yaw, or, when
 * `lay` is given, lying on the ground. The body turns about its centre
 * (`pivotY` above the feet) and ends up `lift` metres above the ground (the height its centre rests at).
 */
export function rootFrame(placement: UnitPlacement, pivotY: number, lay: { rotation: Quaternion; lift: number } | null): Frame {
  const base: Frame = { p: [placement.x, placement.feetY, placement.z], q: qAxis(1, placement.yawRadians) };
  if (!lay) {
    return base;
  }
  const [px, py, pz] = qRotate(lay.rotation, [0, pivotY, 0]);
  return child(base, [-px, lay.lift - py, -pz], lay.rotation);
}

/** The ring marking the viewer's target, at the unit's feet. */
export function targetRingNode(id: string, placement: UnitPlacement, halfFootprintMetres: number): RendererSceneNode {
  return {
    id: `${id}-target-ring`,
    geometry: { kind: "cylinder", radius: halfFootprintMetres + 0.35, height: 0.04 },
    color: TARGET_RING,
    unlit: true,
    opacity: 0.85,
    transform: { translation: [placement.x, placement.feetY + 0.02, placement.z] },
  };
}
