import { describe, expect, test } from "bun:test";
import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { EntityState } from "../src/replication";
import { decodeCatalog, type ContentCatalog } from "../src/world/catalog";
import { QUADRUPED_LIMITS, RUN_SPEED, quadrupedPoseFor, type LocomotionState } from "../src/world/character-animation";
import { characterLook } from "../src/world/humanoid";
import type { UnitContext, UnitFrame } from "../src/world/unit-nodes";
import { stateColor } from "../src/world/units/creature-bodies";
import { CREATURE_MODEL, creatureModelParts, modelChoice, modelChoiceFor, type ModelChoice } from "../src/world/units/creature-models";
import { HUMANOID_ACCESSORY_BUDGET } from "../src/world/units/humanoid-body";
import { qRotate, type Quaternion } from "../src/world/units/model-kit";
import { catalogJson } from "./support/catalog";
import { NO_FLAGS } from "./support/snapshots";

const TEMPLATES = [
  { id: 1, name: "Timber Wolf", family: "wolf", behaviour: "aggressive", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [40, 45, 40] },
  { id: 2, name: "Young Boar", family: "boar", behaviour: "neutral", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [45, 45, 45] },
  { id: 3, name: "Grain Rat", family: "vermin", behaviour: "neutral", minLevel: 1, maxLevel: 1, elite: false, halfExtents: [25, 20, 25] },
  { id: 4, name: "Field Marauder", family: "marauder", behaviour: "aggressive", minLevel: 2, maxLevel: 3, elite: false, halfExtents: [30, 90, 30] },
  { id: 5, name: "Mirefin Lurker", family: "mirefin", behaviour: "aggressive", minLevel: 3, maxLevel: 3, elite: false, halfExtents: [35, 80, 35] },
  { id: 6, name: "Redbrand Bandit", family: "redbrand", behaviour: "aggressive", minLevel: 3, maxLevel: 5, elite: false, halfExtents: [30, 90, 30] },
  { id: 7, name: "Garrick Redbrand", family: "redbrand", behaviour: "aggressive", minLevel: 5, maxLevel: 5, elite: true, halfExtents: [40, 100, 40] },
];
const NPCS = [
  { id: 1, name: "Marshal", role: "quest_giver", level: 10 },
  { id: 3, name: "Innkeeper", role: "vendor", level: 5 },
  { id: 5, name: "Brother Aldous", role: "spirit_healer", level: 10 },
  { id: 6, name: "Greyhaven Guard", role: "guard", level: 10 },
];
// These NPCs give no quests.
const CATALOG: ContentCatalog = decodeCatalog(JSON.stringify({ ...catalogJson(), creatureTemplates: TEMPLATES, npcs: NPCS, quests: [] }));
const CONTEXT: UnitContext = {
  unitsPerMetre: 100,
  playerHalfHeightUnits: 90,
  viewerId: 99,
  viewerLook: characterLook({ classId: "warden", sex: "male" }, null),
  catalog: CATALOG,
  viewerTarget: null,
};
const REST: LocomotionState = {
  forward: 0, right: 0, vertical: 0, moveWeight: 0, airWeight: 0, turnRate: 0, stridePhase: 0, shufflePhase: 0, time: 0,
};
const RUNNING: LocomotionState = { ...REST, forward: RUN_SPEED, moveWeight: 1, stridePhase: 1.3 };

const creature = (appearance: number, flags: Partial<EntityState["flags"]> = {}): EntityState => ({
  kind: "creature", entityId: 100 + appearance, appearance, position: [0, 0, 0], velocity: [0, 0, 0], facing: 0,
  level: 2, healthPercent: 100, flags: { ...NO_FLAGS, hostile: true, attackable: true, ...flags },
});
const npc = (id: number): EntityState => ({
  kind: "npc", entityId: id, appearance: id, position: [0, 0, 0], velocity: [0, 0, 0], facing: 0, level: 10, healthPercent: 100, flags: NO_FLAGS,
});
const PLACEMENT = { x: 0, feetY: 0, z: 0, yawRadians: 0 };

function frame(entity: EntityState, locomotion = REST, context = CONTEXT): UnitFrame {
  return { id: `unit-${entity.kind}-${entity.entityId}`, entity, placement: PLACEMENT, locomotion, context };
}

