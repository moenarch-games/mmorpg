import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { HatStyle } from "../character-customization";
import type { CharacterPreview } from "../character-selection";
import { characterVisualProfile, type CharacterVisualProfile, type CharacterWeaponStyle } from "../character-visuals";
import { mixColor, shadeColor } from "./environment";
import type { ArmPose, LegPose, Pose, Stance } from "./character-animation";
import { BOX, MeshBuilder, blob, compose, frustum, rotateX, type Shape } from "./mesh-builder";
import type { Color } from "./scenery";

/**
 * A stylised humanoid from a handful of shared unit meshes placed by forward
 * kinematics: pelvis, spine, head, arms and legs with elbows and knees,
 * hands and feet, plus class gear (Warden sword, shield and helm; Ranger bow,
 * quiver and cap or hood; Arcanist staff and robe). Every part has a stable
 * node ID under the unit's ID, so the renderer reuses its objects and only
 * transforms change between frames.
 */
export type HumanoidLook = { visuals: CharacterVisualProfile; hat: HatStyle | null; skin: Color; hair: Color; longHair: boolean };

const SKIN: Color = "#d7a582";

/** How a character looks in the world, from its class, sex and chosen headwear. */
export function characterLook(character: Pick<CharacterPreview, "classId" | "sex">, hat: HatStyle | null): HumanoidLook {
  return {
    visuals: characterVisualProfile(character),
    hat,
    skin: SKIN,
    hair: character.sex === "female" ? "#8a5632" : "#4a3222",
    longHair: character.sex === "female",
  };
}

/** Projections carry no appearance yet, so other players share a neutral look. */
export const OTHER_PLAYER_LOOK: HumanoidLook = (() => {
  const look = characterLook({ classId: "warden", sex: "male" }, null);
  return {
    ...look,
    visuals: { ...look.visuals, bodyColor: "#8b9094", chestColor: "#767c80", shoulderColor: "#9aa0a4", cloakColor: "#5d6468" },
  };
})();

/** Where a unit stands in metres: its feet, raised by presentation relief. */
export type UnitPlacement = { x: number; feetY: number; z: number; yawRadians: number };

type Vec3 = [number, number, number];
type Quaternion = [number, number, number, number];

const IDENTITY_Q: Quaternion = [0, 0, 0, 1];

