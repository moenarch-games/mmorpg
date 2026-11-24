/**
 * The selection screen's prebuild of the entry view (#855, owner decision 2026-11-23: prebuild during selection).
 *
 * While the player chooses a character, the world view builds its static scene, composes the view at the selected
 * character's expected entry and prepares that view's programs and textures, one idle slice at a time, so Enter World
 * joins into a scene that is already built. A different character or slot redoes only what differs: the static scene
 * stays, the view is composed and prepared again. Nothing here joins or touches gameplay.
 *
 * The counters back the `entry-prebuild` budget in `.performance/contract.json`.
 */

/**
 * Where the selected character is expected to enter (`entryPreview`): its feet and body centre height in core units, its
 * facing in radians (0 toward +Z) and the zone tick of its first projection.
 */
export type EntryPreview = { feet: [number, number, number]; centreY: number; facing: number; tick: bigint };

/**
 * The static scene's parts (`WorldView`), one per idle slice: the page's module realizing its seeded scene (its terrain
 * tiles), the decoded scene, the batches its groups stream into, the ground and the grass. The grass comes last: its
 * seed reaches the page after the scene's.
 */
export const STATIC_PARTS = 5;

/** What the prebuild drives: the world view's static build, its composed entry view and its prepare. */
export interface PrebuildTarget<Entry> {
  /** Whether the static scene is complete. */
  staticBuilt(): boolean;
  /** Whether the next static part's input has reached the page; until then the prebuild waits, unsettled. */
  staticPartReady(): boolean;
  /** Builds the next part of the static scene. */
  buildStaticPart(): void;
  /** One compose round of the view at `entry`; true once nothing more streams in around it. */
  compose(entry: Entry): boolean;
  /** What preparing `entry`'s view would prepare now (view, lighting, roots); a prepare reruns when it moves. */
  prepareKey(entry: Entry): string;
  /** Prepares `entry`'s view: its programs and textures, drawing nothing. */
  prepare(entry: Entry): Promise<void>;
}

/** Runs `slice` in a later idle moment. */
export type IdleScheduler = (slice: () => void) => void;

/** Deterministic work of the prebuild (the `entry-prebuild` budget). */
export type PrebuildWork = {
  /** Static scene parts built. */
  staticParts: number;
  /** Compose rounds of an entry view. */
  composeRounds: number;
  /** Entry views prepared. */
  prepares: number;
  /** Idle slices run. */
  slices: number;
};

/** A compose settles within this many rounds, even if something still streams in (the first frames keep streaming). */
export const MAX_COMPOSE_ROUNDS = 12;
/** An entry's view is prepared again when its roots change (art arriving), at most this often. */
export const MAX_PREPARES_PER_ENTRY = 4;

/** The browser's idle callback, or a timer without one. */
export const idleScheduler: IdleScheduler = (slice) => {
  if (typeof requestIdleCallback === "function") { requestIdleCallback(() => slice(), { timeout: 1_000 }); } else { setTimeout(slice, 0); }
};

export class EntryPrebuild<Entry> {
  readonly #target: PrebuildTarget<Entry>;
  readonly #schedule: IdleScheduler;
  readonly work: PrebuildWork = { staticParts: 0, composeRounds: 0, prepares: 0, slices: 0 };
  #active = false;
  #scheduled = false;
  #entry: Entry | null = null;
  #entryKey: string | null = null;
  #allowPrepare = false;
  #composedKey: string | null = null;
  #rounds = 0;
  #preparedKey: string | null = null;
  #prepares = 0;
  #preparing = false;

  constructor(target: PrebuildTarget<Entry>, schedule: IdleScheduler = idleScheduler) {
    this.#target = target;
    this.#schedule = schedule;
  }

