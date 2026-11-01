import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { mergeUnitMeshes } from "../src/world/units/merged-unit";

// A unit's meshes merge per material class (#615) and still follow its joints, holders and colours.
describe("mergeUnitMeshes", () => {
  /** A two-joint arm with a skinned body (two materials of one class), a rigid plate under a holder and a glass. */
  const unit = () => {
    const model = new THREE.Group();
    const root = new THREE.Bone();
    root.name = "root";
    const elbow = new THREE.Bone();
    elbow.name = "elbow";
    elbow.position.set(0, 1, 0);
    root.add(elbow);
    model.add(root);
    model.updateMatrixWorld(true);
    const skeleton = new THREE.Skeleton([root, elbow]);
    const skinned = (color: number, offset: number, roughness = 0.8) => {
      const geometry = new THREE.BoxGeometry(0.2, 2, 0.2, 1, 4, 1).translate(offset, 1, 0);
      const position = geometry.getAttribute("position");
      const indices: number[] = [];
      const weights: number[] = [];
      for (let i = 0; i < position.count; i++) {
        const w = Math.min(1, Math.max(0, position.getY(i) - 0.5));
        indices.push(0, 1, 0, 0);
        weights.push(1 - w, w, 0, 0);
      }
      geometry.setAttribute("skinIndex", new THREE.Uint16BufferAttribute(indices, 4));
      geometry.setAttribute("skinWeight", new THREE.Float32BufferAttribute(weights, 4));
      const mesh = new THREE.SkinnedMesh(geometry, new THREE.MeshStandardMaterial({ color, roughness }));
      mesh.name = `body-${offset}`;
      // As SkeletonUtils.clone leaves them: each skin its own skeleton over the same joints.
      mesh.skeleton = skeleton.clone();
      model.add(mesh);
      mesh.bind(skeleton, mesh.matrixWorld);
      return mesh;
    };
    const left = skinned(0xff0000, -0.3);
    const right = skinned(0x00ff00, 0.3);
    // Drawn from both sides: another material class.
    const shiny = skinned(0xffffff, 0.9);
    (shiny.material as THREE.Material).side = THREE.DoubleSide;
    const holder = new THREE.Group();
    holder.position.set(0, 0.5, 0.2);
    elbow.add(holder);
    const plate = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.3, 0.05), new THREE.MeshStandardMaterial({ color: 0x0000ff, roughness: 0.8 }));
    plate.name = "plate";
    holder.add(plate);
    const glass = new THREE.Mesh(new THREE.BoxGeometry(0.1, 0.1, 0.1), new THREE.MeshStandardMaterial({ transparent: true, opacity: 0.4 }));
    elbow.add(glass);
    return { model, elbow, holder, shiny, sources: [left, right, plate] };
  };

  /** The world positions of every drawn vertex, sorted, after posing the elbow and moving the holder. */
  const drawn = (model: THREE.Object3D, elbow: THREE.Object3D, holder: THREE.Object3D) => {
    elbow.rotation.set(0, 0, 0.7);
    holder.position.set(0.1, 0.4, 0.3);
    model.updateMatrixWorld(true);
    const out: string[] = [];
    const point = new THREE.Vector3();
    model.traverse((object) => {
      if (!(object instanceof THREE.Mesh) || (object.material as THREE.Material).transparent) { return; }
      if (object instanceof THREE.SkinnedMesh) { object.skeleton.update(); }
      const count = object.geometry.getAttribute("position").count;
      for (let i = 0; i < count; i++) {
        object.getVertexPosition(i, point);
        point.applyMatrix4(object.matrixWorld);
        out.push(point.toArray().map((value) => value.toFixed(4)).join(","));
      }
    });
    return out.sort();
  };

  test("one skinned mesh draws the opaque class, posed where its sources were, with their colours", () => {
    const before = unit();
    const expected = drawn(before.model, before.elbow, before.holder);
    const after = unit();
    const { meshes: merged, skeleton } = mergeUnitMeshes(after.model, new Set());
    expect(merged).toHaveLength(1);
    const meshes: THREE.Mesh[] = [];
    after.model.traverse((object) => { if (object instanceof THREE.Mesh) { meshes.push(object); } });
    // The merged mesh, the two-sided skin and the glass, which blends: the latter two stay apart.
    expect(meshes).toHaveLength(3);
    // Both skinned draws pose from one joint palette.
    expect(merged[0]!.skeleton).toBe(skeleton);
    expect(after.shiny.skeleton).toBe(skeleton);
    expect(after.model.getObjectByName("plate")?.parent).toBe(after.holder);
    expect(drawn(after.model, after.elbow, after.holder)).toEqual(expected);
    const color = merged[0]!.geometry.getAttribute("color");
    const colors = new Set(Array.from({ length: color.count }, (_, i) => `${color.getX(i)},${color.getY(i)},${color.getZ(i)}`));
    expect(colors).toEqual(new Set(["1,0,0", "0,1,0", "0,0,1"]));
    expect((merged[0]!.material as THREE.MeshStandardMaterial).color.getHex()).toBe(0xffffff);
  });

  test("a kept holder's meshes stay their own draws", () => {
    const { model, holder } = unit();
    const { meshes: merged } = mergeUnitMeshes(model, new Set([holder]));
    expect(merged).toHaveLength(1);
    expect(holder.children[0]).toBeInstanceOf(THREE.Mesh);
  });

  test("plain materials of several metallic and roughness factors read them from a palette", () => {
    const model = new THREE.Group();
    const surfaces = [[0.32, 1], [0.72, 0], [0.32, 1]].map(([roughness, metalness]) => {
      const mesh = new THREE.Mesh(new THREE.BoxGeometry(0.1, 0.1, 0.1), new THREE.MeshStandardMaterial({ roughness, metalness }));
      model.add(mesh);
      return mesh;
    });
    const { meshes } = mergeUnitMeshes(model, new Set());
    expect(meshes).toHaveLength(1);
    const material = meshes[0]!.material as THREE.MeshStandardMaterial;
    expect([material.roughness, material.metalness]).toEqual([1, 1]);
    const palette = material.roughnessMap as THREE.DataTexture;
    expect(material.metalnessMap).toBe(palette);
    expect(Array.from(palette.image.data as Float32Array)).toEqual([0, Math.fround(0.32), 1, 1, 0, Math.fround(0.72), 0, 1]);
    // Every vertex of a mesh samples its pair's texel centre.
    const uv = meshes[0]!.geometry.getAttribute("uv");
    const count = surfaces[0]!.geometry.getAttribute("position").count;
    expect([0, 1, 2].map((k) => uv.getX(k * count + 3))).toEqual([0.25, 0.75, 0.25]);
  });

  test("plates with their own occlusion maps share one tiled atlas", () => {
    // 16×16 BC7-like maps (16-byte 4×4 blocks), every block of plate k filled with k + 1, down to 4×4.
    const occlusion = (k: number) => {
      const mipmaps = [16, 8, 4, 2, 1].map((size) => {
        const blocks = Math.max(1, size / 4) ** 2;
        return { data: new Uint8Array(blocks * 16).fill(k + 1), width: size, height: size };
      });
      return new THREE.CompressedTexture(mipmaps, 16, 16, THREE.RGBA_BPTC_Format);
    };
    const model = new THREE.Group();
    const plates = [0, 1, 2].map((k) => {
      const plate = new THREE.Mesh(new THREE.PlaneGeometry(0.2, 0.2), new THREE.MeshStandardMaterial({ aoMap: occlusion(k), metalness: 1 }));
      plate.position.x = k;
      model.add(plate);
      return plate;
    });
    const { meshes } = mergeUnitMeshes(model, new Set());
    expect(meshes).toHaveLength(1);
    const atlas = (meshes[0]!.material as THREE.MeshStandardMaterial).aoMap as THREE.CompressedTexture;
    // Two columns, two rows of 16×16 tiles; levels down to one block per tile.
    const levels = atlas.mipmaps as { data: Uint8Array; width: number; height: number }[];
    expect(levels.map((level) => [level.width, level.height])).toEqual([[32, 32], [16, 16], [8, 8]]);
    // Level 0: 8 blocks across; block rows 0–3 hold plates 1 and 2, rows 4–7 plate 3 and an empty tile.
    const block = (x: number, y: number) => levels[0]!.data[(y * 8 + x) * 16];
    expect([block(0, 0), block(4, 0), block(3, 3), block(7, 3), block(0, 4), block(4, 4)]).toEqual([1, 2, 1, 2, 3, 0]);
    // Each plate's UVs land in its tile.
    const uv = meshes[0]!.geometry.getAttribute("uv");
    const ranges = [0, 1, 2].map((k) => {
      const count = plates[k]!.geometry.getAttribute("uv").count;
      const us = Array.from({ length: count }, (_, i) => uv.getX(k * count + i));
      const vs = Array.from({ length: count }, (_, i) => uv.getY(k * count + i));
      return [Math.min(...us), Math.max(...us), Math.min(...vs), Math.max(...vs)];
    });
    expect(ranges).toEqual([[0, 0.5, 0, 0.5], [0.5, 1, 0, 0.5], [0, 0.5, 0.5, 1]]);
  });
});