/** Axis-aligned bounds of rendered nodes (box corners, sphere and cylinder by their boxes). */
function bounds(nodes: readonly RendererSceneNode[]): { min: [number, number, number]; max: [number, number, number] } {
  const min: [number, number, number] = [Infinity, Infinity, Infinity];
  const max: [number, number, number] = [-Infinity, -Infinity, -Infinity];
  for (const node of nodes) {
    const geometry = node.geometry as { kind: string; size?: number[]; radius?: number; height?: number };
    if (geometry.kind === "cylinder" && node.id.endsWith("target-ring")) {
      continue;
    }
    const half: number[] = geometry.kind === "box"
      ? geometry.size!.map((value) => value / 2)
      : geometry.kind === "sphere"
        ? [geometry.radius!, geometry.radius!, geometry.radius!]
        : [geometry.radius!, geometry.height! / 2, geometry.radius!];
    const centre = node.transform!.translation as [number, number, number];
    const rotation = node.transform!.rotationQuaternion as Quaternion;
    for (const sx of [-1, 1]) {
      for (const sy of [-1, 1]) {
        for (const sz of [-1, 1]) {
          const corner = qRotate(rotation, [sx * half[0]!, sy * half[1]!, sz * half[2]!]);
          for (let axis = 0; axis < 3; axis++) {
            min[axis] = Math.min(min[axis]!, centre[axis]! + corner[axis]!);
            max[axis] = Math.max(max[axis]!, centre[axis]! + corner[axis]!);
          }
        }
      }
    }
  }
  return { min, max };
}

const MODELS: [string, EntityState][] = [
  ["wolf", creature(1)], ["boar", creature(2)], ["vermin", creature(3)], ["marauder", creature(4)], ["mirefin", creature(5)],
  ["redbrand", creature(6)], ["garrick", creature(7)],
  ["quest_giver", npc(1)], ["vendor", npc(3)], ["spirit_healer", npc(5)], ["guard", npc(6)],
];

function box(entity: EntityState): [number, number, number] {
  const template = CATALOG.creatureTemplates.get(entity.appearance);
  return entity.kind === "creature" ? template!.halfExtents.map((value) => value / 100) as [number, number, number] : [0.3, 0.9, 0.3];
}

describe("model choice", () => {
  test("picks a body from kind, family and role, and falls back to the placeholder", () => {
    expect(modelChoice("creature", "wolf", undefined)).toBe("wolf");
    expect(modelChoice("creature", "boar", undefined)).toBe("boar");
    expect(modelChoice("creature", "vermin", undefined)).toBe("vermin");
    expect(modelChoice("creature", "marauder", undefined)).toBe("marauder");
    expect(modelChoice("creature", "mirefin", undefined)).toBe("mirefin");
    expect(modelChoice("creature", "redbrand", undefined)).toBe("redbrand");
    expect(modelChoice("npc", undefined, "guard")).toBe("guard");
    expect(modelChoice("npc", undefined, "vendor")).toBe("vendor");
    expect(modelChoice("npc", undefined, "quest_giver")).toBe("quest_giver");
    expect(modelChoice("npc", undefined, "spirit_healer")).toBe("spirit_healer");
    expect(modelChoice("creature", undefined, undefined)).toBe("placeholder");
    expect(modelChoice("npc", undefined, undefined)).toBe("placeholder");
    expect(modelChoice("player", "wolf", "guard")).toBe("placeholder");
  });

  test("reads the family and role from the catalog; unknown IDs get the placeholder box", () => {
    expect(modelChoiceFor(creature(3), CATALOG)).toBe("vermin");
    expect(modelChoiceFor(npc(5), CATALOG)).toBe("spirit_healer");
    expect(modelChoiceFor(creature(77), CATALOG)).toBe("placeholder");
    expect(modelChoiceFor(npc(77), CATALOG)).toBe("placeholder");
    const unknown = creature(77);
    expect(creatureModelParts(frame(unknown))).toBeNull();
    expect(CREATURE_MODEL.nodes(frame(unknown)).map((node) => node.id)).toEqual(["unit-creature-177-body", "unit-creature-177-nose"]);
  });

  test("choice covers every model the registry draws", () => {
    const seen = new Set<ModelChoice>(MODELS.map(([, entity]) => modelChoiceFor(entity, CATALOG)));
    expect([...seen].sort()).toEqual(["boar", "guard", "mirefin", "marauder", "quest_giver", "redbrand", "spirit_healer", "vendor", "vermin", "wolf"].sort());
  });
});

