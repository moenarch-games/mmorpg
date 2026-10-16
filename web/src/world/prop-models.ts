import type { EnvironmentStyle } from "./environment";
import { mixColor, shadeColor } from "./environment";
import type { CullClass, SceneBatcher } from "./mesh-batching";
import {
  BLADE,
  BOX,
  GABLE,
  IDENTITY,
  MeshBuilder,
  PYRAMID,
  blob,
  compose,
  disc,
  frustum,
  hash01,
  rotateX,
  rotateY,
  rotateZ,
  scale,
  translate,
  type Affine,
  type Shape,
  type Vec3,
} from "./mesh-builder";
import type { Color, Prop, PropKind } from "./scenery";

/**
 * Procedural low-poly models for every scenery prop kind, appended into the
 * static batches. A model is built in the prop's local frame (its anchor on
 * the drawn terrain at the origin, local +Z turned by the prop's yaw) from the
 * prop's body box, so
 * a structure's walls are exactly its core collider; roofs, towers and
 * canopies rise above it. Parts that move (windmill sails, the campfire
 * flame, swaying reeds) go to `AnimatedParts` instead of the batches.
 */
export type ModelContext = {
  batcher: SceneBatcher;
  style: EnvironmentStyle;
  unitsPerMetre: number;
  /** Rendered terrain height in metres at `(x, z)` metres. */
  surface(x: number, z: number): number;
  /** The XZ point (metres) a building's door should face. */
  doorTarget(prop: Prop): readonly [number, number];
  /** Gate posts pair up across their opening; returns the partner's anchor in metres. */
  gatePartner(prop: Prop): readonly [number, number] | null;
  animated: AnimatedParts;
};

/** A dynamic part: geometry in its own frame, placed and animated every frame. */
export type AnimatedMesh = {
  id: string;
  color: Color;
  positions: Vec3[];
  normals: Vec3[];
  indices: number[];
};

export type AnimatedParts = {
  /** Sails turn about their hub; `axisYaw` is the direction the hub faces. */
  sails: { mesh: AnimatedMesh[]; hub: Vec3; axisYaw: number }[];
  flames: { mesh: AnimatedMesh[]; base: Vec3; seed: number }[];
  /** Reeds sway by a shear about y = `groundY`, one group per phase. */
  reeds: { mesh: AnimatedMesh[]; groundY: number; phase: number }[];
};

type Put = (color: Color, shape: Shape, local: Affine, cull?: CullClass) => void;

const YAW_STEPS = 65_536;

function yawRadians(yaw: number): number {
  return (yaw / YAW_STEPS) * 2 * Math.PI;
}

/** A local transform placing a unit shape: translate, then optional rotation, then scale. */
function at(x: number, y: number, z: number, sx: number, sy: number, sz: number, rotation: Affine = IDENTITY): Affine {
  return compose(translate(x, y, z), rotation, scale(sx, sy, sz));
}

/** A rotation that maps local +Y onto `direction`. */
function alignY([dx, dy, dz]: Vec3): Affine {
  const length = Math.hypot(dx, dy, dz);
  if (length === 0) {
    return IDENTITY;
  }
  const [x, y, z] = [dx / length, dy / length, dz / length];
  // Rotate +Y to (x, y, z): about the axis (z, 0, -x) by acos(y).
  const axisLength = Math.hypot(x, z);
  if (axisLength < 1e-9) {
    return y > 0 ? IDENTITY : rotateX(Math.PI);
  }
  const ax = z / axisLength;
  const az = -x / axisLength;
  const c = y;
  const s = axisLength;
  const t = 1 - c;
  return {
    m: [
      t * ax * ax + c, -s * az, t * ax * az,
      s * az, c, -s * ax,
      t * ax * az, s * ax, t * az * az + c,
    ],
    t: [0, 0, 0],
  };
}

/** A cylinder-like shape from `a` to `b` (local metres). */
function beam(put: Put, color: Color, a: Vec3, b: Vec3, radius: number, sides = 6, cull?: CullClass): void {
  const direction: Vec3 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  const length = Math.hypot(...direction);
  put(color, frustum(sides, 1, { smooth: true }), compose(translate(...a), alignY(direction), scale(radius, length, radius)), cull);
}

function box(put: Put, color: Color, center: Vec3, half: Vec3, rotation: Affine = IDENTITY, cull?: CullClass): void {
  put(color, BOX, at(center[0], center[1], center[2], half[0], half[1], half[2], rotation), cull);
}

/**
 * The world transform of a prop: its anchor on the drawn terrain, then yaw.
 * The exported feet height follows the full-resolution relief, which the
 * 4 m terrain mesh only approximates (by up to about 0.4 m on woodland
 * relief), so props stand on what is drawn instead.
 */
function propFrame(context: ModelContext, prop: Prop, extraYaw = 0): Affine {
  const x = prop.position[0] / context.unitsPerMetre;
  const z = prop.position[2] / context.unitsPerMetre;
  return compose(translate(x, context.surface(x, z), z), rotateY(yawRadians(prop.yaw) + extraYaw));
}

function putter(context: ModelContext, prop: Prop, frame: Affine, defaultCull: CullClass): Put {
  const x = prop.position[0] / context.unitsPerMetre;
  const z = prop.position[2] / context.unitsPerMetre;
  const instanced = prop.kind.startsWith("tree-") || prop.kind.startsWith("rock-") || prop.kind === "grass-tuft";
  return (color, shape, local, cull) => {
    if (instanced) {
      context.batcher.instance(color, shape, compose(frame, local), x, z, cull ?? defaultCull);
      return;
    }
    context.batcher.builder(color, x, z, cull ?? defaultCull).add(shape, compose(frame, local));
  };
}

/** Half extents of the body box in metres. */
function half(prop: Prop, unitsPerMetre: number): Vec3 {
  return [prop.halfExtents[0] / unitsPerMetre, prop.halfExtents[1] / unitsPerMetre, prop.halfExtents[2] / unitsPerMetre];
}

function seedOf(prop: Prop): number {
  return hash01(prop.position[0], prop.position[2], prop.yaw);
}

// ---------------------------------------------------------------------------
// Buildings

/**
 * A building in its door frame: the door is on local +Z, `w` is the half
 * width along local X and `d` the half depth along local Z.
 */
type BuildingFrame = { put: Put; w: number; d: number; h: number };

