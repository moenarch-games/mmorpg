import { describe, expect, test } from "bun:test";
import { BackgroundPoseDetail } from "../src/world/units/pose-detail";

const viewer = [0, 0, 0] as const;
const idle = {
  id: "unit-creature-108",
  entity: { kind: "creature" as const, id: 108 },
  feet: [0, 0, 40] as const,
  pitch: 0,
  layers: [{ clip: "Idle", time: 0, weight: 1, mask: "all" as const }],
  bow: null, crossbow: null, channel: null, throwable: null,
};

describe("background pose detail", () => {
  test("starts off, and can be enabled or disabled without retaining stale deadlines", () => {
    const detail = new BackgroundPoseDetail();
    expect(detail.shouldPose(idle, viewer, null, 0)).toBe(true);
    expect(detail.shouldPose(idle, viewer, null, 0.001)).toBe(true);
    detail.setLevel("balanced");
    expect(detail.shouldPose(idle, viewer, null, 0.01)).toBe(true);
    expect(detail.shouldPose({ ...idle, layers: [{ ...idle.layers[0]!, time: 0.011 }] }, viewer, null, 0.011)).toBe(false);
    expect(detail.shouldPose(idle, viewer, null, 0.1)).toBe(true);
    detail.setLevel("off");
    expect(detail.shouldPose(idle, viewer, null, 0.101)).toBe(true);
    detail.setLevel("balanced");
    expect(detail.shouldPose(idle, viewer, null, 0.102)).toBe(true);
  });

  test("near actors, all players, and the selected target retain full-rate poses", () => {
    const detail = new BackgroundPoseDetail();
    detail.setLevel("aggressive");
    expect(detail.shouldPose({ ...idle, feet: [0, 0, 5] }, viewer, null, 0)).toBe(true);
    expect(detail.shouldPose({ ...idle, feet: [0, 0, 5] }, viewer, null, 0.001)).toBe(true);
    const player = { ...idle, entity: { kind: "player" as const, id: 108 } };
    expect(detail.shouldPose(player, viewer, null, 0)).toBe(true);
    expect(detail.shouldPose(player, viewer, null, 0.001)).toBe(true);
    expect(detail.shouldPose(idle, viewer, idle.entity, 0)).toBe(true);
    expect(detail.shouldPose(idle, viewer, idle.entity, 0.001)).toBe(true);
    // No viewer context, such as the character turntable, always poses normally.
    expect(detail.shouldPose(idle, null, null, 0.002)).toBe(true);
  });

  test("combat, casting, guard and traversal changes immediately restore full-rate sampling", () => {
    const detail = new BackgroundPoseDetail();
    detail.setLevel("balanced");
    expect(detail.shouldPose(idle, viewer, null, 1)).toBe(true);
    expect(detail.shouldPose(idle, viewer, null, 1.001)).toBe(false);
    const attack = { ...idle, layers: [{ ...idle.layers[0]!, clip: "Attack" }] };
    expect(detail.shouldPose(attack, viewer, null, 1.002)).toBe(true);
    expect(detail.shouldPose(attack, viewer, null, 1.003)).toBe(true);
    // Returning to idle re-samples immediately, rather than holding the attack pose.
    expect(detail.shouldPose(idle, viewer, null, 1.004)).toBe(true);
    const cast = { ...idle, channel: { joint: "hand_r" as const, point: [0, 0, 0] as const } };
    expect(detail.shouldPose(cast, viewer, null, 1.005)).toBe(true);
    expect(detail.shouldPose({ ...idle, throwable: "knife" as const }, viewer, null, 1.005)).toBe(true);
    expect(detail.shouldPose({ ...idle, layers: [{ ...idle.layers[0]!, clip: "Idle_Shield_Loop" }] }, viewer, null, 1.006)).toBe(true);
    expect(detail.shouldPose({ ...idle, pitch: 0.2 }, viewer, null, 1.007)).toBe(true);
    expect(detail.shouldPose(idle, viewer, null, 1.008)).toBe(true);
  });

  test("appearance blends and actor removal invalidate their pose schedules", () => {
    const detail = new BackgroundPoseDetail();
    detail.setLevel("light");
    expect(detail.shouldPose(idle, viewer, null, 0)).toBe(true);
    expect(detail.shouldPose(idle, viewer, null, 0.001)).toBe(false);
    const blend = { ...idle, layers: [{ ...idle.layers[0]!, weight: 0.75 }] };
    expect(detail.shouldPose(blend, viewer, null, 0.002)).toBe(true);
    expect(detail.shouldPose(blend, viewer, null, 0.003)).toBe(false);
    detail.forget(idle.id);
    expect(detail.shouldPose(blend, viewer, null, 0.004)).toBe(true);
  });

  test("off-camera background poses sleep and refresh immediately on re-entry", () => {
    const detail = new BackgroundPoseDetail();
    detail.setLevel("balanced");
    expect(detail.shouldPose(idle, viewer, null, 0, false)).toBe(true);
    expect(detail.shouldPose(idle, viewer, null, 1, false)).toBe(false);
    expect(detail.shouldPose({ ...idle, layers: [{ ...idle.layers[0]!, time: 2 }] }, viewer, null, 2, false)).toBe(false);
    expect(detail.shouldPose(idle, viewer, null, 2.001, true)).toBe(true);
    expect(detail.shouldPose(idle, viewer, null, 2.002, true)).toBe(false);
    expect(detail.shouldPose(idle, viewer, null, 2.003, false)).toBe(false);
    // A significant action still samples while off-camera.
    expect(detail.shouldPose({ ...idle, layers: [{ ...idle.layers[0]!, clip: "Attack" }] }, viewer, null, 2.004, false)).toBe(true);
    expect(detail.shouldPose(idle, viewer, null, 2.005, true)).toBe(true);
  });

  test("distance hysteresis avoids rapidly toggling near the background boundary", () => {
    const detail = new BackgroundPoseDetail();
    detail.setLevel("balanced");
    const at = (distance: number) => ({ ...idle, feet: [0, 0, distance] as const });
    expect(detail.shouldPose(at(25.1), viewer, null, 0)).toBe(true);
    expect(detail.shouldPose(at(24.9), viewer, null, 0.001)).toBe(false);
    expect(detail.shouldPose(at(22.4), viewer, null, 0.002)).toBe(true);
    expect(detail.shouldPose(at(22.4), viewer, null, 0.003)).toBe(true);
  });
});
