import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { applyFace, faceLookOf } from "../src/world/units/quaternius-units";

const option = (id: string, morphs: Record<string, number> = {}) => ({ id, label: id, morphs });
const appearance = {
  face: {
    nodes: ["SuperHero_Male", "Eyes"],
    variants: ["default"],
    features: [
      { id: "face", label: "Face shape", default: "face.base", options: [option("face.base"), option("face.broad", { "face.broad": 1 })] },
      { id: "eyes", label: "Eyes", default: "eyes.base", options: [option("eyes.base"), option("eyes.large", { "eyes.large": 1 })] },
    ],
  },
};

function figure(targets: string[]): { model: THREE.Group; head: THREE.Mesh; eyes: THREE.Mesh } {
  const mesh = (name: string) => {
    const result = new THREE.Mesh(new THREE.BufferGeometry(), new THREE.MeshStandardMaterial());
    result.name = name;
    result.morphTargetDictionary = Object.fromEntries(targets.map((target, index) => [target, index]));
    result.morphTargetInfluences = targets.map(() => 0.5);
    return result;
  };
  const model = new THREE.Group();
  const head = mesh("SuperHero_Male");
  const eyes = mesh("Eyes");
  model.add(head, eyes);
  return { model, head, eyes };
}

describe("face morphs (#411)", () => {
  test("applies the chosen options on every face node after resetting the others", () => {
    const { model, head, eyes } = figure(["face.broad", "eyes.large"]);
    const look = faceLookOf(appearance, "default", model)!;
    expect(look.features.map((feature) => feature.id)).toEqual(["face", "eyes"]);
    applyFace(model, look, { face: "face.broad", eyes: "not-offered" });
    expect(head.morphTargetInfluences).toEqual([1, 0]);
    expect(eyes.morphTargetInfluences).toEqual([1, 0]);
  });

  test("an unsupported variant or a model without the targets keeps the base face", () => {
    expect(faceLookOf(appearance, "lod1", figure(["face.broad", "eyes.large"]).model)).toBeNull();
    expect(faceLookOf(appearance, "default", figure([]).model)).toBeNull();
    expect(faceLookOf({}, "default", figure([]).model)).toBeNull();
  });
});