function buildingFrame(context: ModelContext, prop: Prop): BuildingFrame {
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  const [tx, tz] = context.doorTarget(prop);
  const x = prop.position[0] / context.unitsPerMetre;
  const z = prop.position[2] / context.unitsPerMetre;
  const dx = tx - x;
  const dz = tz - z;
  // Door on the face whose outward axis points most toward the target: +Z, +X, -Z or -X.
  const quarter = Math.abs(dx) > Math.abs(dz) ? (dx > 0 ? 1 : 3) : (dz >= 0 ? 0 : 2);
  const turned = quarter % 2 === 1;
  const frame = propFrame(context, prop, (quarter * Math.PI) / 2);
  return { put: putter(context, prop, frame, "always"), w: turned ? hz : hx, d: turned ? hx : hz, h: hy * 2 };
}

/** A gable roof over the rectangle ±w × ±d from `base` up by `rise`, ridge along the longer side. */
function roof(put: Put, color: Color, w: number, d: number, base: number, rise: number, overhang: number): void {
  const alongX = w >= d;
  const rotation = alongX ? IDENTITY : rotateY(Math.PI / 2);
  const [rw, rd] = alongX ? [w, d] : [d, w];
  put(color, GABLE, compose(translate(0, base, 0), rotation, scale(rw + overhang, rise, rd + overhang)));
  // A thin darker ridge beam.
  put(shadeColor(color, 0.72), BOX, compose(translate(0, base + rise, 0), rotation, scale(rw + overhang + 0.05, 0.09, 0.12)));
}

/** Windows along a wall of half length `span` at local z = `face` (door side +Z when `face` > 0). */
function windows(put: Put, color: Color, frameColor: Color, span: number, face: number, y: number, spacing: number, skipMiddle: boolean): void {
  const count = Math.max(1, Math.floor((span * 2) / spacing));
  for (let index = 0; index < count; index += 1) {
    const offset = -span + spacing / 2 + index * ((span * 2 - spacing) / Math.max(1, count - 1 || 1));
    const x = count === 1 ? 0 : offset;
    if (skipMiddle && Math.abs(x) < 1.2) {
      continue;
    }
    const z = face + Math.sign(face) * 0.04;
    box(put, frameColor, [x, y, z], [0.46, 0.52, 0.05]);
    box(put, color, [x, y, z + Math.sign(face) * 0.02], [0.34, 0.4, 0.05]);
  }
}

/** Windows on the side walls (local ±X), rotated a quarter turn. */
function sideWindows(put: Put, color: Color, frameColor: Color, w: number, d: number, y: number, spacing: number): void {
  for (const side of [1, -1]) {
    const turned: Put = (c, shape, local, cull) => put(c, shape, compose(rotateY((side * Math.PI) / 2), local), cull);
    windows(turned, color, frameColor, d, w, y, spacing, false);
  }
}

function door(put: Put, palette: EnvironmentStyle["palette"], d: number, width: number, height: number): void {
  box(put, palette.timber, [0, height / 2 + 0.05, d + 0.05], [width / 2 + 0.14, height / 2 + 0.1, 0.06]);
  box(put, palette.door, [0, height / 2, d + 0.09], [width / 2, height / 2, 0.06]);
}

function chimney(put: Put, color: Color, x: number, z: number, base: number, top: number): void {
  box(put, color, [x, (base + top) / 2, z], [0.42, (top - base) / 2, 0.42]);
  box(put, shadeColor(color, 0.75), [x, top + 0.08, z], [0.5, 0.1, 0.5]);
}

/** Timber framing on plaster walls: corner posts, a floor beam and an eave beam. */
function timberFrame(put: Put, color: Color, w: number, d: number, from: number, to: number): void {
  for (const sx of [-1, 1]) {
    for (const sz of [-1, 1]) {
      box(put, color, [sx * (w - 0.08), (from + to) / 2, sz * (d - 0.08)], [0.16, (to - from) / 2, 0.16]);
    }
  }
  for (const y of [from + 0.1, to - 0.12]) {
    box(put, color, [0, y, d + 0.02], [w, 0.1, 0.04]);
    box(put, color, [0, y, -d - 0.02], [w, 0.1, 0.04]);
    box(put, color, [w + 0.02, y, 0], [0.04, 0.1, d]);
    box(put, color, [-w - 0.02, y, 0], [0.04, 0.1, d]);
  }
}

function house(context: ModelContext, prop: Prop, roofColor: Color, lit: boolean): void {
  const { palette } = context.style;
  const { put, w, d, h } = buildingFrame(context, prop);
  const skirting = Math.min(0.9, h * 0.18);
  box(put, palette.stone, [0, skirting / 2, 0], [w, skirting / 2, d]);
  box(put, palette.plaster, [0, (skirting + h) / 2, 0], [w, (h - skirting) / 2, d]);
  timberFrame(put, palette.timber, w, d, skirting, h);
  const rise = Math.min(w, d) * 0.95;
  roof(put, roofColor, w, d, h, rise, 0.55);
  chimney(put, palette.stoneDark, w * 0.55, -d * 0.35, h * 0.6, h + rise * 0.85);
  door(put, palette, d, 1.2, 2.2);
  const glass = lit ? palette.windowLit : palette.window;
  windows(put, glass, palette.timber, w, d, h * 0.58, 2.6, true);
  windows(put, glass, palette.timber, w, -d, h * 0.58, 2.6, false);
  sideWindows(put, glass, palette.timber, w, d, h * 0.58, 3.2);
}

function inn(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  house(context, prop, palette.roofRed, true);
  const { put, w, d } = buildingFrame(context, prop);
  // A hanging inn sign on a bracket beside the door.
  const x = Math.min(w - 0.6, 2.4);
  box(put, palette.timber, [x, 3.1, d + 0.6], [0.06, 0.06, 0.6]);
  box(put, palette.woodLight, [x, 2.55, d + 1.0], [0.55, 0.38, 0.05]);
  box(put, palette.lampGlow, [x, 2.55, d + 1.06], [0.2, 0.2, 0.03]);
  box(put, palette.iron, [x, 2.98, d + 1.0], [0.02, 0.06, 0.02]);
  // Barrels by the door.
  for (const offset of [-2.2, -2.9]) {
    put(palette.woodDark, frustum(8, 1, { smooth: true }), at(offset, 0, d + 0.6, 0.32, 0.9, 0.32));
  }
}

