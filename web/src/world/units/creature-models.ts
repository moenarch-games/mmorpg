import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { sameEntity } from "../../entity-ref";
import type { EntityKind, EntityState } from "../../replication";
import type { ContentCatalog, CreatureFamily, NpcRole } from "../catalog";
import { poseFor, quadrupedPoseFor, type QuadrupedPose } from "../character-animation";
import type { UnitFrame, UnitModel, UnitPlacement } from "../unit-nodes";
import { PLACEHOLDER_BODY_MODEL, bodyHalfHeightUnits, stateColor } from "./creature-bodies";
import { humanoidParts, REST, addShield, addSword, type HumanoidSpec } from "./humanoid-body";
import { NPC_SPECS } from "./npc-models";
import { addPart, box, child, partNodes, rootFrame, targetRingNode, tilt, type Frame, type Part, type Primitive } from "./model-kit";
import type { Color } from "../scenery";

/**
 * Models for creatures and NPCs, chosen by what the projection and catalog
 * say: animals by family (wolf, boar, vermin) are low-poly quadrupeds that
 * trot, humanoid families (marauder, mirefin, redbrand) and NPC roles are
 * compact humanoids with role gear. Every model stands inside its template's
 * collision box, with the disposition colour as an accent (animal eyes and
 * collar, humanoid clothing), and lies down when dead. Unknown templates and
 * NPCs fall back to the placeholder box.
 */
export type ModelChoice =
  | "wolf" | "boar" | "vermin" | "marauder" | "mirefin" | "redbrand"
  | "guard" | "vendor" | "quest_giver" | "spirit_healer"
  | "placeholder";

/** The model for a unit of this kind with this creature family or NPC role; `placeholder` when either is unknown. */
export function modelChoice(kind: EntityKind, family: CreatureFamily | undefined, role: NpcRole | undefined): ModelChoice {
  if (kind === "creature") {
    return family ?? "placeholder";
  }
  if (kind === "npc") {
    return role ?? "placeholder";
  }
  return "placeholder";
}

/** The model choice for a projected entity, read through the catalog. */
export function modelChoiceFor(entity: Pick<EntityState, "kind" | "appearance" | "entityId">, catalog: ContentCatalog): ModelChoice {
  const family = entity.kind === "creature" ? catalog.creatureTemplates.get(entity.appearance)?.family : undefined;
  const role = entity.kind === "npc" ? catalog.npcs.get(entity.entityId)?.role : undefined;
  return modelChoice(entity.kind, family, role);
}

type Pt = readonly [number, number, number];

/**
 * One animal body. Sizes and positions are fractions of the collision box:
 * x in half widths, y in full heights above the feet, z in half lengths
 * (forward positive), so one spec fits any template of its family.
 */
type AnimalSpec = {
  fur: Color;
  dark: Color;
  /** Torso and its centre. */
  torso: { size: Pt; at: Pt };
  /** Legs: hip height, length, thickness, hip x and the front and back hip z. */
  legs: { hipY: number; length: number; thickness: number; x: number; front: number; back: number };
  head: { size: Pt; at: Pt };
  snout: { size: Pt; at: Pt; color: Color };
  ear: { size: Pt; at: Pt };
  eye: { size: Pt; at: Pt };
  /** A band in the accent colour around the neck. */
  collar: { size: Pt; at: Pt };
  tail: { size: Pt; pivot: Pt; color: Color };
  extras: readonly { name: string; size: Pt; at: Pt; color: Color }[];
  /** Height of the body centre while lying on its side, in half widths. */
  lift: number;
};

const BONE: Color = "#e6dcc0";

