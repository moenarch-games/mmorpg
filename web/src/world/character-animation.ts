/**
 * Procedural character animation. `poseFor` is a pure function from a
 * locomotion state to bounded joint angles: idle breathing, a walk/run cycle
 * whose phase comes from distance travelled, strafe and backpedal variants,
 * a jump/fall pose and a turn-in-place shuffle. `UnitAnimator` derives that
 * state for one unit from its interpolated samples. None of it feeds back
 * into the simulation.
 */

/** Run speed from core: 21 units per tick at 30 Hz, in metres per second. */
export const RUN_SPEED = 6.3;
/** Metres per full stride cycle (two steps) at run speed. */
const STRIDE_METRES = 2.6;
/** Blend ramps reach their target over this many seconds. */
const BLEND_SECONDS = 0.14;

export type LocomotionState = {
  /** Velocity in the character's frame in m/s: forward (+) or back (−), right (+) or left (−). */
  forward: number;
  right: number;
  /** Vertical velocity in m/s. */
  vertical: number;
  /** 0 standing … 1 moving, ramped by the animator. */
  moveWeight: number;
  /** 0 grounded … 1 airborne, ramped by the animator. */
  airWeight: number;
  /** Facing change in rad/s, positive toward +X. */
  turnRate: number;
  /** Stride phase in radians, advanced by distance travelled. */
  stridePhase: number;
  /** Shuffle phase in radians, advanced by turning in place. */
  shufflePhase: number;
  /** Seconds for idle breathing; constant under reduced motion. */
  time: number;
};

export type LegPose = { swing: number; spread: number; knee: number; foot: number };
export type ArmPose = { swing: number; spread: number; elbow: number };

/** Joint angles in radians and offsets in metres; `swing` is positive forward. */
export type Pose = {
  bob: number;
  lean: number;
  twist: number;
  hipYaw: number;
  breathe: number;
  headPitch: number;
  /** Backward swing of a cloak or robe hem. */
  cape: number;
  leftLeg: LegPose;
  rightLeg: LegPose;
  leftArm: ArmPose;
  rightArm: ArmPose;
};

/** What the character holds changes its arm stance. */
export type Stance = "sword-and-shield" | "bow" | "staff";

/** Inclusive bounds every pose value stays within. */
export const POSE_LIMITS = {
  bob: [-0.12, 0.08],
  lean: [-0.3, 0.35],
  twist: [-0.4, 0.4],
  hipYaw: [-0.8, 0.8],
  breathe: [0, 0.03],
  headPitch: [-0.3, 0.3],
  cape: [0, 0.9],
  legSwing: [-1.2, 1.3],
  legSpread: [-0.45, 0.45],
  knee: [0, 1.9],
  foot: [-0.6, 0.6],
  armSwing: [-1.2, 2.2],
  armSpread: [-0.2, 1.2],
  elbow: [0, 2.0],
} as const satisfies Record<string, readonly [number, number]>;

function clamp(value: number, [low, high]: readonly [number, number]): number {
  return Math.min(high, Math.max(low, Number.isFinite(value) ? value : 0));
}

function finite(value: number, fallback = 0): number {
  return Number.isFinite(value) ? value : fallback;
}

function unit(value: number): number {
  return Math.min(1, Math.max(0, finite(value)));
}

