import type { EntityRef } from "../../entity-ref";
import type { ZoneSnapshot } from "../../replication";
import { wasm } from "../../wasm";

/**
 * Tab targeting (`mmorpg-client-model`'s `targeting`, #1280): the nearest living attackable player or creature in the
 * projection; pressing again while one of them is selected cycles outward to the next. Read once per press. Selection
 * is only intent; the zone validates it.
 */
export function nextTabTarget(snapshot: ZoneSnapshot): EntityRef | null {
  return wasm.nextTabTarget(snapshot) ?? null;
}

/** Only a living, projected opponent can own combat camera focus. NPC dialogue is not focus. */
export function combatFocus(snapshot: ZoneSnapshot): EntityRef | null {
  const selected = snapshot.viewer.target;
  if (!selected || snapshot.viewer.dead) { return null; }
  const entity = snapshot.entities.find((candidate) => candidate.kind === selected.kind && candidate.entityId === selected.id);
  return entity && (entity.kind === "creature" || entity.kind === "player") && entity.flags.attackable && !entity.flags.dead
    ? selected : null;
}

/** Small hand movements should not cancel an intentional click. */
export const CLICK_SLOP_PIXELS = 6;
export const TOUCH_CLICK_SLOP_PIXELS = 12;

/** The pointer fields a click tracker reads; a `PointerEvent` in the page. */
export type PointerSample = { type: string; pointerId: number; button: number; clientX: number; clientY: number; pointerType?: string };

/**
 * Tells a click from a camera drag: a matching-button press that ends
 * with `pointerup` within the mouse/touch movement tolerance is a click.
 * Anything else stays a drag or cancellation.
 */
export class PointerClick {
  #press: { pointerId: number; x: number; y: number; slop: number } | null = null;

  constructor(readonly button: 0 | 2) {}

  down(event: PointerSample): void {
    this.#press = event.button === this.button ? {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      slop: event.pointerType === "touch" ? TOUCH_CLICK_SLOP_PIXELS : CLICK_SLOP_PIXELS,
    } : null;
  }

  move(event: PointerSample): void {
    const press = this.#press;
    if (press?.pointerId === event.pointerId && Math.hypot(event.clientX - press.x, event.clientY - press.y) > press.slop) {
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