describe("model bounds", () => {
  test.each(MODELS)("%s stands inside its collision box within 10%%", (_name, entity) => {
    const [hx, hy, hz] = box(entity);
    const { min, max } = bounds(CREATURE_MODEL.nodes(frame(entity)));
    expect(min[1]).toBeGreaterThan(-0.1 * hy * 2);
    expect(min[1]).toBeLessThan(0.05 * hy * 2);
    expect(max[1]).toBeGreaterThan(hy * 2 * 0.9);
    expect(max[1]).toBeLessThan(hy * 2 * 1.1);
    expect(Math.max(-min[0], max[0])).toBeLessThan(hx * 1.1);
    expect(Math.max(-min[2], max[2])).toBeLessThan(hz * 1.1);
  });

  test.each(MODELS)("%s has at most 24 nodes with its target ring", (_name, entity) => {
    const targeted = { ...CONTEXT, viewerTarget: { kind: entity.kind, id: entity.entityId } as UnitContext["viewerTarget"] };
    const nodes = CREATURE_MODEL.nodes(frame(entity, RUNNING, targeted));
    expect(nodes.length).toBeLessThanOrEqual(24);
    expect(nodes[0]!.id).toEndWith("-target-ring");
    expect(new Set(nodes.map((node) => node.id)).size).toBe(nodes.length);
    expect(HUMANOID_ACCESSORY_BUDGET + 13).toBeLessThanOrEqual(24);
  });

  test("Garrick is 15% larger than a bandit", () => {
    const bandit = bounds(CREATURE_MODEL.nodes(frame(creature(6)))).max[1];
    const garrick = bounds(CREATURE_MODEL.nodes(frame(creature(7)))).max[1];
    expect(garrick / bandit).toBeGreaterThan(1.05);
    expect(garrick / bandit).toBeLessThan(1.2);
  });
});

describe("trot gait", () => {
  test("zero speed is idle: legs hang still and the body breathes", () => {
    const idle = quadrupedPoseFor({ ...REST, stridePhase: 2.2, time: 1.4 });
    expect([idle.frontLeft, idle.frontRight, idle.backLeft, idle.backRight].map(Math.abs)).toEqual([0, 0, 0, 0]);
    expect(Math.abs(idle.bob)).toBe(0);
    expect(idle.breathe).toBeGreaterThan(0);
    const torso = (time: number) => CREATURE_MODEL.nodes(frame(creature(1), { ...REST, time })).find((node) => node.id.endsWith("-torso"))!;
    const widthAt = (time: number) => (torso(time).geometry as { size: number[] }).size[0]!;
    expect(widthAt(0.3)).not.toBe(widthAt(1.4));
    expect(quadrupedPoseFor({ ...REST, time: 0 })).toEqual(quadrupedPoseFor({ ...REST, time: 0 }));
  });

  test("diagonal pairs swing together and against each other, reaching further when faster", () => {
    const walk = quadrupedPoseFor({ ...REST, forward: 1.5, moveWeight: 1, stridePhase: 0.7 });
    expect(walk.frontLeft).toBeCloseTo(walk.backRight, 12);
    expect(walk.frontRight).toBeCloseTo(walk.backLeft, 12);
    expect(walk.frontLeft).toBeCloseTo(-walk.frontRight, 12);
    expect(walk.frontLeft).not.toBe(0);
    const run = quadrupedPoseFor({ ...REST, forward: RUN_SPEED, moveWeight: 1, stridePhase: 0.7 });
    expect(Math.abs(run.frontLeft)).toBeGreaterThan(Math.abs(walk.frontLeft));
  });

  test("the phase follows distance travelled, deterministically, within limits", () => {
    const at = (stridePhase: number) => quadrupedPoseFor({ ...RUNNING, stridePhase });
    expect(at(1.1)).toEqual(at(1.1));
    expect(at(1.1).frontLeft).not.toBe(at(1.6).frontLeft);
    for (let phase = 0; phase < 20; phase += 0.37) {
      const pose = at(phase);
      for (const leg of [pose.frontLeft, pose.frontRight, pose.backLeft, pose.backRight]) {
        expect(Math.abs(leg)).toBeLessThanOrEqual(QUADRUPED_LIMITS.legSwing[1]);
      }
      expect(pose.bob).toBeGreaterThanOrEqual(QUADRUPED_LIMITS.bob[0]);
      expect(pose.bob).toBeLessThanOrEqual(0);
    }
    expect(quadrupedPoseFor({ ...REST, forward: Number.NaN, moveWeight: Number.NaN, stridePhase: Number.NaN }).frontLeft).toBe(0);
  });

  test("a running animal's nodes differ from its idle nodes; an idle one is stable", () => {
    const wolf = creature(1);
    expect(CREATURE_MODEL.nodes(frame(wolf, RUNNING))).not.toEqual(CREATURE_MODEL.nodes(frame(wolf)));
    expect(CREATURE_MODEL.nodes(frame(wolf))).toEqual(CREATURE_MODEL.nodes(frame(wolf)));
  });
});