const ANIMALS: Record<"wolf" | "boar" | "vermin", AnimalSpec> = {
  wolf: {
    fur: "#85766a", dark: "#5f544a",
    torso: { size: [1.15, 0.32, 1.1], at: [0, 0.62, -0.25] },
    legs: { hipY: 0.52, length: 0.52, thickness: 0.3, x: 0.45, front: 0.35, back: -0.65 },
    head: { size: [0.8, 0.28, 0.6], at: [0, 0.74, 0.55] },
    snout: { size: [0.4, 0.12, 0.35], at: [0, 0.68, 0.88], color: "#4a4038" },
    ear: { size: [0.2, 0.12, 0.1], at: [0.28, 0.94, 0.42] },
    eye: { size: [0.14, 0.06, 0.06], at: [0.3, 0.77, 0.84] },
    collar: { size: [0.95, 0.3, 0.12], at: [0, 0.66, 0.3] },
    tail: { size: [0.18, 0.12, 0.3], pivot: [0, 0.7, -0.7], color: "#6e6156" },
    extras: [],
    lift: 0.65,
  },
  boar: {
    fur: "#6e4d33", dark: "#4b3220",
    torso: { size: [1.5, 0.5, 1.15], at: [0, 0.6, -0.15] },
    legs: { hipY: 0.4, length: 0.4, thickness: 0.35, x: 0.55, front: 0.4, back: -0.55 },
    head: { size: [0.9, 0.32, 0.5], at: [0, 0.5, 0.65] },
    snout: { size: [0.5, 0.18, 0.24], at: [0, 0.44, 0.88], color: "#8d6a5a" },
    ear: { size: [0.2, 0.1, 0.1], at: [0.35, 0.7, 0.5] },
    eye: { size: [0.12, 0.06, 0.06], at: [0.35, 0.57, 0.89] },
    collar: { size: [1.52, 0.5, 0.1], at: [0, 0.6, 0.4] },
    tail: { size: [0.08, 0.08, 0.2], pivot: [0, 0.75, -0.72], color: "#4b3220" },
    extras: [
      { name: "bristles", size: [0.6, 0.14, 0.8], at: [0, 0.9, -0.25], color: "#3a2819" },
      { name: "tusk-left", size: [0.07, 0.07, 0.18], at: [0.32, 0.4, 0.95], color: BONE },
      { name: "tusk-right", size: [0.07, 0.07, 0.18], at: [-0.32, 0.4, 0.95], color: BONE },
    ],
    lift: 0.8,
  },
  vermin: {
    fur: "#8a7f73", dark: "#675f56",
    torso: { size: [1.1, 0.5, 0.9], at: [0, 0.5, -0.1] },
    legs: { hipY: 0.3, length: 0.3, thickness: 0.25, x: 0.4, front: 0.3, back: -0.45 },
    head: { size: [0.7, 0.4, 0.45], at: [0, 0.58, 0.62] },
    snout: { size: [0.3, 0.2, 0.25], at: [0, 0.52, 0.9], color: "#c79e94" },
    ear: { size: [0.3, 0.3, 0.1], at: [0.3, 0.85, 0.5] },
    eye: { size: [0.1, 0.07, 0.06], at: [0.28, 0.62, 0.83] },
    collar: { size: [1.0, 0.4, 0.1], at: [0, 0.52, 0.38] },
    tail: { size: [0.1, 0.1, 0.45], pivot: [0, 0.5, -0.55], color: "#c79e94" },
    extras: [],
    lift: 0.6,
  },
};

/** The parts of one animal in a gait inside the box `halfMetres`, lying on its side when `dead`. */
export function animalParts(
  family: "wolf" | "boar" | "vermin",
  placement: UnitPlacement,
  halfMetres: readonly [number, number, number],
  accent: Color,
  pose: QuadrupedPose,
  dead: boolean,
): Part[] {
  const spec = ANIMALS[family];
  const [hx, hy, hz] = halfMetres;
  const height = hy * 2;
  const pivotY = spec.legs.hipY * height;
  const root = rootFrame(placement, hy, dead ? { rotation: tilt(0, Math.PI / 2), lift: spec.lift * hx } : null);
  const body = child(root, [0, pivotY + pose.bob, 0], tilt(pose.pitch));
  const parts: Part[] = [];
  const at = ([x, y, z]: Pt, mirror = 1): [number, number, number] => [x * mirror * hx, y * height - pivotY, z * hz];
  const size = ([x, y, z]: Pt): Primitive => box(x * hx, y * height, z * hz);
  const frame = (position: Pt, mirror = 1): Frame => child(body, at(position, mirror));
  const breathe = 1 + pose.breathe;
  const [tx, ty, tz] = spec.torso.size;
  addPart(parts, "torso", size([tx * breathe, ty, tz * breathe]), spec.fur, frame(spec.torso.at));
  const { legs } = spec;
  const swings = [
    ["front-left", 1, legs.front, pose.frontLeft], ["front-right", -1, legs.front, pose.frontRight],
    ["back-left", 1, legs.back, pose.backLeft], ["back-right", -1, legs.back, pose.backRight],
  ] as const;
  for (const [name, side, z, swing] of swings) {
    const hip = child(body, at([legs.x, legs.hipY, z], side), tilt(-swing));
    const length = legs.length * height;
    addPart(parts, `${name}-leg`, box(legs.thickness * hx, length, legs.thickness * hz), spec.dark, child(hip, [0, -length / 2, 0]));
  }
  const head = child(body, at(spec.head.at), tilt(pose.headPitch));
  const relative = (position: Pt, mirror = 1): Frame => {
    const [x, y, z] = at(position, mirror);
    const [hx0, hy0, hz0] = at(spec.head.at);
    return child(head, [x - hx0, y - hy0, z - hz0]);
  };
  addPart(parts, "head", size(spec.head.size), spec.fur, child(head, [0, 0, 0]));
  addPart(parts, "snout", size(spec.snout.size), spec.snout.color, relative(spec.snout.at));
  for (const [name, side] of [["left", 1], ["right", -1]] as const) {
    addPart(parts, `ear-${name}`, size(spec.ear.size), spec.dark, relative(spec.ear.at, side));
    addPart(parts, `eye-${name}`, size(spec.eye.size), accent, relative(spec.eye.at, side));
  }
  addPart(parts, "collar", size(spec.collar.size), accent, frame(spec.collar.at));
  const tail = child(body, at(spec.tail.pivot), tilt(pose.tailLift - 0.4, 0, pose.tailSway));
  addPart(parts, "tail", size(spec.tail.size), spec.tail.color, child(tail, [0, 0, -spec.tail.size[2] * hz / 2]));
  for (const extra of spec.extras) {
    addPart(parts, extra.name, size(extra.size), extra.color, frame(extra.at));
  }
  return parts;
}

