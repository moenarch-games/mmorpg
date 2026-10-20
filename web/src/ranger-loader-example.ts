import * as THREE from "three";
import { createThreeSceneRenderer, type Matrix4Values } from "@moritzbrantner/three-d-renderer";
import { QuaterniusUnits } from "./world/units/quaternius-units";

const canvas = document.querySelector("canvas");
const status = document.querySelector("#status");
if (!canvas || !status) throw new Error("Ranger example elements missing");
const renderer = createThreeSceneRenderer(canvas, { antialias: true });
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
  const duration = clips.find(([name]) => name === "Walk_Loop")?.[1];
  if (!duration) throw new Error("Shared Walk_Loop clip missing");
  status.textContent = `Default Ranger: ${clips.length} shared clips. Walking with bow and quiver.`;
  const start = performance.now();
  function frame(now: number): void {
    renderer.setSize(innerWidth, innerHeight * 0.9, devicePixelRatio);
    camera.aspect = innerWidth / (innerHeight * 0.9);
    camera.updateProjectionMatrix();
    models.update([{ id: "ranger-example", kind: "Ranger", entity: { kind: "player", id: 1 },
      feet: [0, 0, 0], yaw: 0, heightMetres: 1.8, clip: "Walk_Loop", time: ((now - start) / 1000) % duration!,
      tint: null, swordAndShield: false, bowAndQuiver: true, headwear: false }]);
    const view = { viewMatrix: matrix(camera.matrixWorldInverse), projectionMatrix: matrix(camera.projectionMatrix) };
    renderer.render({ camera: view, nodes: [] });
    renderer.renderImportedRoots({ camera: view, roots: models.roots().roots });
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
} else {
  status.textContent = "Ranger loading failed. Run scripts/fetch-bundles.sh and check the browser console.";
}
