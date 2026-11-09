import type { ZoneSnapshot } from "../../replication";
import { wasm } from "../../wasm";

/** What the world view does when a held draw starts, is released or drops. */
export type PowerDrawEffects = {
  /** The bow draws at once: presentation only, nothing is sent. */
  prepare(ability: number): void;
  /** The draw drops without a shot. */
  abandon(): void;
  /** A deliberate release sends the shot once; the zone decides. */
  send(ability: number): void;
};

/**
 * Hold-to-draw for Aimed Shot and Piercing Shot (#607, Atlas 5.2.i607): the shared client model's
 * rule (`mmorpg-client-model`) applied to the world view's effects. Input timing only: the zone
 * alone decides whether the shot happens.
 */
export class PowerDrawInput {
  // Created on the first press: the page builds its world view, and can blur or close a draw
  // that never started, before `mmorpg-wasm` has loaded.
  #draw: InstanceType<typeof wasm.PowerDraw> | null = null;
  readonly #effects: PowerDrawEffects;

  constructor(effects: PowerDrawEffects) {
    this.#effects = effects;
  }

  get #model(): InstanceType<typeof wasm.PowerDraw> {
    this.#draw ??= new wasm.PowerDraw();
    return this.#draw;
  }

  /** A held input (with a release) pressed `slot` holding `ability`; `true` when it started a draw. */
  press(slot: number, ability: number, projection: ZoneSnapshot, interfaceOpen: boolean): boolean {
    const drawing = this.#model.press(slot, ability, projection, interfaceOpen);
    if (drawing) {
      this.#effects.prepare(ability);
    }
    return drawing;
  }

  /** `slot`'s key went up (`retired` when a retirement, not a deliberate release, let it go). */
  release(slot: number, retired: boolean): void {
    if (!this.#draw) { return; }
    const wasDrawing = this.#draw.drawing() !== undefined;
    const fire = this.#draw.release(slot, retired);
    if (fire !== undefined) {
      this.#effects.send(fire);
    } else if (wasDrawing && this.#draw.drawing() === undefined) {
      this.#effects.abandon();
    }
  }

  /** Drops the draw without a shot; `true` if there was one. */
  cancel(): boolean {
    const dropped = this.#draw?.cancel() ?? false;
    if (dropped) {
      this.#effects.abandon();
    }
    return dropped;
  }

  /** Follows a projection: a draw that became unusable, or that an opened panel interrupts, drops. */
  observe(projection: ZoneSnapshot, interfaceOpen: boolean): void {
    if (this.#draw && this.#draw.drawing() !== undefined && this.#draw.observe(projection, interfaceOpen)) {
      this.#effects.abandon();
    }
  }

  get drawing(): boolean {
    return this.#draw?.drawing() !== undefined;
  }
}
