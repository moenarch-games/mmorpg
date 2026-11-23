import * as THREE from "three";
import {
  FrameStages,
  PanelGate,
  RootRegistry,
  WORLD_FRAME_STAGES,
  emptyFrameWork,
  fixedRootEntry,
  rootEntry,
  type FramePresenter,
  type FrameWork,
  type RootPresenter,
} from "../../../src/world/frame-stages";
import type { ZoneSnapshot } from "../../../src/replication";
import { BagState } from "../../../src/world/units/bag-state";
import { ActionBarCache } from "../../../src/world/units/ability-state";
import { playerEntity, testSnapshot } from "../snapshots";

// The `world-frame` measurement (#1060, #1120): `web/tests/frame-stages.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. The frame below wires the same pieces `WorldView.frame()`
// does: one presenter per stage, received projections fanned out to the panels, the latest one gated by projection and
// bag revision, and the render roots kept in the registry.

export function projection(tick: number): ZoneSnapshot {
  return testSnapshot({ zoneId: 1, contentRevision: 4n, viewerId: 1, tick: BigInt(tick), acknowledgedSequence: 0,
    entities: [playerEntity(1, [0, 90, 0])], inventoryRevision: 1n, inventory: Array<null>(16).fill(null) });
}

/** A root owner that streams in: its root set changes only when it loads or drops a root. */
export class Owner implements RootPresenter {
  readonly #roots: THREE.Object3D[] = [];
  rootsRevision = 0;
  add(root: THREE.Object3D): void { this.#roots.push(root); this.rootsRevision += 1; }
  collectRoots(into: THREE.Object3D[]): void { for (const root of this.#roots) { into.push(root); } }
}

type Frame = { stopped: boolean; received: ZoneSnapshot[]; latest: ZoneSnapshot | null; submitted: number };

export function world() {
  const work = emptyFrameWork();
  const bags = new BagState();
  const gate = new PanelGate<ZoneSnapshot>();
  const registry = new RootRegistry(work);
  const models = new Owner();
  const kits = new Owner();
  const ground = { root: new THREE.Group() };
  registry.register([rootEntry("inhabitant", () => models), rootEntry("world", () => kits), fixedRootEntry("ground", () => ground)]);
  const inbox: ZoneSnapshot[] = [];
  const stage = (name: (typeof WORLD_FRAME_STAGES)[number], update: (frame: Frame) => void): FramePresenter<Frame> => ({ stage: name, update });
  const stages = new FrameStages<Frame>([
    stage("input", () => {}),
    stage("replication", (frame) => {
      frame.received = inbox.splice(0);
      for (const received of frame.received) {
        bags.update(received);
        work.panelRenders += 9;
        frame.latest = received;
      }
    }),
    stage("units", () => {}),
    stage("camera", () => {}),
    stage("scenery", () => {}),
    stage("render", (frame) => { if (registry.refresh()) { frame.submitted = registry.roots.length; } }),
    stage("hud", () => {}),
    stage("panels", (frame) => {
      if (frame.latest && gate.changed(frame.latest, bags.changes)) { work.panelRenders += 3; }
    }),
  ], work);
  const frame: Frame = { stopped: false, received: [], latest: null, submitted: 0 };
  const take = (): FrameWork => { const taken = { ...work }; Object.assign(work, emptyFrameWork()); return taken; };
  return { stages, frame, inbox, bags, models, kits, take, run: () => stages.run(frame) };
}

/**
 * A first frame that receives a projection and draws every root, then `frames` warm frames with nothing changed: the
 * first frame's work, the roots it submitted, and the warm frames' largest stage calls and total panel renders and root
 * rebuilds.
 */
export function warmWorldFrames(frames: number) {
  const view = world();
  view.models.add(new THREE.Group());
  view.inbox.push(projection(1));
  view.run();
  const first = view.take();
  const submitted = view.frame.submitted;
  let largestFrameStageCalls = 0;
  let panelRenders = 0;
  let rootRebuilds = 0;
  for (let frame = 0; frame < frames; frame += 1) {
    view.run();
    const work = view.take();
    largestFrameStageCalls = Math.max(largestFrameStageCalls, work.stageCalls);
    panelRenders += work.panelRenders;
    rootRebuilds += work.rootRebuilds;
  }
  return { first, submitted, largestFrameStageCalls, panelRenders, rootRebuilds };
}

/**
 * The hud stage's action bar over `frames` warm frames showing one projection (#1091): the first hand-over's result,
 * how many warm frames decoded again, and the cache (its `decodes` counts every decode).
 */
export function warmActionBar(frames: number) {
  const bar = new ActionBarCache();
  const latest = projection(1);
  const first = bar.changed(latest);
  let redecoded = 0;
  for (let frame = 0; frame < frames; frame += 1) {
    if (bar.changed(latest) !== null) { redecoded += 1; }
  }
  return { bar, latest, first, redecoded };
}