function smithy(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const { put, w, d, h } = buildingFrame(context, prop);
  box(put, palette.stoneDark, [0, h / 2, 0], [w, h / 2, d]);
  box(put, palette.stone, [0, 0.3, 0], [w + 0.02, 0.3, d + 0.02]);
  const rise = Math.min(w, d) * 0.8;
  roof(put, palette.roofSlate, w, d, h, rise, 0.5);
  chimney(put, palette.stone, -w * 0.5, -d * 0.4, h * 0.5, h + rise + 1.4);
  door(put, palette, d, 1.8, 2.6);
  box(put, palette.fire, [0, 1.3, d + 0.12], [0.7, 0.35, 0.04]);
  // Anvil on a stump in front of the forge.
  put(palette.woodDark, frustum(7, 1, { smooth: true }), at(w * 0.55, 0, d + 1.3, 0.32, 0.55, 0.32));
  box(put, palette.iron, [w * 0.55, 0.68, d + 1.3], [0.34, 0.12, 0.16]);
  box(put, palette.iron, [w * 0.55 + 0.4, 0.72, d + 1.3], [0.12, 0.07, 0.1]);
}

function barn(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const { put, w, d, h } = buildingFrame(context, prop);
  box(put, palette.barnRed, [0, h / 2, 0], [w, h / 2, d]);
  for (const sx of [-1, 1]) {
    for (const sz of [-1, 1]) {
      box(put, palette.trim, [sx * (w - 0.1), h / 2, sz * (d - 0.1)], [0.14, h / 2, 0.14]);
    }
  }
  const rise = Math.min(w, d) * 0.85;
  roof(put, palette.roofWood, w, d, h, rise, 0.6);
  // Double door with the classic white cross, and a hay loft above.
  const doorHalf = Math.min(2.2, w * 0.4);
  box(put, palette.trim, [0, 2.1, d + 0.04], [doorHalf + 0.15, 2.2, 0.04]);
  box(put, palette.barnRed, [0, 2.0, d + 0.08], [doorHalf, 2.0, 0.04]);
  for (const sign of [1, -1]) {
    const angle = Math.atan2(4.0, doorHalf * 2) * sign;
    box(put, palette.trim, [0, 2.0, d + 0.12], [Math.hypot(doorHalf, 2.0), 0.1, 0.03], rotateZ(angle));
  }
  box(put, palette.trim, [0, h - 0.9, d + 0.05], [0.8, 0.7, 0.04]);
  box(put, palette.window, [0, h - 0.9, d + 0.08], [0.6, 0.5, 0.04]);
  // Hay bales stacked by the wall.
  box(put, palette.crop, [w - 1.2, 0.45, d + 1.0], [0.8, 0.45, 0.55]);
  box(put, palette.crop, [w - 1.9, 0.45, d + 1.1], [0.8, 0.45, 0.55]);
  box(put, shadeColor(palette.crop, 0.9), [w - 1.55, 1.3, d + 1.05], [0.8, 0.4, 0.55]);
}

function keep(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const { put, w, d, h } = buildingFrame(context, prop);
  box(put, palette.stone, [0, h / 2, 0], [w, h / 2, d]);
  box(put, palette.stoneDark, [0, 0.35, 0], [w + 0.08, 0.35, d + 0.08]);
  // String course and parapet.
  box(put, palette.stoneDark, [0, h * 0.62, 0], [w + 0.12, 0.14, d + 0.12]);
  box(put, palette.stoneDark, [0, h + 0.25, 0], [w + 0.3, 0.25, d + 0.3]);
  const merlon = (x: number, z: number) => box(put, palette.stone, [x, h + 0.95, z], [0.42, 0.45, 0.42]);
  for (let x = -w + 0.6; x <= w - 0.5; x += 1.6) {
    merlon(x, d + 0.02);
    merlon(x, -d - 0.02);
  }
  for (let z = -d + 0.6; z <= d - 0.5; z += 1.6) {
    merlon(w + 0.02, z);
    merlon(-w - 0.02, z);
  }
  // Four corner towers, flush with the walls, with slate cones and a banner pole.
  const radius = 2.3;
  const towerTop = h + 3.2;
  for (const sx of [-1, 1]) {
    for (const sz of [-1, 1]) {
      const x = sx * (w - radius);
      const z = sz * (d - radius);
      put(palette.stoneLight, frustum(8, 1, { offset: 0.5 }), at(x, 0, z, radius, towerTop, radius));
      put(palette.stoneDark, frustum(8, 1, { offset: 0.5 }), at(x, towerTop, z, radius + 0.3, 0.4, radius + 0.3));
      put(palette.roofSlate, frustum(8, 0, { offset: 0.5 }), at(x, towerTop + 0.4, z, radius + 0.35, 3.6, radius + 0.35));
      box(put, palette.window, [x + sx * 0.3, towerTop - 1.6, z + sz * (radius - 0.1)], [0.16, 0.5, 0.2]);
    }
  }
  const poleX = w - radius;
  const poleZ = d - radius;
  box(put, palette.iron, [poleX, towerTop + 4.9, poleZ], [0.05, 1.6, 0.05]);
  box(put, palette.banner, [poleX + 0.75, towerTop + 5.8, poleZ], [0.7, 0.42, 0.03]);
  // Gate with a stone frame, banners either side and arrow slits.
  box(put, palette.stoneLight, [0, 2.3, d + 0.08], [2.1, 2.3, 0.1]);
  box(put, palette.door, [0, 2.0, d + 0.14], [1.6, 2.0, 0.08]);
  box(put, palette.iron, [0, 2.0, d + 0.2], [1.6, 0.08, 0.04]);
  for (const sx of [-1, 1]) {
    box(put, palette.banner, [sx * 3.6, 5.2, d + 0.06], [0.75, 1.8, 0.04]);
    box(put, palette.lampGlow, [sx * 3.6, 5.6, d + 0.1], [0.28, 0.28, 0.02]);
    put(palette.banner, PYRAMID, compose(translate(sx * 3.6, 3.4, d + 0.06), rotateX(Math.PI), scale(0.75, 0.45, 0.04)));
  }
  for (const x of [-w * 0.55, w * 0.55]) {
    box(put, palette.window, [x, h * 0.8, d + 0.05], [0.14, 0.6, 0.05]);
    box(put, palette.window, [x, h * 0.8, -d - 0.05], [0.14, 0.6, 0.05]);
  }
}

