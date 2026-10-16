import { describe, expect, test } from "bun:test";
import type { EntityState } from "../src/replication";
import { decodeCatalog } from "../src/world/catalog";
import { OTHER_PLAYER_LOOK, characterLook } from "../src/world/humanoid";
import {
  PLAYER_MODEL,
  UNIT_MODELS,
  UnitAnimators,
  placeUnit,
  placeWithModel,
  unitIdentity,
  unitModel,
  type UnitContext,
  type UnitModel,
  unitNodeIds,
} from "../src/world/unit-nodes";
import { PLACEHOLDER_BODY_MODEL } from "../src/world/units/creature-bodies";
import { CREATURE_MODEL } from "../src/world/units/creature-models";
import { catalogJson } from "./support/catalog";
import { NO_FLAGS, playerEntity } from "./support/snapshots";

const CONTEXT: UnitContext = {
  unitsPerMetre: 100,
  playerHalfHeightUnits: 90,
  viewerId: 3,
  viewerLook: characterLook({ classId: "ranger", sex: "female" }, "ranger-cap"),
  catalog: decodeCatalog(JSON.stringify(catalogJson())),
  viewerTarget: null,
};

function player(entityId: number, position: EntityState["position"] = [0, 90, 0]): EntityState {
  return playerEntity(entityId, position);
}

describe("unit placement", () => {
  test("feet sit on flat physics ground raised by presentation relief", () => {
    const entity = playerEntity(4, [250, 90, -100], [0, 0, 0], 16_384);
    expect(placeUnit(entity, 90, 0, 100)).toEqual({ x: 2.5, feetY: 0, z: -1, yawRadians: Math.PI / 2 });
    expect(placeUnit(entity, 90, 40, 100).feetY).toBeCloseTo(0.4, 9);
    expect(unitIdentity(entity)).toBe("unit-player-4");
  });
});

describe("unit model registry", () => {
  test("every entity kind has a model and animation state is kept per visible unit", () => {
    expect(Object.keys(UNIT_MODELS)).toEqual(["player", "creature", "npc"]);
    expect(unitModel(player(3))).toBe(PLAYER_MODEL);
    expect(unitModel({ kind: "creature" })).toBe(CREATURE_MODEL);
    expect(unitModel({ kind: "npc" })).toBe(CREATURE_MODEL);
    const animators = new UnitAnimators();
    const entity = { ...player(3), velocity: [21, 0, 0] as const, facing: 16_384 };
    const first = placeUnit(entity, 90, 0, 100);
    animators.locomotion(entity, first, 100, 1 / 30, true);
    const moved = { ...entity, position: [21, 90, 0] as const };
    const locomotion = animators.locomotion(moved, placeUnit(moved, 90, 0, 100), 100, 1 / 30, true);
    expect(locomotion.forward).toBeCloseTo(6.3, 9);
    expect(locomotion.stridePhase).toBeGreaterThan(0);
    animators.retain(new Set());
    const fresh = animators.locomotion(moved, placeUnit(moved, 90, 0, 100), 100, 1 / 30, true);
    expect(fresh.stridePhase).toBe(0);
    const nodes = PLAYER_MODEL.nodes({ id: "unit-player-3", entity, placement: first, locomotion, context: CONTEXT });
    expect(nodes.map((node) => node.id)).toContain("unit-player-3-bow-stave");
  });

  test("the player model dresses the viewer's own unit in the local look and others in the shared one", () => {
    const placement = placeWithModel(PLAYER_MODEL, player(3), CONTEXT, 0);
    expect(placement.feetY).toBe(0);
    const locomotion = new UnitAnimators().locomotion(player(3), placement, 100, 1 / 30, true);
    const draw = (entityId: number, viewerLook = CONTEXT.viewerLook) =>
      PLAYER_MODEL.nodes({ id: "unit", entity: player(entityId), placement, locomotion, context: { ...CONTEXT, viewerLook } });
    expect(draw(3)).not.toEqual(draw(8));
    expect(draw(3, OTHER_PLAYER_LOOK)).toEqual(draw(8));
  });

  test("the Ranger's draw pose turns the rigid archer mesh side-on", () => {
    const placement = placeWithModel(PLAYER_MODEL, player(3), CONTEXT, 0);
    const locomotion = new UnitAnimators().locomotion(player(3), placement, 100, 1 / 30, true);
    const stave = (viewerAction: UnitContext["viewerAction"]) =>
      PLAYER_MODEL.nodes({ id: "unit", entity: player(3), placement, locomotion, context: { ...CONTEXT, viewerAction } })
        .find((node) => node.id === "unit-bow-stave")!.transform.rotationQuaternion;
    expect(stave({ pose: "draw", progress: 1, hold: true })).not.toEqual(stave(null));
  });

  test("models read the projected entity: registry dispatch, per-entity body height and nodes", () => {
    // A stand-in for a creature model keyed by appearance: its body height depends on the entity.
    const heights = new Map([[5, 40], [6, 120]]);
    const seen: EntityState[] = [];
    const sized: UnitModel = {
      halfHeightUnits: (entity) => heights.get(entity.entityId) ?? 0,
      nodes: (frame) => {
        seen.push(frame.entity);
        return [{ id: `${frame.id}-body`, geometry: { kind: "sphere", radius: 0.3 }, color: "#808080", transform: { translation: [0, 0, 0] } }];
      },
    };
    const registry = { player: sized };
    const small = player(5, [100, 40, 0]);
    const tall = player(6, [100, 120, 0]);
    expect(unitModel(small, registry)).toBe(sized);
    // Both stand on the ground whatever their size, raised by the same relief.
    expect(placeWithModel(sized, small, CONTEXT, 25).feetY).toBeCloseTo(0.25, 9);
    expect(placeWithModel(sized, tall, CONTEXT, 25).feetY).toBeCloseTo(0.25, 9);
    // The player's half height would sink the small body into the ground.
    expect(placeWithModel(PLAYER_MODEL, small, CONTEXT, 25).feetY).toBeCloseTo(-0.25, 9);
    const placement = placeWithModel(sized, tall, CONTEXT, 0);
    const locomotion = new UnitAnimators().locomotion(tall, placement, 100, 1 / 30, true);
    sized.nodes({ id: unitIdentity(tall), entity: tall, placement, locomotion, context: CONTEXT });
    expect(seen).toEqual([tall]);
  });
});

