import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { generateMissingTangents } from "../src/world/static-bundle";

// A unit quad in the XY plane, indexed, facing +Z, its UVs following X and Y as glTF's do.
function quad(): THREE.BufferGeometry {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0]), 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1]), 3));
  geometry.setAttribute("uv", new THREE.BufferAttribute(new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]), 2));
  geometry.setIndex([0, 1, 2, 0, 2, 3]);
  return geometry;
}

function gltfOf(...meshes: THREE.Mesh[]): GLTF {
  const scene = new THREE.Group();
  scene.add(...meshes);
  return { scene } as unknown as GLTF;
}

// asset-tooling's `greyhaven-nature` bark and rocks ship no TANGENT and leave it to the consumer (#53).
describe("generateMissingTangents", () => {
  test("a normal-mapped primitive without tangents gets MikkTSpace tangents and stays indexed", async () => {
    const mesh = new THREE.Mesh(quad(), new THREE.MeshStandardMaterial({ normalMap: new THREE.Texture() }));
    await generateMissingTangents([gltfOf(mesh)]);
    const tangents = mesh.geometry.getAttribute("tangent");
    expect(tangents.itemSize).toBe(4);
    expect(mesh.geometry.index).not.toBeNull();
    expect(mesh.geometry.getAttribute("position").count).toBe(4);
    for (let vertex = 0; vertex < tangents.count; vertex += 1) {
      expect([tangents.getX(vertex), tangents.getY(vertex), tangents.getZ(vertex)]).toEqual([1, 0, 0]);
      // glTF: bitangent = cross(normal, tangent.xyz) * w points up the image, so V decreases along it.
      expect(tangents.getW(vertex)).toBe(-1);
    }
  });

  test("the producer's tangents and meshes without a normal map are left alone", async () => {
    const authored = quad();
    const shipped = new THREE.BufferAttribute(new Float32Array([0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]), 4);
    authored.setAttribute("tangent", shipped);
    const tangentMesh = new THREE.Mesh(authored, new THREE.MeshStandardMaterial({ normalMap: new THREE.Texture() }));
    const plain = new THREE.Mesh(quad(), new THREE.MeshStandardMaterial());
    await generateMissingTangents([gltfOf(tangentMesh, plain)]);
    expect(tangentMesh.geometry).toBe(authored);
    expect(authored.getAttribute("tangent")).toBe(shipped);
    expect(plain.geometry.hasAttribute("tangent")).toBe(false);
  });
});