function windmill(context: ModelContext, prop: Prop, index: number): void {
  const { palette } = context.style;
  const { put, w, d, h } = buildingFrame(context, prop);
  const radius = Math.min(w, d);
  put(palette.stoneLight, frustum(8, 0.78, { offset: 0.5 }), at(0, 0, 0, radius, h, radius));
  put(palette.stoneDark, frustum(8, 1, { offset: 0.5 }), at(0, 0, 0, radius + 0.04, 0.5, radius + 0.04));
  const top = radius * 0.78;
  put(palette.roofWood, frustum(8, 0, { offset: 0.5 }), at(0, h, 0, top + 0.35, 2.6, top + 0.35));
  door(put, palette, radius * 0.92, 1.0, 2.0);
  box(put, palette.window, [0, h * 0.6, radius * 0.86], [0.3, 0.4, 0.06]);
  // The sails face the door side; the hub sits proud of the cap.
  const frame = propFrame(context, prop);
  const doorFrame = buildingDoorYaw(context, prop);
  const hubLocal: Vec3 = [0, h + 0.9, top + 0.9];
  const cos = Math.cos(doorFrame);
  const sin = Math.sin(doorFrame);
  const hubWorld: Vec3 = [
    frame.t[0] + hubLocal[0] * cos + hubLocal[2] * sin,
    frame.t[1] + hubLocal[1],
    frame.t[2] - hubLocal[0] * sin + hubLocal[2] * cos,
  ];
  box(put, palette.woodDark, [0, h + 0.9, top + 0.45], [0.18, 0.18, 0.5]);
  context.animated.sails.push({ mesh: sailMeshes(context, index), hub: hubWorld, axisYaw: doorFrame });
}

/** The yaw (radians) of a building's door frame, matching `buildingFrame`. */
function buildingDoorYaw(context: ModelContext, prop: Prop): number {
  const [tx, tz] = context.doorTarget(prop);
  const dx = tx - prop.position[0] / context.unitsPerMetre;
  const dz = tz - prop.position[2] / context.unitsPerMetre;
  const quarter = Math.abs(dx) > Math.abs(dz) ? (dx > 0 ? 1 : 3) : (dz >= 0 ? 0 : 2);
  return yawRadians(prop.yaw) + (quarter * Math.PI) / 2;
}

/** Four sails in the hub's frame: the axis is local +Z, sails in the XY plane. */
function sailMeshes(context: ModelContext, index: number): AnimatedMesh[] {
  const { palette } = context.style;
  const builders = new Map<Color, MeshBuilder>();
  const put: Put = (color, shape, local) => {
    let builder = builders.get(color);
    if (!builder) {
      builder = new MeshBuilder();
      builders.set(color, builder);
    }
    builder.add(shape, local);
  };
  box(put, palette.woodDark, [0, 0, 0], [0.32, 0.32, 0.22]);
  for (let arm = 0; arm < 4; arm += 1) {
    const rotation = rotateZ((arm * Math.PI) / 2);
    put(palette.wood, BOX, compose(rotation, at(0, 3.1, 0.05, 0.1, 3.1, 0.1)));
    put(palette.canvas, BOX, compose(rotation, at(0.62, 3.6, 0.08, 0.5, 2.3, 0.03)));
    for (const y of [1.6, 2.8, 4.0, 5.2]) {
      put(palette.woodLight, BOX, compose(rotation, at(0.62, y, 0.12, 0.56, 0.04, 0.03)));
    }
  }
  return [...builders.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([color, builder], part) => ({ id: `windmill-${index}-sails-${part}`, color, ...builder.data() }));
}

function well(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "always");
  const [hx, , hz] = half(prop, context.unitsPerMetre);
  const radius = Math.min(hx, hz);
  put(palette.stone, frustum(10, 1, { offset: 0.5 }), at(0, 0, 0, radius, 0.95, radius));
  put(palette.stoneDark, frustum(10, 1, { offset: 0.5 }), at(0, 0.95, 0, radius + 0.05, 0.12, radius + 0.05));
  put("#23384a", disc(10), at(0, 1.08, 0, radius - 0.18, 1, radius - 0.18));
  for (const sx of [-1, 1]) {
    box(put, palette.wood, [sx * (radius - 0.1), 1.6, 0], [0.09, 1.6, 0.09]);
  }
  box(put, palette.woodDark, [0, 2.2, 0], [radius + 0.1, 0.06, 0.06]);
  box(put, palette.iron, [0, 1.75, 0], [0.12, 0.16, 0.12]);
  roof(put, palette.roofRed, radius + 0.1, radius * 0.75, 3.1, 0.9, 0.25);
}

function waystone(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "always");
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  const height = hy * 2;
  box(put, palette.stoneDark, [0, 0.12, 0], [hx + 0.05, 0.12, hz + 0.05]);
  put(palette.waystone, frustum(4, 0.7, { offset: 0.5 }), at(0, 0.24, 0, hx * 1.25, height - 0.6, hz * 1.25));
  put(palette.waystone, frustum(4, 0, { offset: 0.5 }), at(0, height - 0.36, 0, hx * 0.88, 0.5, hz * 0.88));
  for (const [x, z, rotation] of [[0, 1, 0], [0, -1, Math.PI], [1, 0, Math.PI / 2], [-1, 0, -Math.PI / 2]] as const) {
    box(put, palette.rune, [x * (hx * 1.02), height * 0.55, z * (hz * 1.02)], [0.12, 0.28, 0.03], rotateY(rotation));
  }
}

function gravestone(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  const top = hy * 2 - hx;
  box(put, palette.stoneDark, [0, top / 2, 0], [hx, top / 2, hz]);
  put(palette.stoneDark, frustum(8, 1, { smooth: true }), compose(translate(0, top, -hz), rotateX(Math.PI / 2), scale(hx, hz * 2, hx)));
  box(put, palette.soil, [0, 0.06, hz + 0.55], [hx * 0.9, 0.08, 0.5]);
}

// ---------------------------------------------------------------------------
// Walls, posts and camp dressing

function palisade(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "always");
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  const alongX = hx >= hz;
  const length = (alongX ? hx : hz) * 2;
  const thickness = alongX ? hz : hx;
  const radius = thickness;
  const count = Math.max(1, Math.round(length / (radius * 2)));
  const spacing = length / count;
  for (let log = 0; log < count; log += 1) {
    const offset = -length / 2 + spacing * (log + 0.5);
    const [x, z] = alongX ? [offset, 0] : [0, offset];
    const jitter = hash01(prop.position[0], prop.position[2], log);
    const height = hy * 2 - 0.25 + jitter * 0.4;
    const color = jitter > 0.5 ? palette.wood : palette.woodDark;
    put(color, frustum(6, 1, { smooth: true }), at(x, 0, z, radius, height, radius));
    put(color, frustum(6, 0, { smooth: true }), at(x, height, z, radius, 0.55, radius));
  }
  // Two lashing rails.
  for (const y of [0.9, hy * 2 - 0.7]) {
    box(put, palette.timber, [0, y, 0], alongX ? [hx, 0.08, thickness + 0.06] : [thickness + 0.06, 0.08, hz]);
  }
}