function qMul(a: Quaternion, b: Quaternion): Quaternion {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

function qAxis(axis: 0 | 1 | 2, angle: number): Quaternion {
  const s = Math.sin(angle / 2);
  const q: Quaternion = [0, 0, 0, Math.cos(angle / 2)];
  q[axis] = s;
  return q;
}

function qRotate(q: Quaternion, [vx, vy, vz]: Vec3): Vec3 {
  const [x, y, z, w] = q;
  // v + 2 w (q × v) + 2 q × (q × v)
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
type Frame = { p: Vec3; q: Quaternion };

function child(parent: Frame, offset: Vec3, rotation: Quaternion = IDENTITY_Q): Frame {
  const [ox, oy, oz] = qRotate(parent.q, offset);
  return { p: [parent.p[0] + ox, parent.p[1] + oy, parent.p[2] + oz], q: qMul(parent.q, rotation) };
}

/** Pitch about X (positive tips +Y toward +Z), then roll about Z, then yaw about Y, all local. */
function euler(pitch: number, yaw = 0, roll = 0): Quaternion {
  return qMul(qMul(qAxis(1, yaw), qAxis(0, pitch)), qAxis(2, roll));
}

type ShapeName = "limb" | "post" | "blob" | "box" | "cone" | "disc" | "skirt" | `torso-${number}`;

const shapes = new Map<ShapeName, Shape>();

/** Unit meshes shared by every humanoid part. */
function shape(name: ShapeName): Shape {
  const cached = shapes.get(name);
  if (cached) {
    return cached;
  }
  const builder = new MeshBuilder();
  switch (name) {
    // Tapered and hanging from its joint: y 0 (radius 1) down to y −1.
    case "limb": builder.add(frustum(8, 0.78, { smooth: true }), compose(rotateX(Math.PI))); break;
    case "skirt": builder.add(frustum(10, 1.45, { smooth: true, caps: false }), compose(rotateX(Math.PI))); break;
    case "post": builder.add(frustum(8, 1, { smooth: true }), compose()); break;
    case "blob": builder.add(blob(1), compose()); break;
    case "box": builder.add(BOX, compose()); break;
    case "cone": builder.add(frustum(8, 0, { smooth: true }), compose()); break;
    case "disc": builder.add(frustum(14, 1, { smooth: false }), compose()); break;
    default: {
      // Torso: a square frustum widening upward, flat faces on ±X and ±Z.
      const top = Number(name.slice("torso-".length)) / 100;
      builder.add(frustum(4, top, { offset: 0.5 }), compose());
    }
  }
  const mesh = builder.data();
  shapes.set(name, mesh);
  return mesh;
}

type Part = { name: string; shape: ShapeName; color: Color; frame: Frame; scale: Vec3; emissive?: Color };

function part(parts: Part[], name: string, shapeName: ShapeName, color: Color, frame: Frame, scale: Vec3, emissive?: Color): void {
  parts.push({ name, shape: shapeName, color, frame, scale, ...(emissive === undefined ? {} : { emissive }) });
}

/**
 * Proportions in metres for a look, read from the character visual profile:
 * a stylised build with a broad chest, thick limbs and big hands and feet.
 */
function build(visuals: CharacterVisualProfile) {
  const stature = visuals.bodyHeight / 1.7;
  const shoulder = 0.23 * (visuals.shoulderSpan / 0.52);
  const limb = visuals.armRadius / 0.16;
  return {
    pelvisY: 0.9 * stature,
    thigh: 0.4 * stature,
    shin: 0.4 * stature,
    torso: 0.46 * stature,
    upperArm: 0.27 * stature,
    forearm: 0.24 * stature,
    shoulder,
    hip: shoulder * visuals.hipRatio * 0.6,
    waist: 0.14 * (visuals.chestSize[0] / 0.86) * (0.82 + 0.2 * visuals.hipRatio),
    chest: shoulder * 1.02,
    depth: 0.12 * (visuals.chestSize[2] / 0.5),
    head: 0.13 * (visuals.headRadius / 0.34),
    limb,
  };
}

function stanceOf(weapon: CharacterWeaponStyle): Stance {
  switch (weapon) {
    case "sword": return "sword-and-shield";
    case "bow": return "bow";
    case "staff": return "staff";
  }
}

export function humanoidStance(look: HumanoidLook): Stance {
  return stanceOf(look.visuals.weapon);
}

const METAL: Color = "#b8bec4";
const DARK_METAL: Color = "#6d7378";
const LEATHER: Color = "#6a4a30";
const BOOTS: Color = "#4c3828";
const GLOW: Color = "#9fe8ff";

/** Renderer nodes for one humanoid unit in a pose. IDs are `${id}-${part}` and stable. */
export function humanoidNodes(id: string, placement: UnitPlacement, look: HumanoidLook, pose: Pose): RendererSceneNode[] {
  const { visuals } = look;
  const b = build(visuals);
  const parts: Part[] = [];
  const robe = visuals.weapon === "staff";
  const pants = robe ? shadeColor(visuals.chestColor, 0.8) : shadeColor(visuals.bodyColor, 0.62);
  const root: Frame = { p: [placement.x, placement.feetY, placement.z], q: qAxis(1, placement.yawRadians) };

  // Pelvis and legs. The character's left is local +X, its front +Z.
  const pelvis = child(root, [0, b.pelvisY + pose.bob, 0], qAxis(1, pose.hipYaw));
  part(parts, "hips", "box", pants, child(pelvis, [0, 0.02, 0]), [b.hip + 0.05, 0.1, b.depth * 0.95]);
  const legs: [string, 1 | -1, LegPose][] = [["left", 1, pose.leftLeg], ["right", -1, pose.rightLeg]];
  for (const [side, sign, leg] of legs) {
    const hip = child(pelvis, [sign * b.hip, -0.02, 0], euler(-leg.swing, 0, sign * leg.spread));
    part(parts, `${side}-thigh`, "limb", pants, hip, [0.098 * b.limb, b.thigh, 0.1 * b.limb]);
    const knee = child(hip, [0, -b.thigh, 0], euler(leg.knee));
    part(parts, `${side}-shin`, "limb", pants, knee, [0.074 * b.limb, b.shin, 0.078 * b.limb]);
    part(parts, `${side}-boot`, "limb", BOOTS, child(knee, [0, -b.shin * 0.42, 0]), [0.084 * b.limb, b.shin * 0.58, 0.088 * b.limb]);
    const ankle = child(knee, [0, -b.shin, 0], euler(leg.foot - leg.knee * 0.25 + leg.swing * 0.2));
    part(parts, `${side}-foot`, "box", BOOTS, child(ankle, [0, -0.04, 0.06]), [0.066, 0.045, 0.14]);
  }

  // Spine, chest and head.
  const spine = child(pelvis, [0, 0.06, 0], euler(pose.lean, pose.twist));
  const torsoShape: ShapeName = `torso-${Math.round((b.chest / b.waist) * 100)}`;
  const breathe = 1 + pose.breathe;
  part(parts, "torso", torsoShape, visuals.bodyColor, spine, [b.waist / 0.707 * breathe, b.torso, b.depth / 0.707 * breathe]);
  // The torso widens upward: its front face sits at the waist depth scaled by the taper.
  const plateHeight = 0.62;
  const plateDepth = b.depth * (1 + (b.chest / b.waist - 1) * plateHeight) * breathe;
  part(parts, "chest-plate", "box", visuals.chestColor, child(spine, [0, b.torso * plateHeight, plateDepth]), [b.chest * 0.6, b.torso * 0.24, 0.02]);
  part(parts, "belt", "box", LEATHER, child(spine, [0, 0.03, 0]), [b.waist + 0.012, 0.035, b.depth + 0.012]);
  part(parts, "buckle", "box", METAL, child(spine, [0, 0.03, b.depth + 0.012]), [0.03, 0.028, 0.01]);
  const neck = child(spine, [0, b.torso + 0.02, 0], euler(pose.headPitch));
  part(parts, "neck", "post", look.skin, child(neck, [0, -0.02, 0]), [0.045, 0.08, 0.045]);
  const head = child(neck, [0, b.head + 0.05, 0.01]);
  part(parts, "head", "blob", look.skin, head, [b.head * 0.92, b.head * 1.05, b.head]);
  part(parts, "nose", "box", shadeColor(look.skin, 0.9), child(head, [0, -0.01, b.head * 0.98]), [0.016, 0.026, 0.022]);
  for (const sign of [1, -1]) {
    part(parts, `eye-${sign > 0 ? "left" : "right"}`, "box", "#2a2320", child(head, [sign * b.head * 0.38, b.head * 0.18, b.head * 0.88]), [0.014, 0.016, 0.01]);
  }
  hat(parts, look, head, b.head);
  if (look.longHair) {
    // A braid down the back, under any hat.
    part(parts, "braid", "limb", look.hair, child(head, [0, -b.head * 0.2, -b.head * 0.85], euler(0.25)), [0.045, 0.3, 0.04]);
    part(parts, "braid-tie", "blob", LEATHER, child(head, [0, -b.head * 0.2 - 0.29, -b.head * 0.85 - 0.075]), [0.035, 0.03, 0.035]);
  }

  // Arms.
  const arms: [string, 1 | -1, ArmPose][] = [["left", 1, pose.leftArm], ["right", -1, pose.rightArm]];
  const hands: Record<string, Frame> = {};
  const forearms: Record<string, Frame> = {};
  for (const [side, sign, arm] of arms) {
    const shoulder = child(spine, [sign * b.shoulder, b.torso - 0.06, 0], euler(-arm.swing, 0, sign * arm.spread));
    part(parts, `${side}-upper-arm`, "limb", visuals.bodyColor, shoulder, [0.068 * b.limb, b.upperArm, 0.068 * b.limb]);
    const elbow = child(shoulder, [0, -b.upperArm, 0], euler(-arm.elbow));
    forearms[side] = elbow;
    part(parts, `${side}-forearm`, "limb", robe ? visuals.chestColor : visuals.bodyColor, elbow, [0.058 * b.limb, b.forearm, 0.058 * b.limb]);
    part(parts, `${side}-glove`, "limb", LEATHER, child(elbow, [0, -b.forearm * 0.52, 0]), [0.064 * b.limb, b.forearm * 0.48, 0.064 * b.limb]);
    const hand = child(elbow, [0, -b.forearm - 0.045, 0]);
    hands[side] = hand;
    part(parts, `${side}-hand`, "blob", look.skin, hand, [0.052, 0.064, 0.058]);
    part(parts, `${side}-shoulder`, "blob", visuals.shoulderColor, child(spine, [sign * b.shoulder, b.torso - 0.02, 0]),
      visuals.weapon === "sword" ? [0.12, 0.085, 0.125] : [0.09, 0.068, 0.095]);
  }

  // Cloak or robe hem, swinging back with speed.
  if (robe) {
    const skirt = child(pelvis, [0, 0.06, 0], euler(pose.cape * 0.35));
    const length = 0.72 * (b.thigh + b.shin) / 0.8;
    part(parts, "robe", "skirt", visuals.chestColor, skirt, [b.hip + 0.07, length, b.depth + 0.04]);
    // A gold hem and a sash break up the robe.
    part(parts, "robe-hem", "post", "#c9a85a", child(skirt, [0, -length, 0]), [(b.hip + 0.07) * 1.46, 0.05, (b.depth + 0.04) * 1.46]);
    part(parts, "sash", "box", "#c9a85a", child(spine, [0.05, b.torso * 0.5, 0], euler(0, 0, 0.55)), [0.03, b.torso * 0.62, b.depth * 1.22]);
  }
  // The cloak hangs from the shoulders, its centre half its length below them.
  // A positive pitch swings a hanging part's lower end backward (−Z).
  const cloakFrame = child(spine, [0, b.torso - 0.02, -b.depth - 0.03], euler(pose.cape));
  part(parts, "cloak", "box", visuals.cloakColor, child(cloakFrame, [0, -0.38, 0]), [b.shoulder * 0.9, 0.38, 0.015]);

  gear(parts, look, spine, hands, forearms, root, b.torso, b.depth);

  return renderParts(id, parts);
}

function renderParts(id: string, parts: Part[]): RendererSceneNode[] {
  return parts.map((entry) => ({
    id: `${id}-${entry.name}`,
    geometry: { kind: "mesh", resourceKey: `humanoid:${entry.shape}`, ...shape(entry.shape) },
    color: entry.color,
    ...(entry.emissive === undefined ? {} : { emissive: entry.emissive }),
    transform: { translation: entry.frame.p, rotationQuaternion: entry.frame.q, scale: entry.scale },
  }));
}

/** Retain local headwear customization while the first asset kit only supplies a bare head. */
export function humanoidHeadwearNodes(id: string, placement: UnitPlacement, look: HumanoidLook, pose: Pose): RendererSceneNode[] {
  const b = build(look.visuals);
  const root: Frame = { p: [placement.x, placement.feetY, placement.z], q: qAxis(1, placement.yawRadians) };
  const pelvis = child(root, [0, b.pelvisY + pose.bob, 0], qAxis(1, pose.hipYaw));
  const spine = child(pelvis, [0, 0.06, 0], euler(pose.lean, pose.twist));
  const neck = child(spine, [0, b.torso + 0.02, 0], euler(pose.headPitch));
  const head = child(neck, [0, b.head + 0.05, 0.01]);
  const parts: Part[] = [];
  hat(parts, look, head, b.head);
  if (look.longHair) {
    part(parts, "braid", "limb", look.hair, child(head, [0, -b.head * 0.2, -b.head * 0.85], euler(0.25)), [0.045, 0.3, 0.04]);
    part(parts, "braid-tie", "blob", LEATHER, child(head, [0, -b.head * 0.2 - 0.29, -b.head * 0.85 - 0.075]), [0.035, 0.03, 0.035]);
  }
  return renderParts(id, parts);
}

/** The chosen headwear on an imported model's posed head (centre in metres, facing `yaw`); none without a hat. */
export function headwearAtNodes(id: string, look: HumanoidLook, centre: Vec3, yawRadians: number, radius: number): RendererSceneNode[] {
  if (look.hat === null) {
    return [];
  }
  const parts: Part[] = [];
  hat(parts, look, { p: centre, q: qAxis(1, yawRadians) }, radius);
  return renderParts(id, parts);
}

function hat(parts: Part[], look: HumanoidLook, head: Frame, radius: number): void {
  const hair = look.hair;
  switch (look.hat) {
    case null:
      part(parts, "hair", "blob", hair, child(head, [0, radius * 0.22, -radius * 0.12]), [radius * 1.02, radius * 0.9, radius * 1.02]);
      return;
    case "ironcrest-helm":
      part(parts, "helm", "blob", METAL, child(head, [0, radius * 0.3, -0.005]), [radius * 1.1, radius * 0.95, radius * 1.12]);
      part(parts, "helm-rim", "post", DARK_METAL, child(head, [0, radius * 0.12, 0]), [radius * 1.12, 0.035, radius * 1.14]);
      part(parts, "helm-crest", "box", "#a8453a", child(head, [0, radius * 1.18, -0.01]), [0.018, radius * 0.4, radius * 0.9]);
      part(parts, "helm-nasal", "box", METAL, child(head, [0, radius * 0.05, radius * 1.08]), [0.014, radius * 0.4, 0.012]);
      return;
    case "ranger-cap":
      part(parts, "hair", "blob", hair, child(head, [0, radius * 0.1, -radius * 0.2]), [radius * 1.02, radius * 0.85, radius * 1.0]);
      part(parts, "cap-brim", "post", "#6f875f", child(head, [0, radius * 0.45, 0.01]), [radius * 1.45, 0.02, radius * 1.45]);
      part(parts, "cap-crown", "cone", "#5d7351", child(head, [0, radius * 0.47, -0.005], euler(-0.15)), [radius * 1.02, radius * 1.15, radius * 1.02]);
      part(parts, "cap-feather", "box", "#c8483a", child(head, [radius * 0.8, radius * 0.95, -radius * 0.3], euler(-0.6, 0, 0.4)), [0.01, radius * 0.7, 0.02]);
      return;
    case "wayfarer-hood":
      part(parts, "hood", "blob", "#b7c6bd", child(head, [0, radius * 0.18, -radius * 0.14]), [radius * 1.18, radius * 1.12, radius * 1.18]);
      part(parts, "hood-peak", "cone", "#a7b8ae", child(head, [0, radius * 0.7, -radius * 0.45], euler(-0.9)), [radius * 0.6, radius * 1.0, radius * 0.6]);
      part(parts, "hood-face", "blob", shadeColor(look.skin, 0.55), child(head, [0, -radius * 0.02, radius * 0.6]), [radius * 0.72, radius * 0.8, radius * 0.45]);
      part(parts, "hood-face-light", "blob", look.skin, child(head, [0, -radius * 0.08, radius * 0.72]), [radius * 0.6, radius * 0.66, radius * 0.4]);
      return;
  }
}

function gear(
  parts: Part[],
  look: HumanoidLook,
  spine: Frame,
  hands: Record<string, Frame>,
  forearms: Record<string, Frame>,
  root: Frame,
  torso: number,
  depth: number,
): void {
  const { visuals } = look;
  const right = hands.right!;
  const left = hands.left!;
  switch (visuals.weapon) {
    case "sword": {
      // Blade along the fist's local +Z, angled down-forward.
      const grip = child(right, [0, -0.01, 0.01], euler(0.75));
      part(parts, "sword-grip", "box", LEATHER, grip, [0.018, 0.018, 0.07]);
      part(parts, "sword-pommel", "blob", "#c7b66d", child(grip, [0, 0, -0.08]), [0.03, 0.03, 0.03]);
      part(parts, "sword-guard", "box", "#c7b66d", child(grip, [0, 0, 0.075]), [0.11, 0.02, 0.022]);
      part(parts, "sword-blade", "box", visuals.weaponColor, child(grip, [0, 0, 0.5]), [0.032, 0.007, 0.42]);
      // A round shield strapped to the left forearm, facing outward.
      const shield = child(forearms.left!, [0.07, -0.15, 0.02], euler(0, 0, -Math.PI / 2));
      part(parts, "shield", "disc", mixColor(visuals.chestColor, "#2f5f8f", 0.55), shield, [0.29, 0.04, 0.29]);
      part(parts, "shield-rim", "disc", DARK_METAL, child(shield, [0, -0.004, 0]), [0.31, 0.03, 0.31]);
      part(parts, "shield-boss", "blob", METAL, child(shield, [0, 0.045, 0]), [0.07, 0.04, 0.07]);
      part(parts, "shield-band", "box", "#d8c27a", child(shield, [0, 0.042, 0]), [0.035, 0.004, 0.25]);
      return;
    }
    case "bow": {
      // The bow stays upright beside the left hand, its curve facing sideways so
      // it reads from behind and in front.
      const bow: Frame = { p: left.p, q: qMul(root.q, euler(0.12, Math.PI / 2, 0.08)) };
      part(parts, "bow-grip", "box", LEATHER, bow, [0.022, 0.07, 0.024]);
      for (const sign of [1, -1]) {
        const limb = child(bow, [0, sign * 0.33, -0.05], euler(sign * 0.3));
        part(parts, `bow-limb-${sign > 0 ? "upper" : "lower"}`, "box", visuals.weaponColor, limb, [0.018, 0.29, 0.02]);
      }
      part(parts, "bow-string", "box", "#e2ddcc", child(bow, [0, 0, -0.12]), [0.004, 0.6, 0.004]);
      // Over the back, the fletching reaching past the right shoulder (local −X).
      const quiver = child(spine, [0.08, torso * 0.3, -depth - 0.06], euler(-0.2, 0, 0.4));
      part(parts, "quiver", "post", LEATHER, quiver, [0.06, 0.48, 0.06]);
      part(parts, "quiver-fletching", "box", "#e9e2d0", child(quiver, [0, 0.52, 0]), [0.05, 0.05, 0.05]);
      part(parts, "quiver-fletching-red", "box", "#c8483a", child(quiver, [0.02, 0.56, 0.01]), [0.03, 0.04, 0.03]);
      return;
    }
    case "staff": {
      // The staff stays upright in the right hand, leaning slightly forward.
      const staff: Frame = { p: right.p, q: qMul(root.q, euler(0.12, 0, -0.05)) };
      part(parts, "staff", "post", visuals.weaponColor, child(staff, [0, -0.8, 0]), [0.024, 1.85, 0.024]);
      part(parts, "staff-fitting", "post", "#c7b66d", child(staff, [0, 0.95, 0]), [0.034, 0.07, 0.034]);
      part(parts, "staff-orb", "blob", GLOW, child(staff, [0, 1.12, 0]), [0.075, 0.075, 0.075], GLOW);
      part(parts, "staff-orb-core", "blob", "#ffffff", child(staff, [0, 1.12, 0]), [0.04, 0.04, 0.04], "#ffffff");
      return;
    }
  }
}
