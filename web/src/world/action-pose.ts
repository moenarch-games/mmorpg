import { POSE_LIMITS, type ArmPose, type Pose } from "./character-animation";
import type { ViewerAction } from "./units/spell-effects";

/**
 * Cast, draw and swing poses laid over the locomotion pose of the viewer's humanoid: arms raised
 * to cast, the bow arm out and the string arm back to draw, and a weapon swung down in a strike.
 * A pure function of the base pose and the action; every value stays within `POSE_LIMITS`.
 */
function clamp(value: number, [low, high]: readonly [number, number]): number {
  return Math.min(high, Math.max(low, value));
}

function mix(from: number, to: number, weight: number): number {
  return from + (to - from) * weight;
}

function blendArm(base: ArmPose, target: ArmPose, weight: number): ArmPose {
  return {
    swing: clamp(mix(base.swing, target.swing, weight), POSE_LIMITS.armSwing),
    spread: clamp(mix(base.spread, target.spread, weight), POSE_LIMITS.armSpread),
    elbow: clamp(mix(base.elbow, target.elbow, weight), POSE_LIMITS.elbow),
  };
}

/** How strongly the pose applies: a cast ramps in and stays; an instant ability swells and releases. */
export function actionWeight(action: ViewerAction): number {
  const progress = Math.min(1, Math.max(0, action.progress));
  return action.hold ? Math.min(1, progress * 5) : Math.sin(Math.PI * progress);
}

export function applyActionPose(base: Pose, action: ViewerAction | null): Pose {
  if (!action) {
    return base;
  }
  const weight = actionWeight(action);
  switch (action.pose) {
    case "cast":
      return {
        ...base,
        lean: clamp(mix(base.lean, 0.06, weight), POSE_LIMITS.lean),
        headPitch: clamp(mix(base.headPitch, -0.12, weight), POSE_LIMITS.headPitch),
        leftArm: blendArm(base.leftArm, { swing: 1.15, spread: 0.25, elbow: 0.7 }, weight),
        rightArm: blendArm(base.rightArm, { swing: 1.7, spread: 0.2, elbow: 0.45 }, weight),
      };
    case "draw":
      return {
        ...base,
        twist: clamp(mix(base.twist, 0.3, weight), POSE_LIMITS.twist),
        leftArm: blendArm(base.leftArm, { swing: 1.55, spread: 0.1, elbow: 0.05 }, weight),
        rightArm: blendArm(base.rightArm, { swing: 0.95, spread: 0.3, elbow: 1.8 }, weight),
      };
    case "swing": {
      // Raised at the start, brought down through the strike.
      const strike = Math.min(1, Math.max(0, action.progress));
      const raised = 2.1 - 1.9 * strike;
      return {
        ...base,
        lean: clamp(mix(base.lean, 0.05 + 0.2 * strike, weight), POSE_LIMITS.lean),
        twist: clamp(mix(base.twist, -0.3 + 0.6 * strike, weight), POSE_LIMITS.twist),
        rightArm: blendArm(base.rightArm, { swing: raised, spread: 0.15, elbow: 0.5 + 0.7 * strike }, Math.min(1, weight * 1.4)),
      };
    }
  }
}