function gatePost(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "always");
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  const radius = Math.min(hx, hz);
  const height = hy * 2;
  put(palette.woodDark, frustum(7, 1, { smooth: true }), at(0, 0, 0, radius, height, radius));
  put(palette.woodDark, frustum(7, 0, { smooth: true }), at(0, height, 0, radius, 0.7, radius));
  box(put, palette.iron, [0, height * 0.45, 0], [radius + 0.03, 0.08, radius + 0.03]);
  const partner = context.gatePartner(prop);
  if (partner) {
    // One crossbeam per gate, drawn by the post with the smaller anchor.
    const x = prop.position[0] / context.unitsPerMetre;
    const z = prop.position[2] / context.unitsPerMetre;
    if (x < partner[0] - 1e-6 || (Math.abs(x - partner[0]) < 1e-6 && z < partner[1])) {
      const y = height - 0.35;
      beam(put, palette.timber, [0, y, 0], [partner[0] - x, y, partner[1] - z], 0.22, 6);
      const mid: Vec3 = [(partner[0] - x) / 2, y - 0.6, (partner[1] - z) / 2];
      box(put, palette.iron, [mid[0], y - 0.3, mid[2]], [0.03, 0.25, 0.03]);
      box(put, palette.lampGlow, mid, [0.16, 0.2, 0.16]);
    }
  }
}

function fence(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  const alongX = hx >= hz;
  const length = (alongX ? hx : hz) * 2;
  const posts = Math.max(2, Math.round(length / 2) + 1);
  for (let post = 0; post < posts; post += 1) {
    const offset = -length / 2 + (length * post) / (posts - 1);
    const [x, z] = alongX ? [offset, 0] : [0, offset];
    box(put, palette.woodDark, [x, hy, z], [0.09, hy, 0.09]);
  }
  for (const y of [hy * 0.8, hy * 1.6]) {
    box(put, palette.wood, [0, y, 0], alongX ? [hx, 0.05, 0.04] : [0.04, 0.05, hz]);
  }
}

function tent(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const { put, w, d, h } = buildingFrame(context, prop);
  // Ridge across the door so the opening sits in a gable end facing +Z.
  put(palette.canvas, GABLE, compose(rotateY(Math.PI / 2), scale(d, h, w)));
  put(palette.canvasDark, GABLE, compose(translate(0, h * 0.02, 0), rotateY(Math.PI / 2), scale(d + 0.02, h * 0.35, w + 0.02)));
  put(palette.ember, PYRAMID, compose(translate(0, 0, d + 0.02), scale(w * 0.35, h * 0.62, 0.02)));
  for (const z of [d, -d]) {
    box(put, palette.woodDark, [0, h / 2 + 0.15, z], [0.05, h / 2 + 0.15, 0.05]);
  }
}

function campfire(context: ModelContext, prop: Prop, index: number): void {
  const { palette } = context.style;
  const frame = propFrame(context, prop);
  const put = putter(context, prop, frame, "mid");
  const [hx, , hz] = half(prop, context.unitsPerMetre);
  const ring = Math.min(hx, hz) * 0.85;
  for (let stone = 0; stone < 9; stone += 1) {
    const angle = (stone / 9) * 2 * Math.PI;
    put(stone % 2 === 0 ? palette.stoneDark : palette.stone, blob(0, { smooth: false, jitter: 0.25, seed: stone }),
      at(Math.sin(angle) * ring, 0.05, Math.cos(angle) * ring, 0.16, 0.12, 0.16));
  }
  for (let log = 0; log < 3; log += 1) {
    const angle = (log / 3) * Math.PI;
    const dx = Math.sin(angle) * ring * 0.8;
    const dz = Math.cos(angle) * ring * 0.8;
    beam(put, palette.woodDark, [-dx, 0.05, -dz], [dx, 0.18, dz], 0.07, 5);
  }
  put(palette.ember, disc(8), at(0, 0.03, 0, ring * 0.7, 1, ring * 0.7));
  const flames = new MeshBuilder();
  const core = new MeshBuilder();
  flames.add(frustum(6, 0, { smooth: true }), at(0, 0, 0, 0.32, 0.9, 0.32));
  flames.add(frustum(5, 0, { smooth: true }), at(0.12, 0, 0.05, 0.18, 0.62, 0.18));
  core.add(frustum(5, 0, { smooth: true }), at(-0.04, 0, -0.03, 0.18, 0.55, 0.18));
  context.animated.flames.push({
    mesh: [
      { id: `campfire-${index}-flame`, color: palette.fire, ...flames.data() },
      { id: `campfire-${index}-core`, color: palette.fireCore, ...core.data() },
    ],
    base: [frame.t[0], frame.t[1] + 0.12, frame.t[2]],
    seed: index,
  });
}

function crate(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  box(put, palette.woodLight, [0, hy, 0], [hx, hy, hz]);
  for (const y of [hy * 0.45, hy * 1.55]) {
    box(put, palette.woodDark, [0, y, 0], [hx + 0.02, 0.05, hz + 0.02]);
  }
}

function barrel(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [hx, hy] = half(prop, context.unitsPerMetre);
  const height = hy * 2;
  put(palette.wood, frustum(9, 1.12, { smooth: true, caps: false }), at(0, 0, 0, hx * 0.88, height / 2, hx * 0.88));
  put(palette.wood, frustum(9, 1 / 1.12, { smooth: true }), at(0, height / 2, 0, hx * 0.98, height / 2, hx * 0.98));
  for (const y of [height * 0.18, height * 0.8]) {
    put(palette.iron, frustum(9, 1, { smooth: true, caps: false }), at(0, y, 0, hx * 0.96, 0.05, hx * 0.96));
  }
}

