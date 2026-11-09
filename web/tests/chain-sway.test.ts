import { describe, expect, it } from "bun:test";
import * as THREE from "three";
import { ChainSway, type DeclaredChain } from "../src/world/units/chain-sway";

const limit = THREE.MathUtils.degToRad(20);
const declared: DeclaredChain[] = [{ joints: ["skirt_f_01", "skirt_f_02"], limit }];

/** Model -> pelvis -> skirt_f_01 -> skirt_f_02 with a bind pose on the first joint. */
function rig() {
  const root = new THREE.Group();
  const model = new THREE.Group();
  root.add(model);
  const pelvis = new THREE.Bone(); pelvis.name = "pelvis";
  const skirt = new THREE.Bone(); skirt.name = "skirt_f_01";
  skirt.quaternion.setFromAxisAngle(new THREE.Vector3(1, 0, 0), -0.28);
  const tip = new THREE.Bone(); tip.name = "skirt_f_02";
  model.add(pelvis); pelvis.add(skirt); skirt.add(tip);
  const sway = ChainSway.of(model, declared)!;
  return { root, model, pelvis, skirt, tip, sway, bind: skirt.quaternion.clone() };
}

const keyedTurn = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), 0.9);
function keyed(): THREE.AnimationClip {
  return new THREE.AnimationClip("Sit", 1, [new THREE.QuaternionKeyframeTrack("skirt_f_01.quaternion", [0, 1], [...keyedTurn.toArray(), ...keyedTurn.toArray()])]);
}

/** The turn a drive `[pitch, 0]` asks of a joint in the two-joint test chain. */
function turnOf(drive: number): THREE.Quaternion {
  return new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), drive * limit / 2);
}

function expectQuaternion(actual: THREE.Quaternion, expected: THREE.Quaternion): void {
  // Sign-agnostic: q and -q are the same rotation.
  expect(Math.abs(actual.dot(expected))).toBeCloseTo(1, 5);
}

describe("chain sway on the mixer pose", () => {
  it("rests on the bind pose when no clip keys the chain, then settles back exactly", () => {
    const { model, skirt, sway, bind } = rig();
    sway.beforePose();
    sway.apply(model, [0.5, 0, 0, 0, 0, 0]);
    expectQuaternion(skirt.quaternion, bind.clone().premultiply(turnOf(0.5)));
    // A mixer never touched the joint, so it must not accumulate frame over frame.
    sway.beforePose();
    sway.apply(model, [0.5, 0, 0, 0, 0, 0]);
    expectQuaternion(skirt.quaternion, bind.clone().premultiply(turnOf(0.5)));
    sway.beforePose();
    sway.apply(model, null);
    expectQuaternion(skirt.quaternion, bind);
  });

  it("rests on a keyed clip's pose weighted by its blend, without drifting or corrupting the mixer's bind capture", () => {
    const { model, skirt, sway, bind } = rig();
    const mixer = new THREE.AnimationMixer(model);
    const action = mixer.clipAction(keyed()).play();
    action.setEffectiveWeight(0.5);
    const pose = () => { action.time = 0.25; mixer.update(0); };
    // The mixer first captures the original here, with the swing already applied last frame.
    sway.beforePose();
    pose();
    sway.apply(model, [0.5, 0, 0, 0, 0, 0]);
    const half = bind.clone().slerp(keyedTurn, 0.5);
    expectQuaternion(skirt.quaternion, half.clone().premultiply(turnOf(0.5)));
    // A frozen clip time: the mixer writes nothing, yet the swing does not stack on itself.
    sway.beforePose();
    pose();
    sway.apply(model, [0.25, 0, 0, 0, 0, 0]);
    expectQuaternion(skirt.quaternion, half.clone().premultiply(turnOf(0.25)));
    // Fully blended, the target is the keyed pose.
    action.setEffectiveWeight(1);
    sway.beforePose();
    pose();
    sway.apply(model, [0.5, 0, 0, 0, 0, 0]);
    expectQuaternion(skirt.quaternion, keyedTurn.clone().premultiply(turnOf(0.5)));
    // Swing gone and the clip faded out: the chain is back on bind exactly.
    action.setEffectiveWeight(0);
    sway.beforePose();
    action.stop();
    mixer.update(0);
    expectQuaternion(skirt.quaternion, bind);
  });

  it("does no joint work for a distant unit", () => {
    const { model, skirt, sway } = rig();
    const before = skirt.quaternion.clone();
    sway.beforePose();
    sway.apply(model, null);
    expect(skirt.quaternion.equals(before)).toBe(true);
    expect(ChainSway.of(new THREE.Group(), declared)).toBeNull();
  });

  it("turns in the model's frame through a rotated parent, and keeps the chain bound", () => {
    const { model, pelvis, skirt, tip, sway, bind } = rig();
    pelvis.quaternion.setFromAxisAngle(new THREE.Vector3(0, 1, 0), Math.PI / 2);
    sway.beforePose();
    sway.apply(model, [1, 0, 1, 0, 1, 0]);
    // The drive turns about the model's X: through the pelvis's quarter turn that is -Z locally.
    const pelvisInverse = pelvis.quaternion.clone().invert();
    expectQuaternion(skirt.quaternion, pelvisInverse.multiply(turnOf(1)).multiply(pelvis.quaternion).multiply(bind));
    // Across the whole chain the tip turned at most the limit.
    const total = skirt.quaternion.clone().multiply(bind.clone().invert()).multiply(tip.quaternion).angleTo(new THREE.Quaternion());
    expect(total).toBeLessThanOrEqual(limit + 1e-3);
  });
});
