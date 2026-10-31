import type { Axis, WorldCommand } from "../command-wire";
import { wasm } from "../wasm";

/**
 * Local movement intent. `jumps` counts Space presses so coalesced frames
 * can merge presses but never replay them.
 */
export type MovementInput = {
  forward: Axis;
  strafe: Axis;
  facing: number;
  jumps: number;
  /** Held swim ascent or descent (#274); absent means neither. */
  rise?: Axis;
  /** Counts dodge-roll presses (#516) like `jumps`; absent means none. */
  rolls?: number;
};

/**
 * Which movement commands the current input needs: the shared client model's outbox
 * (`mmorpg-client-model`, #543), the same one the native client runs. It performs no I/O.
 */
export class MovementOutbox {
  readonly #outbox: InstanceType<typeof wasm.MovementOutbox>;

  constructor(input: MovementInput) {
    this.#outbox = new wasm.MovementOutbox(input.jumps, input.rolls ?? 0);
  }

  /** Starts a new session: only current input counts, earlier presses are dropped. */
  reset(input: MovementInput): void {
    this.#outbox.reset(input.jumps, input.rolls ?? 0);
  }

  /** Commands for the current input; `dead` comes from the latest projection. */
  update(held: MovementInput, nowMs: number, dead = false): WorldCommand[] {
    return this.#outbox.update(
      held.forward, held.strafe, held.facing, held.jumps, held.rolls ?? 0, held.rise ?? 0, nowMs, dead,
    );
  }
}