function cart(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [hx, , hz] = half(prop, context.unitsPerMetre);
  box(put, palette.wood, [0, 0.62, 0], [hx * 0.8, 0.08, hz]);
  box(put, palette.woodLight, [0, 0.9, hz - 0.04], [hx * 0.8, 0.22, 0.04]);
  box(put, palette.woodLight, [0, 0.9, -hz + 0.04], [hx * 0.8, 0.22, 0.04]);
  box(put, palette.woodLight, [hx * 0.8 - 0.04, 0.9, 0], [0.04, 0.22, hz]);
  for (const side of [1, -1]) {
    put(palette.woodDark, frustum(10, 1, { smooth: true }), compose(translate(hx * 0.25, 0.45, side * (hz + 0.08)), rotateX(Math.PI / 2), scale(0.45, 0.1, 0.45)));
    beam(put, palette.woodDark, [-hx * 0.8, 0.6, side * hz * 0.6], [-hx * 1.7, 0.08, side * hz * 0.45], 0.05, 5);
  }
  box(put, palette.crop, [0.1, 0.95, 0], [hx * 0.55, 0.2, hz * 0.7]);
}

function signpost(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [, hy] = half(prop, context.unitsPerMetre);
  const height = hy * 2;
  box(put, palette.wood, [0, height / 2, 0], [0.07, height / 2, 0.07]);
  for (const [y, turn, length] of [[height - 0.25, 0, 0.55], [height - 0.6, Math.PI * 0.6, 0.5]] as const) {
    const rotation = rotateY(turn);
    box(put, palette.woodLight, [0, y, 0], [length, 0.12, 0.03], compose(rotation, translate(length * 0.55, 0, 0)));
  }
}

function lamp(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [, hy] = half(prop, context.unitsPerMetre);
  const height = hy * 2;
  box(put, palette.stoneDark, [0, 0.15, 0], [0.18, 0.15, 0.18]);
  put(palette.iron, frustum(6, 0.7, { smooth: true }), at(0, 0, 0, 0.08, height, 0.08));
  box(put, palette.iron, [0.25, height - 0.05, 0], [0.3, 0.035, 0.035]);
  box(put, palette.lampGlow, [0.48, height - 0.42, 0], [0.13, 0.18, 0.13]);
  put(palette.iron, PYRAMID, at(0.48, height - 0.24, 0, 0.18, 0.18, 0.18));
}

function mineEntrance(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const { put, w, d, h } = buildingFrame(context, prop);
  box(put, "#141414", [0, h * 0.45, -d * 0.2], [w * 0.72, h * 0.45, d * 0.6]);
  for (const sx of [-1, 1]) {
    box(put, palette.wood, [sx * (w * 0.78), h / 2, d * 0.4], [0.28, h / 2, 0.28]);
    put(palette.rockDark, blob(0, { smooth: false, jitter: 0.3, seed: sx + 5 }), at(sx * (w + 0.4), 0.3, d * 0.5, 0.9, 0.7, 0.8));
  }
  box(put, palette.woodDark, [0, h - 0.25, d * 0.4], [w * 0.95, 0.3, 0.32]);
  box(put, palette.woodDark, [0, h * 0.72, d * 0.4], [w * 0.78, 0.12, 0.2]);
  // Rails leading out of the mine.
  for (const sx of [-0.45, 0.45]) {
    box(put, palette.iron, [sx, 0.05, d + 2.4], [0.04, 0.04, 2.8]);
  }
  for (let sleeper = 0; sleeper < 6; sleeper += 1) {
    box(put, palette.woodDark, [0, 0.02, d + 0.2 + sleeper * 0.9], [0.7, 0.04, 0.12]);
  }
}

function cliff(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "always");
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  const height = hy * 2;
  const seed = seedOf(prop);
  // Strata: three slabs that step in slightly, alternating tones.
  const layers = 3;
  for (let layer = 0; layer < layers; layer += 1) {
    const from = (height * layer) / layers;
    const to = (height * (layer + 1)) / layers;
    const inset = layer * 0.25;
    const color = layer % 2 === 0 ? palette.rock : palette.rockDark;
    box(put, color, [0, (from + to) / 2, 0], [hx - inset, (to - from) / 2, hz - inset]);
  }
  put(palette.moss, BOX, at(0, height + 0.08, 0, hx - 0.7, 0.1, hz - 0.7));
  // Boulders and crags break up the silhouette along the faces (above head
  // height, so walkers never meet rock the collider does not have) and the top.
  const area = hx * hz;
  const count = Math.min(26, Math.max(4, Math.round(area / 6)));
  for (let index = 0; index < count; index += 1) {
    const u = hash01(seed * 1e6, index, 1);
    const v = hash01(seed * 1e6, index, 2);
    const size = 0.9 + hash01(seed * 1e6, index, 3) * 1.6;
    const onTop = index % 3 === 0 || height < 3;
    const side = Math.floor(v * 4);
    const along = u * 2 - 1;
    const inset = size * 0.75;
    const [x, z] = onTop
      ? [along * (hx - size), (v * 2 - 1) * (hz - size)]
      : side === 0 ? [along * hx, hz - inset]
        : side === 1 ? [along * hx, -hz + inset]
          : side === 2 ? [hx - inset, along * hz] : [-hx + inset, along * hz];
    const y = onTop ? height + size * 0.2 : 2.2 + size + hash01(seed * 1e6, index, 4) * Math.max(0, height - 2.2 - size);
    put(index % 2 === 0 ? palette.rockDark : palette.rock, blob(0, { smooth: false, jitter: 0.28, seed: index % 5 }),
      at(x, y, z, size, size * (onTop ? 0.6 : 0.9), size));
  }
}

function dock(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [hx, , hz] = half(prop, context.unitsPerMetre);
  const deck = 0.34;
  const planks = Math.round(hx * 2 / 0.5);
  for (let plank = 0; plank < planks; plank += 1) {
    const x = -hx + 0.25 + plank * 0.5;
    box(put, plank % 2 === 0 ? palette.woodLight : palette.wood, [x, deck, 0], [0.24, 0.05, hz]);
  }
  for (const x of [-hx + 0.3, 0, hx - 0.3]) {
    for (const z of [hz - 0.1, -hz + 0.1]) {
      put(palette.woodDark, frustum(6, 1, { smooth: true }), at(x, -0.4, z, 0.1, deck + 0.75, 0.1));
    }
  }
}