describe("death", () => {
  const lying = (entity: EntityState) => {
    const dead = { ...entity, flags: { ...entity.flags, dead: true } };
    return bounds(CREATURE_MODEL.nodes(frame(dead, RUNNING)));
  };

  test("animals roll onto their side: height shrinks to the body width, length is kept", () => {
    for (const entity of [creature(1), creature(2), creature(3)]) {
      const [hx, hy, hz] = box(entity);
      const { min, max } = lying(entity);
      expect(min[1]).toBeGreaterThan(-0.15 * hx);
      expect(max[1]).toBeLessThan(hx * 2.2);
      expect(max[1]).toBeLessThan(hy * 2 * 0.95);
      expect(Math.max(-min[2], max[2])).toBeLessThan(hz * 1.1);
      expect(max[1] - min[1]).toBeLessThan(hy * 2);
    }
  });

  test("humanoids lie flat on their backs, head and feet along the facing", () => {
    for (const entity of [creature(4), creature(5), creature(6), creature(7), npc(6), npc(5)]) {
      const [, hy] = box(entity);
      const { min, max } = lying(entity);
      expect(min[1]).toBeGreaterThan(-0.08);
      expect(max[1]).toBeLessThan(0.55);
      expect(max[2] - min[2]).toBeGreaterThan(hy * 1.4);
    }
  });

  test("corpses keep the shade and never move their legs", () => {
    const wolf = creature(1);
    const dead = { ...wolf, flags: { ...wolf.flags, dead: true } };
    expect(CREATURE_MODEL.nodes(frame(dead, RUNNING))).toEqual(CREATURE_MODEL.nodes(frame(dead)));
    const living = CREATURE_MODEL.nodes(frame(wolf));
    const corpse = CREATURE_MODEL.nodes(frame(dead));
    const fur = (nodes: RendererSceneNode[]) => nodes.find((node) => node.id.endsWith("-torso"))!.color as string;
    const channel = (color: string, shift: number) => (Number.parseInt(color.slice(1), 16) >> shift) & 0xff;
    expect(channel(fur(corpse), 16)).toBeLessThan(channel(fur(living), 16) * 0.5);
  });
});

describe("accents", () => {
  const eyes = (entity: EntityState) => CREATURE_MODEL.nodes(frame(entity)).find((node) => node.id.endsWith("-collar"))!.color as string;
  const tunic = (entity: EntityState) => CREATURE_MODEL.nodes(frame(entity)).find((node) => node.id.endsWith("-torso"))!.color as string;

  test("hostile, neutral, tapped and corpse animals are told apart by their accent", () => {
    const hostile = eyes(creature(1));
    const neutral = eyes(creature(1, { hostile: false }));
    const tapped = eyes(creature(1, { tappedByOther: true }));
    const dead = eyes(creature(1, { dead: true }));
    expect(new Set([hostile, neutral, tapped, dead]).size).toBe(4);
    expect(hostile).toBe(stateColor(creature(1), CATALOG));
  });

  test("humanoid creatures wear the disposition colour", () => {
    const hostile = tunic(creature(4));
    const neutral = tunic(creature(4, { hostile: false }));
    const tapped = tunic(creature(4, { tappedByOther: true }));
    const dead = tunic(creature(4, { dead: true }));
    expect(new Set([hostile, neutral, tapped, dead]).size).toBe(4);
  });

  test("NPC roles differ in colour", () => {
    const colours = [npc(1), npc(3), npc(5), npc(6)].map(tunic);
    expect(new Set(colours).size).toBe(4);
  });
});