describe("creature and NPC models", () => {
  const wolf: EntityState = {
    kind: "creature", entityId: 108, appearance: 1, position: [300, 45, 400], velocity: [0, 0, 0], facing: 0,
    level: 2, healthPercent: 60, flags: { ...NO_FLAGS, hostile: true, attackable: true },
  };
  const guard: EntityState = { ...playerEntity(6, [0, 90, 0]), kind: "npc", appearance: 6, level: 10 };

  test("stand on their own collision box and ring only the viewer's target", () => {
    expect(PLACEHOLDER_BODY_MODEL.halfHeightUnits(wolf, CONTEXT)).toBe(45);
    expect(PLACEHOLDER_BODY_MODEL.halfHeightUnits(guard, CONTEXT)).toBe(90);
    expect(placeWithModel(PLACEHOLDER_BODY_MODEL, wolf, CONTEXT, 25).feetY).toBeCloseTo(0.25, 9);
    const draw = (entity: EntityState, viewerTarget: UnitContext["viewerTarget"]) => {
      const placement = placeWithModel(PLACEHOLDER_BODY_MODEL, entity, CONTEXT, 0);
      const locomotion = new UnitAnimators().locomotion(entity, placement, 100, 1 / 30, true);
      return PLACEHOLDER_BODY_MODEL.nodes({
        id: unitIdentity(entity), entity, placement, locomotion, context: { ...CONTEXT, viewerTarget },
      }).map((node) => node.id);
    };
    expect(draw(wolf, null)).toEqual(["unit-creature-108-body", "unit-creature-108-nose"]);
    expect(draw(wolf, { kind: "creature", id: 108 })[0]).toBe("unit-creature-108-target-ring");
    // Same ID, another kind: not the target.
    expect(draw(wolf, { kind: "npc", id: 108 })).not.toContain("unit-creature-108-target-ring");
    expect(draw(guard, { kind: "npc", id: 6 })).toEqual(["unit-npc-6-target-ring", "unit-npc-6-body", "unit-npc-6-nose"]);
  });
});

describe("unitNodeIds", () => {
  test("lists the nodes of one unit and never those of a unit whose ID shares a prefix", () => {
    const nodes = ["unit-creature-10-body", "unit-creature-108-ear-left", "unit-creature-108-body", "unit-npc-108-body", "tree-1"].map((id) => ({ id }));
    expect(unitNodeIds(nodes, "unit-creature-108")).toEqual(["unit-creature-108-ear-left", "unit-creature-108-body"]);
    expect(unitNodeIds(nodes, "unit-creature-9")).toEqual([]);
  });
});
