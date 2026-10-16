import { poseFor, type LocomotionState, type Pose, type Stance } from "../character-animation";
import type { UnitPlacement } from "../humanoid";
import type { Color } from "../scenery";
import { addPart, ball, box, child, post, rootFrame, tilt, type Frame, type Part, type Primitive } from "./model-kit";

/**
 * A compact humanoid for creatures and NPCs: pelvis, torso, head, jointed
 * arms and single-piece legs with boots, posed by the shared `poseFor` so it
 * walks and breathes like a player. It is drawn at the unit's own collision
 * box (design measures are for a 1.8 m × 0.6 m person and scale per axis) and
 * keeps to 12 base parts, leaving the rest of a unit's 24-node budget to the
 * role accessories a spec adds. The full player rig (`humanoidNodes`) draws
 * about fifty nodes, too many for a crowd of creatures.
 */
export type HumanoidSpec = {
  skin: Color;
  /** Torso and arm clothing. */
  tunic: Color;
  trousers: Color;
  boots: Color;
  stance: Stance;
  /** Constant forward lean in radians on top of the pose's. */
  hunch: number;
  /** Adds role gear; may use at most `HUMANOID_ACCESSORY_BUDGET` parts. */
  accessories: (gear: HumanoidGear) => void;
};

/** Parts a spec's accessories may add beyond the 12 base parts, keeping a unit with its target ring at 24 nodes. */
export const HUMANOID_ACCESSORY_BUDGET = 11;

/** What accessories attach to: joint frames and the box scale (design metres → this unit's box). */
export type HumanoidGear = {
  parts: Part[];
  pose: Pose;
  root: Frame;
  pelvis: Frame;
  spine: Frame;
  head: Frame;
  rightHand: Frame;
  leftForearm: Frame;
  rightForearm: Frame;
  /** Scale of design widths, heights and depths. */
  sx: number;
  sy: number;
  sz: number;
  spec: HumanoidSpec;
  /** Adds a part with sizes given in design metres. */
  add(name: string, primitive: Primitive, color: Color, frame: Frame): void;
};

const DESIGN_HEIGHT = 1.8;
const DESIGN_HALF_WIDTH = 0.3;

/** A person standing still, for corpses and idle units. */
export const REST: LocomotionState = {
  forward: 0, right: 0, vertical: 0, moveWeight: 0, airWeight: 0, turnRate: 0, stridePhase: 0, shufflePhase: 0, time: 0,
};

/** Half the horizontal depth a lying body keeps below its centre, in design metres. */
const LYING_LIFT = 0.16;

/**
 * The parts of one humanoid in a pose inside the box `halfMetres` (x, y, z);
 * lying flat on its back when `dead`.
 */
export function humanoidParts(
  placement: UnitPlacement,
  halfMetres: readonly [number, number, number],
  spec: HumanoidSpec,
  pose: Pose,
  dead: boolean,
): Part[] {
  const [halfX, halfY, halfZ] = halfMetres;
  const sx = halfX / DESIGN_HALF_WIDTH;
  const sy = halfY * 2 / DESIGN_HEIGHT;
  const sz = halfZ / DESIGN_HALF_WIDTH;
  const resting = dead ? poseFor(REST, spec.stance) : pose;
  const root = rootFrame(placement, halfY, dead ? { rotation: tilt(-Math.PI / 2), lift: LYING_LIFT * sz } : null);
  const parts: Part[] = [];
  const { trousers, boots, skin, tunic } = spec;
  const add = (name: string, primitive: Primitive, color: Color, frame: Frame) => addPart(parts, name, primitive, color, frame);
  const pelvis = child(root, [0, 0.9 * sy + resting.bob, 0]);
  for (const [side, sign, leg] of [["left", 1, resting.leftLeg], ["right", -1, resting.rightLeg]] as const) {
    const hip = child(pelvis, [sign * 0.1 * sx, 0, 0], tilt(-leg.swing, sign * leg.spread));
    add(`${side}-leg`, box(0.15 * sx, 0.78 * sy, 0.17 * sz), trousers, child(hip, [0, -0.39 * sy, 0]));
    add(`${side}-boot`, box(0.16 * sx, 0.1 * sy, 0.27 * sz), boots, child(hip, [0, -0.83 * sy, 0.04 * sz]));
  }
  const spine = child(pelvis, [0, 0.04 * sy, 0], tilt(resting.lean + spec.hunch, 0, resting.twist));
  const breathe = 1 + resting.breathe;
  add("torso", box(0.42 * sx * breathe, 0.56 * sy, 0.24 * sz * breathe), tunic, child(spine, [0, 0.28 * sy, 0]));
  const neck = child(spine, [0, 0.58 * sy, 0], tilt(resting.headPitch));
  const head = child(neck, [0, 0.12 * sy, 0.01 * sz]);
  add("head", ball(0.125 * sy), skin, head);
  const arms = [["left", 1, resting.leftArm], ["right", -1, resting.rightArm]] as const;
  const forearms: Record<string, Frame> = {};
  const hands: Record<string, Frame> = {};
  for (const [side, sign, arm] of arms) {
    const shoulder = child(spine, [sign * 0.22 * sx, 0.5 * sy, 0], tilt(-arm.swing, sign * arm.spread * 0.4));
    add(`${side}-arm`, box(0.085 * sx, 0.28 * sy, 0.09 * sz), tunic, child(shoulder, [0, -0.14 * sy, 0]));
    const elbow = child(shoulder, [0, -0.28 * sy, 0], tilt(-arm.elbow));
    forearms[side] = elbow;
    add(`${side}-forearm`, box(0.075 * sx, 0.27 * sy, 0.08 * sz), skin, child(elbow, [0, -0.135 * sy, 0]));
    const hand = child(elbow, [0, -0.29 * sy, 0]);
    hands[side] = hand;
    add(`${side}-hand`, ball(0.045 * sy), skin, hand);
  }
  const base = parts.length;
  spec.accessories({
    parts,
    pose: resting,
    root,
    pelvis,
    spine,
    head,
    rightHand: hands.right!,
    leftForearm: forearms.left!,
    rightForearm: forearms.right!,
    sx,
    sy,
    sz,
    spec,
    add,
  });
  if (parts.length - base > HUMANOID_ACCESSORY_BUDGET) {
    throw new Error(`humanoid accessories use ${parts.length - base} parts, over the budget of ${HUMANOID_ACCESSORY_BUDGET}`);
  }
  return parts;
}

/** A hanging blade at the right hand: grip and blade as one box, tilted forward, tip down. */
export function addSword(gear: HumanoidGear, color: Color = "#c3c8cc"): void {
  gear.add("sword", box(0.04 * gear.sx, 0.46 * gear.sy, 0.03 * gear.sz), color, child(gear.rightHand, [0, -0.2 * gear.sy, 0.05 * gear.sz], tilt(0.35)));
}

/** A round shield on the left forearm, facing forward. */
export function addShield(gear: HumanoidGear, color: Color): void {
  gear.add(
    "shield",
    post(0.1 * gear.sx, 0.04),
    color,
    child(gear.leftForearm, [-0.07 * gear.sx, -0.13 * gear.sy, 0.07 * gear.sz], tilt(Math.PI / 2)),
  );
}