/** The pose for a locomotion state and stance; pure, finite and within `POSE_LIMITS`. */
export function poseFor(state: LocomotionState, stance: Stance): Pose {
  const forward = finite(state.forward);
  const right = finite(state.right);
  const speed = Math.hypot(forward, right);
  const move = unit(state.moveWeight);
  const air = unit(state.airWeight);
  const ground = 1 - air;
  const phase = finite(state.stridePhase);
  const time = finite(state.time);
  const run = unit(speed / RUN_SPEED);
  // Direction of travel relative to facing: 0 forward, ±π/2 strafing, π backward.
  const direction = speed > 0.05 ? Math.atan2(right, forward) : 0;
  const along = Math.cos(direction);
  const across = Math.sin(direction);
  const backward = along < -0.2;
  // Diagonals turn the hips toward the travel direction; pure strafes sidestep.
  const hipYaw = backward ? 0 : Math.max(-0.7, Math.min(0.7, direction)) * Math.min(1, Math.abs(along) * 2) * move * ground;

  const stride = (0.45 + 0.4 * run) * move * ground;
  const sin = Math.sin(phase);
  const cos = Math.cos(phase);
  const legAngle = (side: 1 | -1) => side * stride * sin;
  // With turned hips the legs swing straight ahead of the hips; otherwise
  // the swing splits into forward (pitch) and sideways (spread) parts.
  const turnedHips = Math.abs(hipYaw) > 0.05;
  const forwardSwing = turnedHips ? 1 : Math.abs(along);
  const leg = (side: 1 | -1): LegPose => {
    const angle = legAngle(side);
    const lifting = Math.max(0, side * cos);
    return {
      swing: angle * forwardSwing * (backward ? -0.8 : 1),
      spread: angle * (turnedHips ? 0 : across) * -0.55,
      knee: (0.15 * run + (0.35 + 0.9 * run) * lifting) * move * ground,
      foot: angle * 0.35,
    };
  };

  // Idle: breathing and a relaxed stance.
  const breathe = 0.015 + 0.012 * Math.sin(time * 2.1);
  const idle = 1 - move;

  // Turn in place: short alternating steps while standing still.
  const turning = Math.min(1, Math.abs(finite(state.turnRate)) / 2.5) * idle * ground;
  const shuffle = finite(state.shufflePhase);
  const shuffleStep = (side: 1 | -1) => Math.max(0, side * Math.sin(shuffle)) * turning;

  const leftLeg = leg(1);
  const rightLeg = leg(-1);
  leftLeg.knee += shuffleStep(1) * 0.55;
  rightLeg.knee += shuffleStep(-1) * 0.55;
  leftLeg.spread += shuffleStep(1) * 0.12;
  rightLeg.spread -= shuffleStep(-1) * 0.12;

  // Jump: legs tuck while rising and reach for the ground while falling.
  const rising = Math.max(0, Math.min(1, finite(state.vertical) / 3));
  const tuck = air * (0.45 + 0.55 * rising);
  leftLeg.swing += tuck * 0.55;
  rightLeg.swing += tuck * 0.15;
  leftLeg.knee += tuck * 1.1;
  rightLeg.knee += air * (0.35 + 0.3 * rising);

  // Arms swing against the legs; held gear damps its arm.
  const armStride = (0.5 + 0.55 * run) * move * ground * (backward ? 0.6 : 1);
  const heldRight = stance === "staff" ? 0.35 : stance === "sword-and-shield" ? 0.7 : 0.85;
  const heldLeft = stance === "sword-and-shield" ? 0.5 : stance === "bow" ? 0.6 : 0.9;
  const arm = (side: 1 | -1, held: number, restElbow: number): ArmPose => ({
    swing: -side * sin * armStride * held * forwardSwing + air * 0.35 + idle * 0.02 * Math.sin(time * 1.3 + side),
    spread: 0.1 + 0.05 * idle * Math.sin(time * 2.1) + air * (0.55 + 0.25 * (1 - rising)) + turning * 0.1,
    elbow: restElbow + (0.35 + 0.9 * run) * move * ground + air * 0.3,
  });
  const leftArm = arm(1, heldLeft, stance === "sword-and-shield" ? 0.55 : stance === "bow" ? 0.35 : 0.2);
  const rightArm = arm(-1, heldRight, stance === "staff" ? 0.9 : stance === "sword-and-shield" ? 0.45 : 0.2);

  return {
    bob: clamp((0.045 * run + 0.02) * (Math.cos(2 * phase) - 1) / 2 * move * ground - breathe * 0.3 * idle, POSE_LIMITS.bob),
    lean: clamp((backward ? -0.06 : 0.05 + 0.14 * run) * move * ground - air * 0.05 * (1 - rising), POSE_LIMITS.lean),
    twist: clamp(0.14 * sin * stride - hipYaw * 0.6, POSE_LIMITS.twist),
    hipYaw: clamp(hipYaw, POSE_LIMITS.hipYaw),
    breathe: clamp(breathe * idle, POSE_LIMITS.breathe),
    headPitch: clamp(-0.08 * run * move + 0.04 * idle * Math.sin(time * 0.7), POSE_LIMITS.headPitch),
    cape: clamp(0.08 + (backward ? 0.05 : 0.45 * run) * move + air * (0.2 + 0.3 * (1 - rising)), POSE_LIMITS.cape),
    leftLeg: boundLeg(leftLeg),
    rightLeg: boundLeg(rightLeg),
    leftArm: boundArm(leftArm),
    rightArm: boundArm(rightArm),
  };
}

