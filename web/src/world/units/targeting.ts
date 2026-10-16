import type { WorldCommand } from "../../command-wire";
import { sameEntity, type EntityRef } from "../../entity-ref";
import type { ZoneSnapshot } from "../../replication";

/**
 * Tab targeting: the nearest living attackable creature in the projection.
 * Pressing again while one of them is selected cycles outward to the next.
 * Selection is only intent; the zone validates it.
 */
export function nextTabTarget(snapshot: ZoneSnapshot): EntityRef | null {
  const viewer = snapshot.entities[0];
  if (!viewer) {
    return null;
  }
  const candidates = snapshot.entities
    .filter((entity) => entity.kind === "creature" && entity.flags.attackable)
    .map((entity) => {
      const dx = entity.position[0] - viewer.position[0];
      const dz = entity.position[2] - viewer.position[2];
      return { target: { kind: entity.kind, id: entity.entityId }, distance: dx * dx + dz * dz };
    })
    .sort((left, right) => left.distance - right.distance || left.target.id - right.target.id);
  if (candidates.length === 0) {
    return null;
  }
  const current = candidates.findIndex(({ target }) => sameEntity(target, snapshot.viewer.target));
  return candidates[(current + 1) % candidates.length]?.target ?? null;
}

/** F or a right click: start auto-attacking, or stop if already attacking. */
export function attackToggle(snapshot: ZoneSnapshot): WorldCommand {
  return snapshot.viewer.autoAttacking ? { kind: "stop-attack" } : { kind: "start-attack" };
}

/** Moving the pointer this far turns a right click into a turn drag. */
export const CLICK_SLOP_PIXELS = 4;

/** The pointer fields a click tracker reads; a `PointerEvent` in the page. */
export type PointerSample = { type: string; pointerId: number; button: number; clientX: number; clientY: number };

/**
 * Tells a click from a camera drag: a matching-button press that ends
 * with `pointerup` within `CLICK_SLOP_PIXELS` of where it started is a click.
 * Anything else stays a drag or cancellation.
 */
export class PointerClick {
  #press: { pointerId: number; x: number; y: number } | null = null;

  constructor(readonly button: 0 | 2) {}

  down(event: PointerSample): void {
    this.#press = event.button === this.button ? { pointerId: event.pointerId, x: event.clientX, y: event.clientY } : null;
  }

  move(event: PointerSample): void {
    const press = this.#press;
    if (press?.pointerId === event.pointerId && Math.hypot(event.clientX - press.x, event.clientY - press.y) > CLICK_SLOP_PIXELS) {
      this.#press = null;
    }
  }

  /** Whether this pointer event ends a press as a click; any end forgets the press. */
  end(event: PointerSample): boolean {
    this.move(event);
    if (this.#press?.pointerId !== event.pointerId) {
      return false;
    }
    this.#press = null;
    return event.type === "pointerup" && event.button === this.button;
  }

  reset(): void {
    this.#press = null;
  }
}

/** Existing secondary-button attack gesture, sharing the camera's click/drag threshold. */
export class SecondaryClick extends PointerClick {
  constructor() { super(2); }
}