  /**
   * Prebuilds toward `entry` (keyed by `entryKey`; null builds only the static scene), preparing its view once `prepare`
   * allows it. Cheap to call again with the same arguments; call again once a waiting static part's input arrived.
   */
  request(entry: Entry | null, entryKey: string | null, prepare: boolean): void {
    this.#active = true;
    if (entryKey !== this.#entryKey) {
      this.#entry = entry;
      this.#entryKey = entryKey;
      this.#rounds = 0;
      this.#prepares = 0;
    }
    this.#allowPrepare = prepare;
    this.#next();
  }

  /** Stops prebuilding (the page enters the world); what was built stays. */
  pause(): void {
    this.#active = false;
  }

  /** Forgets every view composed or prepared (a newly loaded world); the next request starts over. */
  reset(): void {
    this.#active = false;
    this.#entry = null;
    this.#entryKey = null;
    this.#composedKey = null;
    this.#preparedKey = null;
    this.#rounds = 0;
    this.#prepares = 0;
  }

  /**
   * Whether the requested entry's view is composed, and prepared if allowed, so the prebuild has nothing left to do; not
   * while a static part waits for its input.
   */
  get settled(): boolean {
    return this.#pending() === null && !this.#preparing;
  }

  /** Whether the requested entry's view has been prepared. */
  get prepared(): boolean {
    return this.#entryKey !== null && this.#preparedKey !== null && this.#preparedKey.startsWith(`${this.#entryKey}|`);
  }

  #pending(): "static" | "waiting" | "compose" | "prepare" | null {
    if (!this.#target.staticBuilt()) { return this.#target.staticPartReady() ? "static" : "waiting"; }
    const entry = this.#entry;
    if (entry === null || this.#entryKey === null) { return null; }
    if (this.#composedKey !== this.#entryKey) { return "compose"; }
    if (!this.#allowPrepare || this.#prepares >= MAX_PREPARES_PER_ENTRY) { return null; }
    return this.#preparedKey === this.#prepareKey(entry) ? null : "prepare";
  }

  #prepareKey(entry: Entry): string {
    return `${this.#entryKey}|${this.#target.prepareKey(entry)}`;
  }

  #next(): void {
    if (!this.#active || this.#scheduled || this.#preparing) { return; }
    const pending = this.#pending();
    if (pending === null || pending === "waiting") { return; }
    this.#scheduled = true;
    this.#schedule(() => this.#slice());
  }

  /** One step per idle slice, so input and the turntable's frames run between them. */
  #slice(): void {
    this.#scheduled = false;
    if (!this.#active) { return; }
    const pending = this.#pending();
    if (pending === null || pending === "waiting") { return; }
    this.work.slices += 1;
    const started = typeof performance === "undefined" ? 0 : performance.now();
    if (pending === "static") {
      this.work.staticParts += 1;
      this.#target.buildStaticPart();
    } else if (pending === "compose") {
      this.work.composeRounds += 1;
      this.#rounds += 1;
      if (this.#target.compose(this.#entry!) || this.#rounds >= MAX_COMPOSE_ROUNDS) { this.#composedKey = this.#entryKey; }
    } else if (pending === "prepare") {
      const key = this.#prepareKey(this.#entry!);
      this.work.prepares += 1;
      this.#prepares += 1;
      this.#preparing = true;
      void this.#target.prepare(this.#entry!).catch((error: unknown) => {
        console.warn("Prebuilding the entry view failed; Enter World prepares it instead.", error);
      }).finally(() => {
        this.#preparing = false;
        this.#preparedKey = key;
        this.#next();
      });
      measure(pending, started);
      return;
    }
    measure(pending === "static" ? `static-${this.work.staticParts}` : pending, started);
    this.#next();
  }
}

/** A slice in the browser's performance timeline (`vale:prebuild:<step>`), for profiles and traces. */
function measure(step: string, start: number): void {
  if (typeof performance === "undefined" || typeof performance.measure !== "function") { return; }
  performance.measure(`vale:prebuild:${step}`, { start, end: performance.now() });
}
