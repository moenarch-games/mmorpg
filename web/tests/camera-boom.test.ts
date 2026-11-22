import { expect, test } from "bun:test";
import { clearSightLine, type CameraBox, type CameraView } from "../src/world/orbit-camera";

const view: CameraView = { target: [0, 1, 0], eye: [0, 5, -10] };

// The boom's rules and their cases live with the shared camera (`mmorpg-client-model`, #1058);
// this checks the browser's adapter passes views and boxes through.
test("a wall behind the character pulls the eye in front of it, and a box aside does not", () => {
  const wall: CameraBox = { min: [-5, 0, -6], max: [5, 8, -5] };
  const { eye, target } = clearSightLine(view, [{ min: [5, 0, -5], max: [6, 8, 5] }, wall]);
  expect(target).toEqual(view.target);
  expect(eye[2]).toBeGreaterThan(-5);
  expect(eye[2]).toBeLessThan(0);
  expect(clearSightLine(view, [{ min: [5, 0, -5], max: [6, 8, 5] }])).toEqual(view);
});