/** Joint values of a four-legged body: leg swings in radians (positive reaches forward), offsets in metres. */
export type QuadrupedPose = {
  /** Body height offset; the body dips at each footfall. */
  bob: number;
  /** Nose-up (+) body pitch. */
  pitch: number;
  breathe: number;
  headPitch: number;
  /** Tail sway in radians, positive toward +X. */
  tailSway: number;
  /** Tail raise in radians. */
  tailLift: number;
  frontLeft: number;
  frontRight: number;
  backLeft: number;
  backRight: number;
};

/** Inclusive bounds every quadruped pose value stays within. */
export const QUADRUPED_LIMITS = {
  bob: [-0.05, 0],
  pitch: [-0.12, 0.12],
  breathe: [0, 0.04],
  headPitch: [-0.3, 0.3],
  tailSway: [-0.6, 0.6],
  tailLift: [0, 0.8],
  legSwing: [-0.9, 0.9],
} as const satisfies Record<string, readonly [number, number]>;

/** Short legs cycle faster than a person: gait cycles per stride cycle of the shared animator. */
export const QUADRUPED_GAIT_RATE = 2.2;

/**
 * The trot of a four-legged body: diagonal leg pairs (front left with back
 * right, front right with back left) swing against each other, their phase
 * advancing with distance travelled and their reach growing with horizontal
 * speed. At rest the legs hang still and the body breathes. Pure, finite and
 * within `QUADRUPED_LIMITS`.
 */
export function quadrupedPoseFor(state: LocomotionState): QuadrupedPose {
  const speed = Math.hypot(finite(state.forward), finite(state.right));
  const move = unit(state.moveWeight);
  const idle = 1 - move;
  const run = unit(speed / RUN_SPEED);
  const phase = finite(state.stridePhase) * QUADRUPED_GAIT_RATE;
  const time = finite(state.time);
  const reach = (0.3 + 0.45 * run) * move;
  const diagonal = reach * Math.sin(phase);
  const breathe = (0.02 + 0.015 * Math.sin(time * 2.1)) * idle;
  return {
    bob: clamp(-(0.012 + 0.03 * run) * move * (1 - Math.cos(2 * phase)) / 2, QUADRUPED_LIMITS.bob),
    pitch: clamp(0.05 * run * move * Math.sin(2 * phase), QUADRUPED_LIMITS.pitch),
    breathe: clamp(breathe, QUADRUPED_LIMITS.breathe),
    headPitch: clamp(-0.12 * run * move + 0.05 * idle * Math.sin(time * 0.7), QUADRUPED_LIMITS.headPitch),
    tailSway: clamp(0.3 * idle * Math.sin(time * 1.7) + 0.2 * move * Math.sin(phase), QUADRUPED_LIMITS.tailSway),
    tailLift: clamp(0.15 + 0.45 * run * move, QUADRUPED_LIMITS.tailLift),
    frontLeft: clamp(diagonal, QUADRUPED_LIMITS.legSwing),
    backRight: clamp(diagonal, QUADRUPED_LIMITS.legSwing),
    frontRight: clamp(-diagonal, QUADRUPED_LIMITS.legSwing),
    backLeft: clamp(-diagonal, QUADRUPED_LIMITS.legSwing),
  };
}