function cropRow(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "near");
  const ridge = putter(context, prop, propFrame(context, prop), "far");
  const [hx, , hz] = half(prop, context.unitsPerMetre);
  box(ridge, shadeColor(palette.soil, 0.9), [0, 0.06, 0], [hx, 0.08, hz * 0.8]);
  const plants = Math.floor((hx * 2) / 0.7);
  for (let plant = 0; plant < plants; plant += 1) {
    const x = -hx + 0.35 + plant * 0.7;
    const jitter = hash01(prop.position[2], plant);
    const height = 0.55 + jitter * 0.25;
    put(palette.cropStem, BLADE, at(x, 0.08, 0, 0.12, height * 0.8, 0.12, rotateY(jitter * 6)));
    put(palette.crop, BLADE, at(x + 0.12, 0.08, 0.05, 0.13, height, 0.13, rotateY(jitter * 9 + 1)));
  }
  // A green band along the ridge reads as the crop's leaves from afar.
  box(ridge, mixColor(palette.cropStem, palette.crop, 0.5), [0, 0.3, 0], [hx - 0.2, 0.12, 0.16]);
}

// ---------------------------------------------------------------------------
// Vegetation and rocks

/** Trees beyond the walls are seen from afar only and use fewer vertices. */
function isBackground(prop: Prop): boolean {
  return prop.collider === null;
}

function trunk(put: Put, color: Color, radius: number, height: number, sides: number): void {
  put(color, frustum(sides, 0.62, { smooth: true, caps: false }), at(0, -0.3, 0, radius, height + 0.3, radius));
}

function tree(context: ModelContext, prop: Prop, variant: "oak" | "pine" | "birch"): void {
  const { palette } = context.style;
  const background = isBackground(prop);
  const put = putter(context, prop, propFrame(context, prop), background ? "always" : "far");
  const [hx, hy] = half(prop, context.unitsPerMetre);
  const seed = seedOf(prop);
  // Collider trunks are full height; decorative trees scale with their prop.
  const grow = background ? prop.scale : 0.85 + seed * 0.4;
  const radius = Math.max(0.18, hx);
  const detail: 0 | 1 = background ? 0 : 1;
  const variantSeed = Math.floor(seed * 4);
  switch (variant) {
    case "oak": {
      const trunkHeight = background ? hy * 1.1 : hy * 2 * 0.72;
      trunk(put, palette.trunk, radius, trunkHeight + 0.6, background ? 5 : 7);
      const top = trunkHeight;
      put(palette.oakLeaves, blob(detail, { jitter: 0.18, seed: variantSeed }), at(0, top + 1.5 * grow, 0, 2.5 * grow, 2.0 * grow, 2.5 * grow));
      if (!background) {
        put(palette.oakLeavesLight, blob(1, { jitter: 0.2, seed: variantSeed + 4 }),
          at(1.2 * grow, top + 1.0 * grow, 0.7 * grow, 1.6 * grow, 1.4 * grow, 1.6 * grow));
        put(palette.oakLeaves, blob(1, { jitter: 0.2, seed: variantSeed + 8 }),
          at(-1.0 * grow, top + 0.9 * grow, -0.9 * grow, 1.5 * grow, 1.3 * grow, 1.5 * grow));
        put(palette.oakLeavesLight, blob(0, { jitter: 0.15, seed: variantSeed + 12 }),
          at(-0.3 * grow, top + 2.9 * grow, 0.2 * grow, 1.3 * grow, 1.0 * grow, 1.3 * grow));
      }
      return;
    }
    case "pine": {
      const trunkHeight = background ? hy * 0.5 : hy * 2 * 0.3;
      trunk(put, palette.trunk, radius * 0.9, trunkHeight + 1.2 * grow, 5);
      const tiers = background ? 3 : 4;
      const sides = background ? 6 : 8;
      for (let tier = 0; tier < tiers; tier += 1) {
        const shrink = 1 - tier / (tiers + 0.6);
        const base = trunkHeight + tier * 1.35 * grow;
        const color = tier % 2 === 0 ? palette.pineNeedles : palette.pineNeedlesLight;
        put(color, frustum(sides, 0, { smooth: true, offset: tier * 0.37 }), at(0, base, 0, 2.2 * grow * shrink, 2.5 * grow, 2.2 * grow * shrink));
      }
      return;
    }
    case "birch": {
      const trunkHeight = background ? hy * 1.2 : hy * 2 * 0.78;
      trunk(put, palette.birchBark, radius * 0.75, trunkHeight + 0.8, 6);
      if (!background) {
        for (let mark = 0; mark < 4; mark += 1) {
          const y = 0.6 + mark * trunkHeight * 0.22;
          box(put, palette.ember, [0, y, radius * 0.62], [radius * 0.35, 0.05, 0.03], rotateY(mark * 1.7));
        }
      }
      put(palette.birchLeaves, blob(detail, { jitter: 0.2, seed: variantSeed + 16 }), at(0, trunkHeight + 1.6 * grow, 0, 1.6 * grow, 2.4 * grow, 1.6 * grow));
      if (!background) {
        put(palette.birchLeaves, blob(0, { jitter: 0.2, seed: variantSeed + 20 }), at(0.8 * grow, trunkHeight + 0.6 * grow, 0.3 * grow, 1.1 * grow, 1.4 * grow, 1.1 * grow));
      }
      return;
    }
  }
}

function bush(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "mid");
  const [hx, hy] = half(prop, context.unitsPerMetre);
  const seed = Math.floor(seedOf(prop) * 6);
  put(palette.bush, blob(0, { jitter: 0.22, seed: seed + 30 }), at(0, hy * 0.7, 0, hx, hy * 1.1, hx));
  put(palette.bushLight, blob(0, { jitter: 0.22, seed: seed + 40 }), at(hx * 0.55, hy * 0.55, hx * 0.3, hx * 0.7, hy * 0.85, hx * 0.7));
  put(palette.bush, blob(0, { jitter: 0.22, seed: seed + 50 }), at(-hx * 0.5, hy * 0.45, -hx * 0.35, hx * 0.62, hy * 0.75, hx * 0.62));
}

function rock(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), prop.collider ? "always" : "mid");
  const [hx, hy, hz] = half(prop, context.unitsPerMetre);
  const seed = seedOf(prop);
  const color = seed > 0.5 ? palette.rock : palette.rockDark;
  put(color, blob(0, { smooth: false, jitter: 0.25, seed: Math.floor(seed * 7) + 60 }), at(0, hy * 0.7, 0, hx, hy * 1.4, hz));
  if (prop.collider) {
    put(palette.moss, blob(0, { smooth: false, jitter: 0.2, seed: 70 }), at(hx * 0.2, hy * 1.75, 0, hx * 0.6, hy * 0.3, hz * 0.6));
  }
}

