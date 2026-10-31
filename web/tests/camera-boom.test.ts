import { expect, test } from "bun:test";
import { clearSightLine, type CameraBox, type CameraView } from "../src/world/orbit-camera";

const view: CameraView = { target: [0, 1, 0], eye: [0, 5, -10] };

test("an unobstructed view keeps its eye", () => {
  expect(clearSightLine(view, [{ min: [5, 0, -5], max: [6, 8, 5] }])).toEqual(view);
});

test("a wall behind the character pulls the eye in front of it", () => {
  const wall: CameraBox = { min: [-5, 0, -6], max: [5, 8, -5] };
  const { eye, target } = clearSightLine(view, [wall]);
  expect(target).toEqual(view.target);
  expect(eye[2]).toBeGreaterThan(-5);
  expect(eye[2]).toBeLessThan(0);
});

test("a roof over the character keeps the eye under it", () => {
  const roof: CameraBox = { min: [-10, 3, -10], max: [10, 3.5, 10] };
  expect(clearSightLine(view, [roof]).eye[1]).toBeLessThan(3);
});

test("a box around the character, such as the floor it stands on, never pulls the eye", () => {
  const floor: CameraBox = { min: [-10, -1, -10], max: [10, 1.4, 10] };
  expect(clearSightLine(view, [floor])).toEqual(view);
});
