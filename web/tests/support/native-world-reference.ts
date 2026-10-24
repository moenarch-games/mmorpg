/** #182 reference: the Pages presentation path of the shared static world (#184), through the real WASM export. */
import * as THREE from "three";
import type { RendererInstance, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { ENVIRONMENT, DAY_SECONDS, environmentAt, skyAt } from "../../src/world/environment";
import { decodeScenery } from "../../src/world/scenery";
import { SceneryFrame, buildSceneryScene } from "../../src/world/scenery-nodes";
import { localZoneModule } from "./local-zone-module";

const input = JSON.parse(await Bun.stdin.text());
const scenery = decodeScenery(JSON.stringify(input.scenery));
const resources: Record<string, unknown> = {};
const resourceObjects = new WeakMap<object, string>();
const wasm = await localZoneModule();
const staticScene = buildSceneryScene(scenery, wasm);
const encoder = new TextEncoder();
function digest(positions: readonly (readonly number[])[], indices: readonly number[], normals?: readonly (readonly number[])[], colors?: readonly (readonly number[])[]): string {
  let hash = 14695981039346656037n;
  const byte = (value: number) => { hash = BigInt.asUintN(64, (hash ^ BigInt(value)) * 1099511628211n); };
  const number = (value: number, integer: boolean) => {
    const bytes = new ArrayBuffer(4); const view = new DataView(bytes);
    if (integer) { view.setUint32(0, value, true); } else { view.setFloat32(0, value, true); }
    for (const value of new Uint8Array(bytes)) { byte(value); }
  };
  for (const [name, values] of [["positions", positions], ["normals", normals], ["colors", colors]] as const) {
    for (const value of encoder.encode(name)) { byte(value); }
    byte(values ? 1 : 0);
    if (values) {
      number(values.length, true);
      for (const vector of values) { for (const value of vector) { number(value, false); } }
    }
  }
  number(indices.length, true);
  for (const value of indices) { number(value, true); }
  return hash.toString(16).padStart(16, "0");
}
function mesh(geometry: RendererSceneNode["geometry"]): string {
  const cached = resourceObjects.get(geometry);
  if (cached) { return cached; }
  let positions: readonly (readonly number[])[];
  let normals: readonly (readonly number[])[] | undefined;
  let colors: readonly (readonly number[])[] | undefined;
  let indices: readonly number[];
  if (geometry.kind === "mesh") {
    ({ positions, normals, colors, indices } = geometry);
  } else {
    // These are the exact tessellation arguments in the pinned renderer/index.js.
    let primitive: THREE.BufferGeometry;
    if (geometry.kind === "box") { primitive = new THREE.BoxGeometry(...geometry.size); }
    else if (geometry.kind === "sphere") { primitive = new THREE.SphereGeometry(geometry.radius, 20, 12); }
    else { primitive = new THREE.CylinderGeometry(geometry.radius, geometry.radius, geometry.height, 20); }
    const vectors = (name: string) => {
      const attr = primitive.getAttribute(name);
      return Array.from({ length: attr.count }, (_, index) => [attr.getX(index), attr.getY(index), attr.getZ(index)]);
    };
    positions = vectors("position"); normals = vectors("normal");
    indices = Array.from(primitive.index!.array); primitive.dispose();
  }
  const hash = digest(positions, indices, normals, colors);
  const key = `mesh:${hash}`;
  resources[key] = { digest: hash, vertices: positions.length, indices: indices.length,
    bounds: [0, 1, 2].map((axis) => positions.reduce(([low, high], p) => [Math.min(low!, p[axis]!), Math.max(high!, p[axis]!)], [Infinity, -Infinity])) };
  resourceObjects.set(geometry, key);
  return key;
}
function rgb(value: string): number[] {
  const hex = Number.parseInt(value.slice(1), 16);
  return [hex >> 16 & 255, hex >> 8 & 255, hex & 255].map((channel) => channel / 255);
}
function draw(node: RendererSceneNode, id = node.id) {
  const matrix = new THREE.Matrix4();
  if (node.modelMatrix) { matrix.fromArray(node.modelMatrix); }
  else {
    matrix.compose(new THREE.Vector3(...node.transform.translation), new THREE.Quaternion(...(node.transform.rotationQuaternion ?? [0, 0, 0, 1])).normalize(), new THREE.Vector3(...(node.transform.scale ?? [1, 1, 1])));
  }
  return { id, reference_resource: mesh(node.geometry), source_key: node.geometry.kind === "mesh" ? node.geometry.resourceKey : null, model: matrix.elements, material: {
    color: rgb(node.color), opacity: node.opacity ?? 1, unlit: node.unlit ?? false,
    emissive: node.emissive ? rgb(node.emissive) : [0, 0, 0], double_sided: node.doubleSided ?? false,
  } };
}
const scenes: unknown[] = [];
for (const request of input.requests) {
  if (request.kind === "world") {
    const frame = new SceneryFrame(staticScene).nodes(request.eye, { seconds: request.seconds, animate: request.animate, outpostKit: true });
    const draws = frame.nodes.filter((node) => node.visible !== false).map((node) => draw(node));
    for (const batch of frame.instanceBatches) {
      if (batch.visible === false) { continue; }
      batch.instances.forEach((instance: RendererInstance, index: number) => draws.push(draw({ ...batch, ...instance, id: `${batch.id}:${index}`, color: instance.color ?? batch.color }, `${batch.id}:${index}`)));
    }
    const phase = request.animate ? request.seconds / DAY_SECONDS : 0;
    const environment = environmentAt(ENVIRONMENT, phase);
    scenes.push({ name: request.name, draws, environment, sky_stops: skyAt(ENVIRONMENT, phase) });
  }
}
console.log(JSON.stringify({ resources, scenes }));