function grassTuft(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "near");
  const [hx, hy] = half(prop, context.unitsPerMetre);
  const seed = seedOf(prop);
  const color = seed > 0.5 ? palette.grass : palette.grassDark;
  // A small seeded family of whole tufts shares immutable unit geometry.
  // Authored dimensions and yaw live in the instance transform rather than baked vertices.
  const variant = Math.floor(seed * 4);
  const tuft = new MeshBuilder();
  const blades = 4;
  for (let blade = 0; blade < blades; blade += 1) {
    const angle = (blade / blades) * 2 * Math.PI + variant * 1.5;
    const lean = 0.2 + hash01(variant, blade) * 0.35;
    const height = 3.4 * (0.7 + hash01(variant, blade, 1) * 0.6);
    const spread = 0.3 + hash01(variant, blade, 2) * 0.5;
    tuft.add(BLADE, compose(translate(Math.sin(angle) * spread, 0, Math.cos(angle) * spread), rotateY(angle), rotateX(-lean), scale(0.34, height, 0.34)));
  }
  put(color, tuft.data(), compose(translate(0, -0.02, 0), scale(hx, hy, hx)));
}

function flowers(context: ModelContext, prop: Prop): void {
  const { palette } = context.style;
  const put = putter(context, prop, propFrame(context, prop), "near");
  const [hx, hy] = half(prop, context.unitsPerMetre);
  const color = palette.flowers[prop.yaw % palette.flowers.length]!;
  put(palette.grassDark, BLADE, at(0, -0.02, 0, hx * 0.5, hy * 1.4, hx * 0.5));
  for (let head = 0; head < 3; head += 1) {
    const angle = (head / 3) * 2 * Math.PI;
    const height = hy * (1.5 + hash01(prop.position[0], head) * 0.8);
    put(color, blob(0, { seed: 90 }), at(Math.sin(angle) * hx * 0.45, height, Math.cos(angle) * hx * 0.45, 0.07, 0.05, 0.07));
  }
}

/** Reeds sway, so they collect into a few shear groups rather than static batches. */
function reeds(context: ModelContext, prop: Prop, groups: Map<number, Map<Color, MeshBuilder>>): void {
  const { palette } = context.style;
  const group = Math.floor(seedOf(prop) * 3);
  let byColor = groups.get(group);
  if (!byColor) {
    byColor = new Map();
    groups.set(group, byColor);
  }
  const target = byColor;
  const frame = propFrame(context, prop);
  const put: Put = (color, shape, local) => {
    let builder = target.get(color);
    if (!builder) {
      builder = new MeshBuilder();
      target.set(color, builder);
    }
    builder.add(shape, compose(frame, local));
  };
  const [hx, hy] = half(prop, context.unitsPerMetre);
  for (let stalk = 0; stalk < 5; stalk += 1) {
    const angle = (stalk / 5) * 2 * Math.PI;
    const height = hy * 2 * (0.8 + hash01(prop.position[0], prop.position[2], stalk) * 0.45);
    put(palette.reeds, BLADE, compose(translate(Math.sin(angle) * hx * 1.2, -0.05, Math.cos(angle) * hx * 1.2), rotateY(angle), rotateX(-0.08), scale(0.06, height, 0.06)));
    if (stalk % 2 === 0) {
      put(palette.cattail, frustum(5, 1, { smooth: true }), at(Math.sin(angle) * hx * 1.2, height * 0.78, Math.cos(angle) * hx * 1.2, 0.045, 0.2, 0.045));
    }
  }
}

export type PropModelStats = { byKind: Partial<Record<PropKind, number>> };

/**
 * Appends every prop's model to the batches and collects animated parts.
 * Returns how many props of each kind were modelled.
 */
export function appendPropModels(context: ModelContext, props: readonly Prop[]): PropModelStats {
  const byKind: Partial<Record<PropKind, number>> = {};
  const reedGroups = new Map<number, Map<Color, MeshBuilder>>();
  const { palette } = context.style;
  let windmills = 0;
  let campfires = 0;
  for (const prop of props) {
    byKind[prop.kind] = (byKind[prop.kind] ?? 0) + 1;
    switch (prop.kind) {
      case "keep": keep(context, prop); break;
      case "inn": inn(context, prop); break;
      case "house": house(context, prop, palette.roofThatch, false); break;
      case "farmhouse": house(context, prop, palette.roofThatch, true); break;
      case "smithy": smithy(context, prop); break;
      case "barn": barn(context, prop); break;
      case "windmill": windmill(context, prop, windmills); windmills += 1; break;
      case "well": well(context, prop); break;
      case "palisade": palisade(context, prop); break;
      case "gate-post": gatePost(context, prop); break;
      case "waystone": waystone(context, prop); break;
      case "gravestone": gravestone(context, prop); break;
      case "tree-oak": tree(context, prop, "oak"); break;
      case "tree-pine": tree(context, prop, "pine"); break;
      case "tree-birch": tree(context, prop, "birch"); break;
      case "bush": bush(context, prop); break;
      case "rock-small":
      case "rock-medium":
      case "rock-large": rock(context, prop); break;
      case "cliff": cliff(context, prop); break;
      case "grass-tuft": grassTuft(context, prop); break;
      case "flowers": flowers(context, prop); break;
      case "reeds": reeds(context, prop, reedGroups); break;
      case "fence": fence(context, prop); break;
      case "tent": tent(context, prop); break;
      case "campfire": campfire(context, prop, campfires); campfires += 1; break;
      case "crate": crate(context, prop); break;
      case "barrel": barrel(context, prop); break;
      case "cart": cart(context, prop); break;
      case "signpost": signpost(context, prop); break;
      case "lamp": lamp(context, prop); break;
      case "mine-entrance": mineEntrance(context, prop); break;
      case "crop-row": cropRow(context, prop); break;
      case "dock": dock(context, prop); break;
      default: {
        const unknown: never = prop.kind;
        throw new Error(`No model for prop kind ${String(unknown)}.`);
      }
    }
  }
  for (const [group, byColor] of [...reedGroups.entries()].sort(([a], [b]) => a - b)) {
    context.animated.reeds.push({
      mesh: [...byColor.entries()]
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([color, builder], part) => ({ id: `reeds-${group}-${part}`, color, ...builder.data() })),
      groundY: 0,
      phase: group * 2.1,
    });
  }
  return { byKind };
}
