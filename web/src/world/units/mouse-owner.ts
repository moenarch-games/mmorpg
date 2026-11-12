import type { AimPress, BeginHold, Click, EndedGesture, GestureAim, HoldKind, MouseOwnerKind, WheelKindName, WheelReleaseView, WheelView } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import type { ZoneSnapshot } from "../../replication";
import { wasm } from "../../wasm";

export type { AimPress, BeginHold, Click, EndedGesture, GestureAim, HoldKind, MouseOwnerKind, WheelKindName, WheelReleaseView, WheelView };

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
  #aimStyle = "hold";

  get #model(): InstanceType<typeof wasm.MouseOwners> {
    if (!this.#owners) {
      this.#owners = new wasm.MouseOwners();
      this.#owners.setFocus(this.#focus);
      this.#owners.setHoldCamera(this.#holdCamera);
      this.#owners.setAimStyle(this.#aimStyle);
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

  setAimStyle(value: "hold" | "armed"): void {
    this.#aimStyle = value;
    this.#owners?.setAimStyle(value);
  }

  /**
   * A held input pressed `slot` holding `ability` (#649): `use` it as usual, it now `aim`s (nothing
   * is sent yet), or another gesture owns the mouse and the press does nothing.
   */
  pressAim(slot: number, ability: number, projection: ZoneSnapshot, interfaceOpen: boolean): AimPress {
    return this.#model.pressAim(slot, ability, projection, interfaceOpen);
  }

  /** `slot` went up (or was `retired`): the ability to throw along the aim, if it throws now. */
  releaseAim(slot: number, retired: boolean): number | undefined {
    return this.#owners?.releaseAim(slot, retired);
  }

  /** Where a throw along `facing` is expected to land, in core units; an aid, never an outcome. */
  aimLanding(projection: ZoneSnapshot, facing: number): [number, number, number] | null {
    const landing = this.#owners?.aimLanding(projection, facing);
    return landing && landing.length === 3 ? [landing[0]!, landing[1]!, landing[2]!] : null;
  }

  /**
   * A wheel key went down (#653): opens the fixed wheel from its sector `choices` or the context
   * wheel from the projection; `true` if a wheel is open now.
   */
  openWheel(kind: WheelKindName, choices: string[], projection: ZoneSnapshot): boolean {
    return this.#model.openWheel(kind, choices, projection);
  }

  /** The open wheel, labelled for the viewer's class, or `null`. */
  wheel(projection: ZoneSnapshot): WheelView | null {
    return this.#owners && this.owner() === "wheel" ? this.#owners.wheel(projection) as WheelView | null : null;
  }

  /** The wheel key went up after `heldMs`, or was retired: pick, stay open or cancel. */
  releaseWheel(heldMs: number, retired: boolean): WheelReleaseView {
    return this.#owners?.releaseWheel(heldMs, retired) ?? { outcome: "ignored", action: null };
  }

  /** Turns the opt-in local wheel usage counters on from their stored JSON, or off with `null`. */
  setUsageCounting(stored: string | null): void {
    this.#model.setUsageCounting(stored ?? undefined);
  }

  /** `ms` milliseconds passed under the current owner; returns the counters' JSON while counting. */
  countUsage(ms: number): string | undefined {
    if (!this.#owners) { return undefined; }
    this.#owners.countUsage(ms);
    return this.#owners.usageJson();
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