function boundLeg(leg: LegPose): LegPose {
  return {
    swing: clamp(leg.swing, POSE_LIMITS.legSwing),
    spread: clamp(leg.spread, POSE_LIMITS.legSpread),
    knee: clamp(leg.knee, POSE_LIMITS.knee),
    foot: clamp(leg.foot, POSE_LIMITS.foot),
  };
}

function boundArm(arm: ArmPose): ArmPose {
  return {
    swing: clamp(arm.swing, POSE_LIMITS.armSwing),
    spread: clamp(arm.spread, POSE_LIMITS.armSpread),
    elbow: clamp(arm.elbow, POSE_LIMITS.elbow),
  };
}

/** One interpolated sample of a unit, in metres, m/s and radians. */
export type UnitSample = {
  x: number;
  z: number;
  /** Velocity in m/s (world axes). */
  velocity: readonly [number, number, number];
  /** Yaw in radians: 0 faces +Z, increasing toward +X. */
  facing: number;
};

function ramp(current: number, target: number, deltaSeconds: number, instant: boolean): number {
  if (instant) {
    return target;
  }
  const step = deltaSeconds / BLEND_SECONDS;
  return current < target ? Math.min(target, current + step) : Math.max(target, current - step);
}

function angleDelta(from: number, to: number): number {
  const full = 2 * Math.PI;
  return ((((to - from) % full) + full + Math.PI) % full) - Math.PI;
}

/**
 * Per-unit animation state: distance travelled (stride phase), turning and
 * blend weights. `instant` (reduced motion) snaps blends and freezes idle
 * time so a standing character renders identically every frame.
 */
export class UnitAnimator {
  #last: UnitSample | null = null;
  #stride = 0;
  #shuffle = 0;
  #turnRate = 0;
  #move = 0;
  #air = 0;
  #time = 0;

  update(sample: UnitSample, deltaSeconds: number, instant: boolean): LocomotionState {
    const dt = Math.max(0, finite(deltaSeconds));
    const [vx, vy, vz] = sample.velocity;
    const sin = Math.sin(sample.facing);
    const cos = Math.cos(sample.facing);
    // The character's right is facing − 90°: (−cos, sin).
    const forward = vx * sin + vz * cos;
    const right = -vx * cos + vz * sin;
    const last = this.#last;
    if (last) {
      const travelled = Math.hypot(sample.x - last.x, sample.z - last.z);
      // A teleport (respawn, re-entry) must not spin the stride.
      if (travelled < 2) {
        this.#stride += (travelled / STRIDE_METRES) * 2 * Math.PI;
      }
      const turned = angleDelta(last.facing, sample.facing);
      this.#turnRate = dt > 0 ? turned / dt : 0;
      this.#shuffle += Math.abs(turned) * 3;
    }
    this.#last = sample;
    this.#stride %= 2 * Math.PI * 1000;
    this.#shuffle %= 2 * Math.PI * 1000;
    const moving = Math.hypot(forward, right) > 0.3 ? 1 : 0;
    const airborne = Math.abs(vy) >= 0.3 ? 1 : 0;
    this.#move = ramp(this.#move, moving, dt, instant);
    this.#air = ramp(this.#air, airborne, dt, instant);
    this.#time = instant ? 0 : this.#time + dt;
    return {
      forward,
      right,
      vertical: vy,
      moveWeight: this.#move,
      airWeight: this.#air,
      turnRate: this.#turnRate,
      stridePhase: this.#stride,
      shufflePhase: this.#shuffle,
      time: this.#time,
    };
  }
}
