import type { ZoneSnapshot } from "../../replication";
import { FEEDBACK_MS, type HudLine } from "./combat-hud";

type ProgressBar = { max: number; value: number };

/** Displays durable self state; no XP curve or award logic lives in the HUD. */
export class ProgressionHud {
  readonly #bar: ProgressBar;
  readonly #status: HudLine;
  readonly #feedback: HudLine;
  #tick = -1n;
  #level: number | null = null;
  #latest: { text: string; until: number } | null = null;

  constructor(bar: ProgressBar, status: HudLine, feedback: HudLine) {
    this.#bar = bar;
    this.#status = status;
    this.#feedback = feedback;
  }

  reset(): void {
    this.#tick = -1n;
    this.#level = null;
    this.#latest = null;
    this.#bar.max = 1;
    this.#bar.value = 0;
    this.#status.textContent = "";
    this.#feedback.textContent = "";
  }

  update(snapshot: ZoneSnapshot, now: number): void {
    if (snapshot.tick > this.#tick) {
      this.#tick = snapshot.tick;
      const viewer = snapshot.viewer;
      const capped = viewer.experienceToNextLevel === 0;
      this.#bar.max = capped ? 1 : viewer.experienceToNextLevel;
      this.#bar.value = capped ? 1 : viewer.experience;
      this.#status.textContent = capped
        ? `Level ${viewer.level} · Maximum level`
        : `Level ${viewer.level} · ${viewer.experience} / ${viewer.experienceToNextLevel} XP`;
      if (this.#level !== null && viewer.level > this.#level) {
        this.#latest = { text: `You reached level ${viewer.level}!`, until: now + FEEDBACK_MS };
      }
      this.#level = viewer.level;
    }
    this.#feedback.textContent = this.#latest && this.#latest.until > now ? this.#latest.text : "";
  }
}
