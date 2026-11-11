import type { BeginHold, Click, EndedGesture, GestureAim, HoldKind, MouseOwnerKind } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import type { ZoneSnapshot } from "../../replication";
import { wasm } from "../../wasm";

export type { BeginHold, Click, EndedGesture, GestureAim, HoldKind, MouseOwnerKind };

/**
 * One mouse owner at a time (#636, Atlas 5.2.i636-2): the shared `mmorpg-client-model` state
 * machine the native client also runs. Free look, focus, a strike or guard hold, manual aim or a
 * wheel; it says what motion and clicks mean and never decides an outcome.
 */
export class MouseOwnerInput {
  // Created on first use: the page builds its world view before `mmorpg-wasm` has loaded.
  #owners: InstanceType<typeof wasm.MouseOwners> | null = null;
  #focus = false;
  #holdCamera = "locked";

  get #model(): InstanceType<typeof wasm.MouseOwners> {
    if (!this.#owners) {
      this.#owners = new wasm.MouseOwners();
      this.#owners.setFocus(this.#focus);
      this.#owners.setHoldCamera(this.#holdCamera);
    }
    return this.#owners;
  }

  owner(): MouseOwnerKind {
    return this.#owners?.owner() ?? (this.#focus ? "focus" : "free-look");
  }

  /** Whether a strike, guard, aim or wheel owns the mouse. */
  get gesture(): boolean {
    const owner = this.owner();
    return owner !== "free-look" && owner !== "focus";
  }

  setFocus(focus: boolean): void {
    this.#focus = focus;
    this.#owners?.setFocus(focus);
  }

  setHoldCamera(value: "locked" | "free"): void {
    this.#holdCamera = value;
    this.#owners?.setHoldCamera(value);
  }

  beginHold(hold: HoldKind): BeginHold {
    return this.#model.beginHold(hold);
  }

  /** `hold`'s key went up deliberately: `true` when it owned the mouse and now acts. */
  releaseHold(hold: HoldKind): boolean {
    return this.#owners?.releaseHold(hold) ?? false;
  }

  leftClick(): Click {
    return this.#model.leftClick();
  }

  rightClick(): Click {
    return this.#model.rightClick();
  }

  /** Ends the gesture without firing. */
  interrupt(): EndedGesture {
    return this.#owners?.interrupt() ?? "none";
  }

  observe(projection: ZoneSnapshot): EndedGesture {
    return this.#owners && this.gesture ? this.#owners.observe(projection) : "none";
  }

  /** Mouse motion in CSS pixels; `true` when the camera turns with it. */
  motion(dx: number, dy: number): boolean {
    return this.#model.motion(dx, dy);
  }

  aim(): GestureAim {
    return this.#owners?.aim() ?? { strike: null, guard: null };
  }
}
