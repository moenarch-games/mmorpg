import type * as THREE from "three";

/**
 * The world frame as ordered stages (#1060). `WorldView.frame()` runs one presenter per stage over one reused context;
 * render roots live in a registry that rebuilds only when a root owner reports a change, and panels redraw only when
 * their projection or source revision moves. The counters here back the `world-frame` budget in
 * `.performance/contract.json`.
 */

/** The world frame's stages, in the order they run. */
export const WORLD_FRAME_STAGES = ["input", "replication", "units", "camera", "scenery", "render", "hud", "panels"] as const;
export type WorldFrameStage = (typeof WORLD_FRAME_STAGES)[number];

/** One stage's presenter: it reads and writes the frame's shared context. */
export interface FramePresenter<Context> {
  readonly stage: WorldFrameStage;
  update(context: Context): void;
}

/** Something that draws imported render roots: the set it draws changes only when its revision moves. */
export interface RootPresenter {
  /** Moves whenever `collectRoots` may append a different set. */
  readonly rootsRevision: number;
  /** Appends the roots drawn now; reading them changes nothing. */
  collectRoots(into: THREE.Object3D[]): void;
}

/** What a map view (`plan`) hides of a root: inhabitants always, everything but the ground on a ground-only map. */
export type RootRole = "world" | "inhabitant" | "ground";

/** One registered owner of roots; `revision` and `collect` read through to an owner that may be absent or replaced. */
export type RootEntry = {
  readonly role: RootRole;
  revision(): number;
  collect(into: THREE.Object3D[]): void;
};

/** Deterministic work of the frames since the last `take` (the `world-frame` budget). */
export type FrameWork = {
  /** Presenter stage calls. */
  stageCalls: number;
  /** Panel redraws the stage ran (a received projection, a changed latest projection or bag revision). */
  panelRenders: number;
  /** Rebuilds of the render-root list. */
  rootRebuilds: number;
};

export function emptyFrameWork(): FrameWork {
  return { stageCalls: 0, panelRenders: 0, rootRebuilds: 0 };
}

/** Runs registered presenters in stage order; a presenter may stop the rest of the frame. */
export class FrameStages<Context extends { stopped: boolean }> {
  readonly #presenters: readonly FramePresenter<Context>[];
  readonly #work: FrameWork;

  constructor(presenters: readonly FramePresenter<Context>[], work: FrameWork) {
    const order = presenters.map((presenter) => WORLD_FRAME_STAGES.indexOf(presenter.stage));
    if (order.some((index, at) => index < 0 || (at > 0 && index <= order[at - 1]!))) {
      throw new Error(`Frame presenters must follow the stage order ${WORLD_FRAME_STAGES.join(", ")}.`);
    }
    this.#presenters = presenters;
    this.#work = work;
  }

  run(context: Context): void {
    context.stopped = false;
    for (const presenter of this.#presenters) {
      this.#work.stageCalls += 1;
      presenter.update(context);
      if (context.stopped) { return; }
    }
  }
}

/**
 * The render-root list. It rebuilds only when an entry's revision moved or the caller marked it dirty (a replaced owner,
 * a changed detail level or map view), so an unchanged frame reads one number per entry and allocates nothing.
 */
export class RootRegistry {
  #entries: readonly RootEntry[] = [];
  #seen: number[] = [];
  #roots: THREE.Object3D[] = [];
  #roles: RootRole[] = [];
  #dirty = true;
  readonly #work: FrameWork;

  constructor(work: FrameWork) {
    this.#work = work;
  }

  /** Replaces every entry (a newly loaded world); the next refresh rebuilds. */
  register(entries: readonly RootEntry[]): void {
    this.#entries = entries;
    this.#seen = entries.map(() => Number.NaN);
    this.#dirty = true;
  }

  /** A condition the entries read outside their revisions changed. */
  markDirty(): void {
    this.#dirty = true;
  }

  /** Rebuilds the list if anything changed; true when it did. */
  refresh(): boolean {
    let changed = this.#dirty;
    for (let index = 0; index < this.#entries.length; index += 1) {
      const revision = this.#entries[index]!.revision();
      if (revision !== this.#seen[index]) {
        this.#seen[index] = revision;
        changed = true;
      }
    }
    if (!changed) { return false; }
    this.#dirty = false;
    const roots: THREE.Object3D[] = [];
    const roles: RootRole[] = [];
    for (const entry of this.#entries) {
      const from = roots.length;
      entry.collect(roots);
      for (let index = from; index < roots.length; index += 1) { roles.push(entry.role); }
    }
    this.#roots = roots;
    this.#roles = roles;
    this.#work.rootRebuilds += 1;
    return true;
  }

  /** The roots of the last rebuild, in registration order. */
  get roots(): readonly THREE.Object3D[] {
    return this.#roots;
  }

  /** The role of `roots[index]`. */
  role(index: number): RootRole {
    return this.#roles[index]!;
  }
}

/** Whether a panel source moved since the panels last drew it: the latest projection object or a source revision. */
export class PanelGate<Projection> {
  #projection: Projection | null = null;
  #revision = Number.NaN;

  /** True (and remembered) when `projection` or `revision` differ from the last accepted pair. */
  changed(projection: Projection, revision: number): boolean {
    if (projection === this.#projection && revision === this.#revision) { return false; }
    this.#projection = projection;
    this.#revision = revision;
    return true;
  }

  reset(): void {
    this.#projection = null;
    this.#revision = Number.NaN;
  }
}

/** A root entry over an owner read through `owner()`, absent while it returns null. */
export function rootEntry(role: RootRole, owner: () => RootPresenter | null | undefined, shown: () => boolean = always): RootEntry {
  return {
    role,
    revision: () => owner()?.rootsRevision ?? -1,
    collect: (into) => { if (shown()) { owner()?.collectRoots(into); } },
  };
}

/** A root entry for a fixed root owned by `owner()` (one that never swaps its root, only its contents). */
export function fixedRootEntry(role: RootRole, owner: () => { readonly root: THREE.Object3D } | null | undefined, shown: () => boolean = always): RootEntry {
  return {
    role,
    revision: () => (owner() ? 0 : -1),
    collect: (into) => {
      const root = owner()?.root;
      if (root && shown()) { into.push(root); }
    },
  };
}

function always(): boolean {
  return true;
}
