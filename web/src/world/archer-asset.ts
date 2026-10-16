import type { IndexedMeshGeometry, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import archerObj from "../../assets/medieval-character-kit/archer.obj?raw";
import manifest from "../../assets/medieval-character-kit/manifest.json";
import materials from "../../assets/medieval-character-kit/materials.json";
import type { HumanoidLook, UnitPlacement } from "./humanoid";

type Point = [number, number, number];
type PartMesh = { name: string; geometry: IndexedMeshGeometry };

const entry = manifest.assets.find((asset) => asset.id === "medieval.archer");
if (!entry || manifest.coordinateSystem !== "right-handed-y-up" || manifest.unit !== "millimeter" || manifest.consumerScaleToMeters !== "0.001") {
  throw new Error("incompatible asset-tooling archer package");
}
const asset = entry;

/** Lower the packaged, triangle-only OBJ groups once into 3d-lab's indexed mesh contract. */
export function lowerArcherObj(obj: string): PartMesh[] {
  const vertices: Point[] = [];
  const groups: { name: string; faces: number[][] }[] = [];
  for (const line of obj.split("\n")) {
    if (line.startsWith("v ")) {
      const coordinates = line.slice(2).trim().split(/\s+/).map(Number);
      if (coordinates.length !== 3 || coordinates.some((value) => !Number.isFinite(value))) {
        throw new Error("invalid archer OBJ vertex");
      }
      vertices.push(coordinates as Point);
    } else if (line.startsWith("g ")) {
      groups.push({ name: line.slice(2).trim(), faces: [] });
    } else if (line.startsWith("f ")) {
      const indices = line.slice(2).trim().split(/\s+/).map(Number);
      if (groups.length === 0 || indices.length !== 3 || indices.some((value) => !Number.isSafeInteger(value) || value < 1 || value > vertices.length)) {
        throw new Error("invalid archer OBJ face");
      }
      groups[groups.length - 1]!.faces.push(indices);
    } else if (line && !line.startsWith("#") && !line.startsWith("o ")) {
      throw new Error(`unsupported archer OBJ statement: ${line}`);
    }
  }
  if (vertices.length !== asset.vertexCount || groups.map((group) => group.name).join("|") !== asset.parts.join("|") ||
      groups.reduce((count, group) => count + group.faces.length, 0) !== asset.triangleCount) {
    throw new Error("archer OBJ topology differs from its asset-tooling manifest");
  }

  return groups.map((group) => {
    const positions: Point[] = [];
    const normals: Point[] = [];
    const indices: number[] = [];
    for (const face of group.faces) {
      const [a, b, c] = face.map((index) => vertices[index - 1]!) as [Point, Point, Point];
      const u: Point = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
      const v: Point = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
      const normal: Point = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
      const length = Math.hypot(...normal);
      if (length === 0) {
        throw new Error(`degenerate archer OBJ face in ${group.name}`);
      }
      const unit: Point = normal.map((value) => value / length) as Point;
      for (const point of [a, b, c]) {
        indices.push(positions.length);
        positions.push(point.map((value) => value * 0.001) as Point);
        normals.push(unit);
      }
    }
    return { name: group.name, geometry: { kind: "mesh", resourceKey: `asset-tooling:${asset.sha256}:${group.name}`, positions, normals, indices } };
  });
}

export const ARCHER_MESHES = lowerArcherObj(archerObj);
const neutral = materials.palettes.find((palette) => palette.id === "neutral")!;
const materialColors = new Map(neutral.materials.map((material) => [material.id, `#${material.baseColorSrgb8.slice(0, 3).map((channel) => channel.toString(16).padStart(2, "0")).join("")}`]));

/**
 * Asset geometry is presentation only; the projected player still determines placement. The
 * asset is one rigid mesh, so an action pose reaches it only as `turn`: extra yaw in radians
 * that turns the whole archer side-on, like the humanoid torso twist.
 */
export function archerNodes(id: string, placement: UnitPlacement, look: HumanoidLook, bob: number, turn = 0): RendererSceneNode[] {
  const halfYaw = (placement.yawRadians + turn) / 2;
  const rotationQuaternion: [number, number, number, number] = [0, Math.sin(halfYaw), 0, Math.cos(halfYaw)];
  return ARCHER_MESHES.map(({ name, geometry }) => {
    const role = materials.bindings.archer[name as keyof typeof materials.bindings.archer];
    if (!role) {
      throw new Error(`missing archer material binding for ${name}`);
    }
    const color = role === "cloth-primary" ? look.visuals.bodyColor : role === "cloth-secondary" ? look.visuals.chestColor : materialColors.get(role);
    if (!color) {
      throw new Error(`missing archer material ${role}`);
    }
    return {
      id: `${id}-${name}`,
      geometry,
      color: color as `#${string}`,
      transform: { translation: [placement.x, placement.feetY + bob, placement.z], rotationQuaternion },
    };
  });
}
