import * as THREE from "three";
import { createThreeSceneRenderer, type Matrix4Values } from "@moritzbrantner/three-d-renderer";
import { QuaterniusUnits } from "./world/units/quaternius-units";
import { GreyhavenLighting, presentationMaterial } from "./world/lighting";
import { loadWasm } from "./wasm";

// The Ranger's appearance presets come from the shared presenter in mmorpg-wasm.
await loadWasm();

const canvas = document.querySelector("canvas");
const status = document.querySelector("#status");
if (!canvas || !status) throw new Error("Ranger example elements missing");
// Lit like the game (#600): the mid-afternoon sun, without fog or lamps.
const lighting = new GreyhavenLighting(import.meta.env.BASE_URL);
lighting.apply({ hour: 15, sky: false, fog: null, pointLights: null });
const renderer = createThreeSceneRenderer(canvas, {
  antialias: false, material: presentationMaterial, draw: (target, scene, view) => lighting.draw(target, scene, view),
});
const models = new QuaterniusUnits(() => "Idle_Loop");
models.configureRenderer(renderer);
const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 100);
camera.position.set(3, 1.6, 4);
camera.lookAt(0, 0.9, 0);
camera.updateMatrixWorld();
function matrix(matrix: THREE.Matrix4): Matrix4Values {
  const e = matrix.elements;
  return [e[0]!, e[1]!, e[2]!, e[3]!, e[4]!, e[5]!, e[6]!, e[7]!, e[8]!, e[9]!, e[10]!, e[11]!, e[12]!, e[13]!, e[14]!, e[15]!];
}
if (await models.preload("Ranger")) {
  const clips = models.drainLoaded().flatMap(([, clips]) => clips);
  // `?clip=Bow_Hold_Loop&time=0.4&yaw=1.57` poses one clip at a fixed time; a `Bow_` clip holds the
  // bow in hand with its nock drawn (#556).
  const query = new URLSearchParams(location.search);
  const clip = query.get("clip") ?? "Walk_Loop";
  const fixed = query.has("time") ? Number(query.get("time")) : null;
  const yaw = Number(query.get("yaw") ?? 0);
  const duration = clips.find(([name]) => name === clip)?.[1];
  if (!duration) throw new Error(`Shared ${clip} clip missing`);
  status.textContent = `Default Ranger: ${clips.length} shared clips. ${clip} with bow and quiver.`;
  const start = performance.now();
  function frame(now: number): void {
    renderer.setSize(innerWidth, innerHeight * 0.9, devicePixelRatio);
    camera.aspect = innerWidth / (innerHeight * 0.9);
    camera.updateProjectionMatrix();
    const time = fixed ?? ((now - start) / 1000) % duration!;
    models.update([{ id: "ranger-example", kind: "Ranger", entity: { kind: "player", id: 1 },
      feet: [0, 0, 0], yaw, pitch: 0, heightMetres: 1.8, clip, time, lower: [clip, time],
      layers: [{ clip, time, weight: 1, mask: "all" }],
      tint: null, loadout: "bow-and-quiver", resident: null, figure: null, variety: null, outfit: null, headwear: false, swing: null,
      bow: clip.startsWith("Bow_") ? { clip, time } : null, sheathed: [false, false], throwable: null, belted: null, crossbow: null, channel: null }], { headwear: "none", hair: { style: "parted", color: "brown" }, face: {} });
    const view = { viewMatrix: matrix(camera.matrixWorldInverse), projectionMatrix: matrix(camera.projectionMatrix) };
    renderer.render({ camera: view, nodes: [] });
    renderer.renderImportedRoots({ camera: view, roots: models.roots().roots });
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
} else {
  status.textContent = "Ranger loading failed. Run scripts/fetch-bundles.sh and check the browser console.";
}
