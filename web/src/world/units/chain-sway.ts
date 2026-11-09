import * as THREE from "three";

/** A bundle's accessory-bone chain (Atlas 6.6.39, asset-tooling #391): its joints from the root and its tip's bound in radians. */
export type DeclaredChain = { readonly joints: readonly string[]; readonly limit: number };

/** One chain of a model: its joints, whose unswung rotations stay the spring's rest targets, and its bound. */
type BoundChain = { joints: THREE.Object3D[]; rests: THREE.Quaternion[]; limit: number };

/** Longest declaration per root joint, as `units::sway::chains_of` keeps per figure. */
const declared = new Map<string, DeclaredChain>();

/** Adds a loaded figure's declared chains to the table `ChainSway.of` binds from. */
export function declareChains(chains: readonly DeclaredChain[]): void {
  for (const chain of chains) {
    const root = chain.joints[0];
    if (root === undefined) { continue; }
    const kept = declared.get(root);
    if (!kept || chain.joints.length > kept.joints.length) { declared.set(root, chain); }
  }
}

const modelInverse = new THREE.Matrix4();
const parentInModel = new THREE.Matrix4();
const parentPosition = new THREE.Vector3();
const parentScale = new THREE.Vector3();
const parentTurn = new THREE.Quaternion();
const parentTurnInverse = new THREE.Quaternion();
const turn = new THREE.Quaternion();

/**
 * Cosmetic chain sway on one model (Atlas 6.6.39, #498): the joints the bundles declare, turned
 * every frame by the shared spring's drive about whatever rotation the mixer gave them, so the
 * spring rests on a clip's keyed `secondaryPose` weighted by its blend and else on the bind pose
 * (`units::sway`). Mixers write a joint only when its value changes and capture a binding's
 * original value on first use, so the joints' unswung rotations are restored before each pose.
 */
export class ChainSway {
  readonly #chains: readonly BoundChain[];
  #swung = false;

  private constructor(chains: readonly BoundChain[]) {
    this.#chains = chains;
  }

  /** Binds `model` to the declared chains it has (each chain's prefix of present joints), or null with none. */
  static of(model: THREE.Object3D, chains: Iterable<DeclaredChain> = declared.values()): ChainSway | null {
    const bound: BoundChain[] = [];
    for (const chain of chains) {
      const joints: THREE.Object3D[] = [];
      for (const name of chain.joints) {
        const joint = model.getObjectByName(THREE.PropertyBinding.sanitizeNodeName(name));
        if (!joint) { break; }
        joints.push(joint);
      }
      if (joints.length > 0) { bound.push({ joints, rests: joints.map((joint) => joint.quaternion.clone()), limit: chain.limit }); }
    }
    return bound.length > 0 ? new ChainSway(bound) : null;
  }

  /** Puts the joints back to their mixer rotations: call before posing. */
  beforePose(): void {
    if (!this.#swung) { return; }
    this.#swung = false;
    for (const { joints, rests } of this.#chains) {
      joints.forEach((joint, index) => joint.quaternion.copy(rests[index]!));
    }
  }

  /**
   * Swings the joints by `drive` (`[pitch, roll]` by depth from each chain's root, as a share of its
   * bound) about their rotations this frame, in `model`'s frame; null leaves them as posed. Call after
   * posing and placing the model; a distant or settled unit costs nothing here.
   */
  apply(model: THREE.Object3D, drive: readonly number[] | null): void {
    if (!drive) { return; }
    model.updateWorldMatrix(true, false);
    modelInverse.copy(model.matrixWorld).invert();
    for (const { joints, rests, limit } of this.#chains) {
      const share = limit / joints.length;
      joints[0]!.parent?.updateWorldMatrix(true, false);
      joints.forEach((joint, depth) => {
        const rest = rests[depth]!;
        rest.copy(joint.quaternion);
        const pitch = ((drive[depth * 2] ?? 0) * share) / 2;
        const roll = ((drive[depth * 2 + 1] ?? 0) * share) / 2;
        const sp = Math.sin(pitch), cp = Math.cos(pitch), sr = Math.sin(roll), cr = Math.cos(roll);
        // `Swing::quaternion`: roll × pitch.
        turn.set(cr * sp, sr * sp, sr * cp, cr * cp);
        parentInModel.multiplyMatrices(modelInverse, joint.parent!.matrixWorld);
        parentInModel.decompose(parentPosition, parentTurn, parentScale);
        parentTurnInverse.copy(parentTurn).invert();
        joint.quaternion.copy(parentTurnInverse).multiply(turn).multiply(parentTurn).multiply(rest);
        joint.updateWorldMatrix(false, false);
      });
    }
    this.#swung = true;
  }
}
