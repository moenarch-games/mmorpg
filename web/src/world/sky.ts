import { skyAt, type EnvironmentStyle } from "./environment";

/**
 * The CSS sky behind the transparent world canvas: a gradient from the
 * zenith to a hazy horizon that tracks the camera's horizon line, a slow
 * cloud layer (CSS, paused under reduced motion) and a subtle time-of-day
 * drift. Writes custom properties only when their value changes.
 */
export class SkyLayer {
  readonly #element: HTMLElement;
  readonly #style: EnvironmentStyle;
  readonly #written = new Map<string, string>();

  constructor(element: HTMLElement, style: EnvironmentStyle) {
    this.#element = element;
    this.#style = style;
  }

  show(visible: boolean): void {
    this.#element.hidden = !visible;
  }

  /**
   * `horizon` is the horizon's height on screen as a fraction from the top;
   * `dayFraction` is the day's fraction since noon (the shared `clock::day_fraction`).
   */
  update(horizon: number, dayFraction: number): void {
    const stops = skyAt(this.#style, dayFraction);
    const clamped = Math.min(1.5, Math.max(-0.5, Number.isFinite(horizon) ? horizon : 0.5));
    this.#set("--sky-zenith", stops.zenith);
    this.#set("--sky-upper", stops.upper);
    this.#set("--sky-horizon", stops.horizon);
    this.#set("--sky-ground", stops.ground);
    this.#set("--horizon", `${(clamped * 100).toFixed(1)}%`);
  }

  #set(name: string, value: string): void {
    if (this.#written.get(name) !== value) {
      this.#written.set(name, value);
      this.#element.style.setProperty(name, value);
    }
  }
}