/** A pose for a body that lies still: legs relaxed, nothing moving. */
function corpsePose(): QuadrupedPose {
  return { ...quadrupedPoseFor(REST), frontLeft: 0.25, frontRight: 0.1, backLeft: -0.2, backRight: -0.1, tailLift: 0 };
}

const MARAUDER_SKIN: Color = "#a07a52";
const MIREFIN_SKIN: Color = "#4d8e88";
const BANDIT_SKIN: Color = "#d0a27c";

/** Humanoid creature looks: the disposition colour is the clothing tint. */
export function creatureHumanoidSpec(family: "marauder" | "mirefin" | "redbrand", elite: boolean, accent: Color): HumanoidSpec {
  switch (family) {
    case "marauder":
      return {
        skin: MARAUDER_SKIN, tunic: accent, trousers: "#4a4036", boots: "#2f2923", stance: "sword-and-shield", hunch: 0.08,
        accessories: (gear) => {
          const { head, pelvis, sx, sy, sz } = gear;
          gear.add("snout", box(0.1 * sx, 0.08 * sy, 0.13 * sz), "#6e5236", child(head, [0, -0.04 * sy, 0.13 * sz]));
          gear.add("ear-left", box(0.05 * sx, 0.1 * sy, 0.04 * sz), "#6e5236", child(head, [0.09 * sx, 0.12 * sy, -0.02 * sz], tilt(0, -0.3)));
          gear.add("ear-right", box(0.05 * sx, 0.1 * sy, 0.04 * sz), "#6e5236", child(head, [-0.09 * sx, 0.12 * sy, -0.02 * sz], tilt(0, 0.3)));
          gear.add("rags", box(0.3 * sx, 0.3 * sy, 0.03 * sz), "#5a4a3a", child(pelvis, [0, -0.17 * sy, 0.1 * sz]));
          gear.add("club", box(0.06 * sx, 0.5 * sy, 0.06 * sz), "#6a4a30", child(gear.rightHand, [0, -0.2 * sy, 0.05 * sz], tilt(0.35)));
          addShield(gear, "#6b5a45");
        },
      };
    case "mirefin":
      return {
        skin: MIREFIN_SKIN, tunic: MIREFIN_SKIN, trousers: "#3a6f6b", boots: "#2f5a57", stance: "sword-and-shield", hunch: 0.25,
        accessories: (gear) => {
          const { head, spine, sx, sy, sz } = gear;
          for (const [index, [z, height]] of ([[0.06, 0.14], [-0.02, 0.19], [-0.1, 0.13]] as const).entries()) {
            gear.add(`fin-${index}`, box(0.03 * sx, height * sy, 0.07 * sz), "#2f9a8f", child(head, [0, (0.11 + height / 2) * sy, z * sz], tilt(-0.35)));
          }
          for (const [name, side] of [["left", 1], ["right", -1]] as const) {
            gear.add(`eye-${name}`, box(0.045 * sx, 0.045 * sy, 0.04 * sz), accent, child(head, [side * 0.07 * sx, 0.05 * sy, 0.1 * sz]));
            gear.add(`fin-${name}`, box(0.02 * sx, 0.18 * sy, 0.12 * sz), "#2f9a8f", child(side > 0 ? gear.leftForearm : gear.rightForearm, [side * 0.03 * sx, -0.1 * sy, 0]));
          }
          gear.add("harness", box(0.45 * sx, 0.24 * sy, 0.27 * sz), accent, child(spine, [0, 0.36 * sy, 0]));
          gear.add("loincloth", box(0.3 * sx, 0.24 * sy, 0.03 * sz), accent, child(gear.pelvis, [0, -0.14 * sy, 0.1 * sz]));
        },
      };
    case "redbrand":
      return {
        skin: BANDIT_SKIN, tunic: accent, trousers: "#3a3330", boots: "#2a221c", stance: "sword-and-shield", hunch: 0.02,
        accessories: (gear) => {
          const { head, spine, sx, sy, sz } = gear;
          gear.add("bandana", box(0.27 * sx, 0.07 * sy, 0.27 * sz), "#e04a3a", child(head, [0, 0.07 * sy, 0]));
          gear.add("bandana-knot", box(0.05 * sx, 0.07 * sy, 0.06 * sz), "#e04a3a", child(head, [0, 0.06 * sy, -0.15 * sz], tilt(0.4)));
          gear.add("belt", box(0.44 * sx, 0.05 * sy, 0.26 * sz), "#2a221c", child(spine, [0, 0.02 * sy, 0]));
          addSword(gear);
          addShield(gear, "#7a2a22");
          if (elite) {
            gear.add("cloak", box(0.44 * sx, 0.8 * sy, 0.03 * sz), "#5d1717", child(spine, [0, 0.2 * sy, -0.14 * sz], tilt(gear.pose.cape)));
            for (const [name, side] of [["left", 1], ["right", -1]] as const) {
              gear.add(`pauldron-${name}`, box(0.13 * sx, 0.07 * sy, 0.14 * sz), "#3a3330", child(spine, [side * 0.25 * sx, 0.57 * sy, 0]));
            }
          }
        },
      };
  }
}

