import { describe, expect, test } from "bun:test";
import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { poseFor, type LocomotionState } from "../src/world/character-animation";
import { OTHER_PLAYER_LOOK, characterLook, humanoidNodes, humanoidStance } from "../src/world/humanoid";
import { expectValidMesh } from "./support/geometry";

const rest: LocomotionState = {
  forward: 0, right: 0, vertical: 0, moveWeight: 0, airWeight: 0, turnRate: 0, stridePhase: 0, shufflePhase: 0, time: 0,
};
const running: LocomotionState = { ...rest, forward: 6.3, moveWeight: 1, stridePhase: 1.2 };
const placement = { x: 2.5, feetY: 0.3, z: -1, yawRadians: 0.4 };

function ids(nodes: readonly RendererSceneNode[]): string[] {
  return nodes.map((node) => node.id);
}

function lowestPoint(nodes: readonly RendererSceneNode[]): number {
  // Approximate each part by its centre minus its vertical scale.
  return Math.min(...nodes.map((node) => node.transform!.translation[1] - (node.transform!.scale?.[1] ?? 0)));
}

describe("humanoid model", () => {
  const warden = characterLook({ classId: "warden", sex: "male" }, "ironcrest-helm");
  const ranger = characterLook({ classId: "ranger", sex: "female" }, "ranger-cap");
  const arcanist = characterLook({ classId: "arcanist", sex: "female" }, "wayfarer-hood");

  test("parts are valid meshes with stable, unique IDs under the unit's ID", () => {
    for (const look of [warden, ranger, arcanist, OTHER_PLAYER_LOOK]) {
      const nodes = humanoidNodes("unit-player-7", placement, look, poseFor(rest, humanoidStance(look)));
      expect(new Set(ids(nodes)).size).toBe(nodes.length);
      expect(nodes.every((node) => node.id.startsWith("unit-player-7-"))).toBe(true);
      for (const node of nodes) {
        expect(node.geometry.kind).toBe("mesh");
        if (node.geometry.kind === "mesh") {
          expectValidMesh(node.geometry, node.id);
        }
        const { translation, rotationQuaternion, scale } = node.transform!;
        expect([...translation, ...scale!].every(Number.isFinite)).toBe(true);
        expect(scale!.every((value) => value > 0)).toBe(true);
        expect(Math.hypot(...rotationQuaternion!)).toBeCloseTo(1, 9);
      }
      // Moving changes transforms, never the set of parts.
      const moving = humanoidNodes("unit-player-7", placement, look, poseFor(running, humanoidStance(look)));
      expect(ids(moving)).toEqual(ids(nodes));
    }
  });

  test("class gear and appearance show on the model", () => {
    const parts = (look: typeof warden) => ids(humanoidNodes("u", placement, look, poseFor(rest, humanoidStance(look))));
    expect(parts(warden)).toEqual(expect.arrayContaining(["u-sword-blade", "u-shield", "u-helm", "u-helm-crest"]));
    expect(parts(ranger)).toEqual(expect.arrayContaining(["u-bow-grip", "u-bow-string", "u-quiver", "u-cap-brim"]));
    expect(parts(arcanist)).toEqual(expect.arrayContaining(["u-staff", "u-staff-orb", "u-robe", "u-hood"]));
    expect(parts(OTHER_PLAYER_LOOK)).toContain("u-hair");
    expect(parts(warden)).not.toContain("u-bow-grip");
  });

  test("the existing Arcanist staff orb and core emit light without changing their mesh identity", () => {
    const nodes = humanoidNodes("u", placement, arcanist, poseFor(rest, humanoidStance(arcanist)));
    const moving = humanoidNodes("u", placement, arcanist, poseFor(running, humanoidStance(arcanist)));
    for (const id of ["u-staff-orb", "u-staff-orb-core"]) {
      const node = nodes.find((part) => part.id === id)!;
      expect(node).toBeDefined();
      expect(node.emissive).toBe(node.color);
      expect(node.geometry.kind).toBe("mesh");
      if (node.geometry.kind === "mesh") {
        expect(node.geometry.resourceKey).toBe("humanoid:blob");
      }
      expect(moving.find((part) => part.id === id)!.geometry).toEqual(node.geometry);
    }
    expect(nodes.find((part) => part.id === "u-staff")!.emissive).toBeUndefined();
    expect(nodes.find((part) => part.id === "u-staff-orb")!.transform!.scale).toEqual([0.075, 0.075, 0.075]);
    expect(nodes.find((part) => part.id === "u-staff-orb-core")!.transform!.scale).toEqual([0.04, 0.04, 0.04]);
  });

  test("stands on its feet at about 1.8 m and female builds are slighter", () => {
    const male = humanoidNodes("m", { ...placement, feetY: 0 }, warden, poseFor(rest, "sword-and-shield"));
    const female = humanoidNodes("f", { ...placement, feetY: 0 }, characterLook({ classId: "warden", sex: "female" }, "ironcrest-helm"), poseFor(rest, "sword-and-shield"));
    const head = (nodes: readonly RendererSceneNode[], id: string) => nodes.find((node) => node.id === `${id}-head`)!.transform!.translation;
    expect(head(male, "m")[1]).toBeGreaterThan(1.5);
    expect(head(male, "m")[1]).toBeLessThan(1.8);
    expect(head(female, "f")[1]).toBeLessThan(head(male, "m")[1]);
    expect(lowestPoint(male)).toBeGreaterThan(-0.1);
    expect(lowestPoint(male)).toBeLessThan(0.1);
    const shoulders = (nodes: readonly RendererSceneNode[], id: string) => {
      const left = nodes.find((node) => node.id === `${id}-left-shoulder`)!.transform!.translation;
      const right = nodes.find((node) => node.id === `${id}-right-shoulder`)!.transform!.translation;
      return Math.hypot(left[0] - right[0], left[2] - right[2]);
    };
    expect(shoulders(female, "f")).toBeLessThan(shoulders(male, "m"));
  });

  test("faces its yaw: the nose leads toward the facing direction", () => {
    const nodes = humanoidNodes("u", { x: 0, feetY: 0, z: 0, yawRadians: Math.PI / 2 }, warden, poseFor(rest, "sword-and-shield"));
    const nose = nodes.find((node) => node.id === "u-nose")!.transform!.translation;
    const head = nodes.find((node) => node.id === "u-head")!.transform!.translation;
    expect(nose[0] - head[0]).toBeGreaterThan(0.08);
    expect(Math.abs(nose[2] - head[2])).toBeLessThan(0.02);
  });
});

