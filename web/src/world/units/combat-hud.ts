import type { ZoneSnapshot } from "../../replication";
import { hudTexts } from "./hud-texts";

/**
 * Plain-text combat HUD lines beside the class HUD's frames, cast bars and combat text
 * (`class-hud.ts`). Their words (status, event lines, refusals and unit names) come from
 * `mmorpg-client-model`'s `texts` (#1278), the native client's rules, once per projection.
 */

/** How long a feedback line stays up, in milliseconds. */
export const FEEDBACK_MS = 3_000;

/** A HUD line the combat HUD writes; an `HTMLElement` in the page. */
export type HudLine = { textContent: string | null };

/**
 * The two combat HUD lines: the viewer's status, and the latest feedback
 * event, which fades after `FEEDBACK_MS`. Lines are written only when their
 * text changes.
 */
export class CombatHud {
  readonly #status: HudLine;
  readonly #feedback: HudLine;
  #latest: { text: string; until: number } | null = null;
  #tick = -1n;

  constructor(status: HudLine, feedback: HudLine) {
    this.#status = status;
    this.#feedback = feedback;
  }

  /** Clears both lines for a new session. */
  reset(): void {
    this.#latest = null;
    this.#tick = -1n;
    this.#status.textContent = "";
    this.#feedback.textContent = "";
  }

  /** Reads one projection; its events are read once, however many frames show it. */
  update(snapshot: ZoneSnapshot, now: number): void {
    const texts = hudTexts(snapshot);
    write(this.#status, texts.status);
    if (snapshot.tick !== this.#tick) {
      this.#tick = snapshot.tick;
      if (texts.feedback !== null) {
        this.#latest = { text: texts.feedback, until: now + FEEDBACK_MS };
      }
    }
    write(this.#feedback, this.#latest && this.#latest.until > now ? this.#latest.text : "");
  }
}

function write(line: HudLine, text: string): void {
  if (line.textContent !== text) {
    line.textContent = text;
  }
}
