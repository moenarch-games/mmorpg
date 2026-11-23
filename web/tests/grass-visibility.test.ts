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
  test("the full, simplified and distant blades share the same terrain-tinted color gradient", () => {
    const clump = new Float32Array([6, 0, 6, 0, 0.75, 0.25, 0.6, 0.2, 0]);
    const grass = new Grass({
      cellMetres: 8,
      cell: (column, row) => column === 0 && row === 0 ? clump : new Float32Array(),
      farCellMetres: 32,
      farCell: (column, row) => column === 0 && row === 0 ? clump : new Float32Array(),
    }, "high");
    try {
      grass.update([4, 4, 12], [4, 0, 4], 0);
      const meshes = grass.root.children.filter((child): child is THREE.InstancedMesh => child instanceof THREE.InstancedMesh);
      const near = meshes.filter((mesh) => mesh.name === "grass:0,0");
      const far = meshes.find((mesh) => mesh.name === "grass:far:0,0");
      expect(near).toHaveLength(2);
      expect(far).toBeDefined();
      const baseColor = (mesh: THREE.InstancedMesh) => {
        const attribute = mesh.geometry.getAttribute("color");
        return [attribute.getX(0), attribute.getY(0), attribute.getZ(0)];
      };
      expect(baseColor(near[0]!)).toEqual(baseColor(near[1]!));
      expect(baseColor(near[0]!)).toEqual(baseColor(far!));
      const shader = (near[0]!.material as THREE.ShaderMaterial).vertexShader;
      expect(shader).toContain("smoothstep(grassMix.z");
      expect(shader).toContain("smoothstep(grassMix.w");
      expect(shader).not.toContain("step(grassMix.z, jittered)");
    } finally {
      grass.dispose();
    }
  });

  test("reduced motion holds the wind still (#1079)", () => {
    const grass = new Grass({
      cellMetres: 12,
      cell: (column, row) => column === 0 && row === 0
        ? new Float32Array([6, 0, 6, 0, 0.75, 0.25, 0.6, 0.2, 0])
        : new Float32Array(),
    }, "medium");
    try {
      // The bounded per-frame streamer builds the cell over a few frames.
      for (let frame = 0; frame < 40; frame += 1) {
        grass.update([0, 4, 8], [0, 0, 0], 1);
      }
      const mesh = grass.root.children.find((object): object is THREE.InstancedMesh => object instanceof THREE.InstancedMesh);
      const time = (mesh!.material as THREE.ShaderMaterial).uniforms.grassTime as { value: number };
      expect(time.value).toBe(1);
      grass.update([0, 4, 8], [0, 0, 0], 2, false);
      expect(time.value).toBe(1);
      grass.update([0, 4, 8], [0, 0, 0], 3);
      expect(time.value).toBe(3);
    } finally {
      grass.dispose();
    }
  });
});
