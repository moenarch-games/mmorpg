import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { Grass } from "../src/world/grass";

describe("grass visibility during third-person camera movement", () => {
  test("orbiting a stationary character does not change grass residency, density or fade centre", () => {
    const grass = new Grass({
      cellMetres: 12,
      cell: (column, row) => column === 0 && row === 0
        ? new Float32Array([6, 0, 6, 0, 0.75, 0.25, 0.6, 0.2, 0])
        : new Float32Array(),
    }, "medium");
    const feet: [number, number, number] = [0, 0, 0];
    try {
      // Allow the bounded per-frame cell streamer to fill the stationary neighbourhood.
      for (let frame = 0; frame < 40; frame += 1) {
        grass.update([0, 4, 8], feet, frame / 60);
      }
      const before = grass.observe();
      expect(before.drawn).toBeGreaterThan(0);
      grass.update([20, 4, -8], feet, 41 / 60);
      expect(grass.observe()).toEqual(before);

      const mesh = grass.root.children.find((object): object is THREE.InstancedMesh => object instanceof THREE.InstancedMesh);
      expect(mesh).toBeDefined();
      const uniform = (mesh!.material as THREE.ShaderMaterial).uniforms.grassEye as { value: THREE.Vector3 };
      expect(uniform.value.toArray()).toEqual(feet);

      // Without an avatar (e.g. a free camera), keep the eye as the fallback anchor.
      grass.update([12, 4, 0], null, 42 / 60);
      expect(uniform.value.toArray()).toEqual([12, 4, 0]);
    } finally {
      grass.dispose();
    }
  });
});
