import type { ContentCatalog, CreatureFamily } from "../catalog";
import type { EntityState } from "../../replication";
import type { UnitPlacement } from "../humanoid";

/** One unit of the current projection, as the `?debug` readout lists it. Positions are metres. */
export type ProjectedUnit = {
  readonly kind: string;
  readonly id: number;
  readonly family: CreatureFamily | null;
  readonly x: number;
  readonly z: number;
};

/** Families drawn as low-poly quadrupeds. */
export const ANIMAL_FAMILIES: readonly CreatureFamily[] = ["wolf", "boar", "vermin"];

export function projectedUnit(entity: Pick<EntityState, "kind" | "entityId" | "appearance">, placement: Pick<UnitPlacement, "x" | "z">, catalog: ContentCatalog): ProjectedUnit {
  const family = entity.kind === "creature" ? (catalog.creatureTemplates.get(entity.appearance)?.family ?? null) : null;
  return { kind: entity.kind, id: entity.entityId, family, x: placement.x, z: placement.z };
}

/** The projected animal (wolf, boar or vermin) nearest to a point, or null when none is projected. */
export function nearestAnimal(units: readonly ProjectedUnit[], x: number, z: number): ProjectedUnit | null {
  let best: ProjectedUnit | null = null;
  let bestDistance = Infinity;
  for (const unit of units) {
    if (unit.kind !== "creature" || !unit.family || !ANIMAL_FAMILIES.includes(unit.family)) {
      continue;
    }
    const distance = Math.hypot(unit.x - x, unit.z - z);
    if (distance < bestDistance || (distance === bestDistance && best !== null && unit.id < best.id)) {
      best = unit;
      bestDistance = distance;
    }
  }
  return best;
}
