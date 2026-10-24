/**
 * Intent-ordered art prefetch (#412, Atlas 6.2.5). Character selection is background
 * preparation time: the selected character and its saved area load first, other likely
 * choices only once that foreground work has settled, one piece at a time.
 *
 * Foreground pieces start at once and in parallel. Background pieces wait until no foreground
 * piece is in flight, then start one at a time in plan order. A new plan replaces every queued
 * background piece, so a selection change drops stale choices. Started loads cannot be aborted
 * (three.js loaders are not abortable); they finish and stay loaded. Each key starts once.
 */
export type PrefetchPlan = {
  /** Needed now: the selected character's preview and its entry area's pieces. */
  readonly foreground: readonly string[];
  /** Likely later, most likely first: other characters' areas, then adjacent areas. */
  readonly background: readonly string[];
};

export class ArtPrefetch {
  readonly #start: (key: string) => Promise<unknown>;
  readonly #onSettled: (idle: boolean) => void;
  readonly #started = new Set<string>();
  readonly #settled = new Set<string>();
  #queued: string[] = [];
  #foregroundInFlight = 0;
  #backgroundInFlight = 0;

  /** `start` loads one piece; `onSettled` runs after each settles, `idle` once none is in flight. */
  constructor(start: (key: string) => Promise<unknown>, onSettled: (idle: boolean) => void = () => {}) {
    this.#start = start;
    this.#onSettled = onSettled;
  }

  /** Starts `plan.foreground` now and makes `plan.background` the whole background queue. */
  plan(plan: PrefetchPlan): void {
    this.request(plan.foreground);
    const foreground = new Set(plan.foreground);
    this.#queued = [...new Set(plan.background)].filter((key) => !foreground.has(key) && !this.#started.has(key));
    this.#pump();
  }

  /** Starts `keys` now as foreground work, keeping the queued background plan. */
  request(keys: readonly string[]): void {
    for (const key of keys) { this.#launch(key, true); }
    this.#queued = this.#queued.filter((key) => !this.#started.has(key));
  }

  started(key: string): boolean { return this.#started.has(key); }
  settled(key: string): boolean { return this.#settled.has(key); }
  /** Background pieces still waiting, in start order. */
  get queued(): readonly string[] { return this.#queued; }

  #launch(key: string, foreground: boolean): void {
    if (this.#started.has(key)) { return; }
    this.#started.add(key);
    if (foreground) { this.#foregroundInFlight += 1; } else { this.#backgroundInFlight += 1; }
    const done = () => {
      this.#settled.add(key);
      if (foreground) { this.#foregroundInFlight -= 1; } else { this.#backgroundInFlight -= 1; }
      this.#pump();
      this.#onSettled(this.#foregroundInFlight + this.#backgroundInFlight === 0);
    };
    void Promise.resolve().then(() => this.#start(key)).then(done, done);
  }

  #pump(): void {
    if (this.#foregroundInFlight > 0 || this.#backgroundInFlight > 0) { return; }
    const next = this.#queued.shift();
    if (next !== undefined) { this.#launch(next, false); }
  }
}