/** Collision half extents of a unit in metres: its template's box, or the 0.6 m × 1.8 m character box. */
export function unitHalfMetres(entity: EntityState, catalog: ContentCatalog, unitsPerMetre: number, playerHalfHeightUnits: number): [number, number, number] {
  const template = entity.kind === "creature" ? catalog.creatureTemplates.get(entity.appearance) : undefined;
  if (template) {
    const [x, y, z] = template.halfExtents;
    return [x / unitsPerMetre, y / unitsPerMetre, z / unitsPerMetre];
  }
  return [0.3, playerHalfHeightUnits / unitsPerMetre, 0.3];
}

/** The parts a creature or NPC model draws, or null when the entity has no model of its own. */
export function creatureModelParts(frame: UnitFrame): Part[] | null {
  const { entity, placement, locomotion, context } = frame;
  const choice = modelChoiceFor(entity, context.catalog);
  if (choice === "placeholder") {
    return null;
  }
  const half = unitHalfMetres(entity, context.catalog, context.unitsPerMetre, context.playerHalfHeightUnits);
  const dead = entity.flags.dead;
  const accent = stateColor(entity, context.catalog);
  switch (choice) {
    case "wolf":
    case "boar":
    case "vermin":
      return animalParts(choice, placement, half, accent, dead ? corpsePose() : quadrupedPoseFor(locomotion), dead);
    case "marauder":
    case "mirefin":
    case "redbrand": {
      const elite = context.catalog.creatureTemplates.get(entity.appearance)?.elite ?? false;
      const spec = creatureHumanoidSpec(choice, elite, accent);
      return humanoidParts(placement, half, spec, poseFor(locomotion, spec.stance), dead);
    }
    default: {
      const spec = NPC_SPECS[choice]();
      return humanoidParts(placement, half, spec, poseFor(locomotion, spec.stance), dead);
    }
  }
}

/** Nodes of a creature or NPC: its model and, for the viewer's target, the ring; the placeholder box when there is no model. */
function modelNodes(frame: UnitFrame): RendererSceneNode[] {
  const parts = creatureModelParts(frame);
  if (parts === null) {
    return PLACEHOLDER_BODY_MODEL.nodes(frame);
  }
  const { entity, placement, context, id } = frame;
  const nodes = partNodes(id, parts, entity.flags.dead);
  if (sameEntity({ kind: entity.kind, id: entity.entityId }, context.viewerTarget)) {
    const [hx, , hz] = unitHalfMetres(entity, context.catalog, context.unitsPerMetre, context.playerHalfHeightUnits);
    nodes.unshift(targetRingNode(id, placement, Math.max(hx, hz)));
  }
  return nodes;
}

/** The unit model of creatures and NPCs: feet under the collision box, as the placeholder stands. */
export const CREATURE_MODEL: UnitModel = {
  halfHeightUnits: (entity, context) => bodyHalfHeightUnits(entity, context.catalog, context.playerHalfHeightUnits),
  nodes: modelNodes,
};
