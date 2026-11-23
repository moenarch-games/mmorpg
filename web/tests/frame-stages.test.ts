import { expect, test } from "bun:test";
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
} from "../src/world/frame-stages";
import type { ZoneSnapshot } from "../src/replication";
import { BagState } from "../src/world/units/bag-state";
import { playerEntity, testSnapshot } from "./support/snapshots";

// The `world-frame` budget (#1060, `.performance/contract.json`): a warm world frame with nothing changed runs each
// stage once, redraws no panel and rebuilds no root list. The frame below wires the same pieces `WorldView.frame()`
// does: one presenter per stage, received projections fanned out to the panels, the latest one gated by projection and
// bag revision, and the render roots kept in the registry.

function projection(tick: number): ZoneSnapshot {
  return testSnapshot({ zoneId: 1, contentRevision: 4n, viewerId: 1, tick: BigInt(tick), acknowledgedSequence: 0,
    entities: [playerEntity(1, [0, 90, 0])], inventoryRevision: 1n, inventory: Array<null>(16).fill(null) });
}

/** A root owner that streams in: its root set changes only when it loads or drops a root. */
class Owner implements RootPresenter {
  readonly #roots: THREE.Object3D[] = [];
  rootsRevision = 0;
  add(root: THREE.Object3D): void { this.#roots.push(root); this.rootsRevision += 1; }
  collectRoots(into: THREE.Object3D[]): void { for (const root of this.#roots) { into.push(root); } }
}

type Frame = { stopped: boolean; received: ZoneSnapshot[]; latest: ZoneSnapshot | null; submitted: number };

function world() {
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

test("the stages run once each, in the declared order", () => {
  const order: string[] = [];
  const work = emptyFrameWork();
  const stages = new FrameStages<{ stopped: boolean }>(WORLD_FRAME_STAGES.map((name) => ({ stage: name, update: () => { order.push(name); } })), work);
  stages.run({ stopped: false });
  expect(order).toEqual([...WORLD_FRAME_STAGES]);
  expect(work.stageCalls).toBe(WORLD_FRAME_STAGES.length);
  expect(() => new FrameStages([{ stage: "units", update: () => {} }, { stage: "input", update: () => {} }], work)).toThrow();
});

test("a stopped frame runs no later stage", () => {
  const work = emptyFrameWork();
  const ran: string[] = [];
  const stages = new FrameStages<{ stopped: boolean }>(WORLD_FRAME_STAGES.map((name) => ({
    stage: name, update: (frame) => { ran.push(name); if (name === "render") { frame.stopped = true; } },
  })), work);
  stages.run({ stopped: false });
  expect(ran.at(-1)).toBe("render");
  expect(work.stageCalls).toBe(WORLD_FRAME_STAGES.indexOf("render") + 1);
});

for (const frames of [16, 64, 256]) {
  test(`${frames} warm frames with nothing changed redraw no panel and rebuild no root list`, () => {
    // The contract's `largest_frame_stage_calls` budget (8): a new stage is a contract change.
    expect(WORLD_FRAME_STAGES.length).toBe(8);
    const view = world();
    view.models.add(new THREE.Group());
    view.inbox.push(projection(1));
    view.run();
    const first = view.take();
    expect(first.rootRebuilds).toBe(1);
    expect(first.panelRenders).toBe(9 + 3);
    expect(view.frame.submitted).toBe(2);

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
    expect({ largestFrameStageCalls, panelRenders, rootRebuilds }).toEqual({ largestFrameStageCalls: 8, panelRenders: 0, rootRebuilds: 0 });
  });
}

test("a joined root, a new projection or a bag change each cause exactly their own work", () => {
  const view = world();
  view.inbox.push(projection(1));
  view.run();
  view.take();

  view.kits.add(new THREE.Group());
  view.run();
  expect(view.take()).toEqual({ stageCalls: WORLD_FRAME_STAGES.length, panelRenders: 0, rootRebuilds: 1 });
  expect(view.frame.submitted).toBe(2);

  view.inbox.push(projection(2));
  view.run();
  expect(view.take()).toEqual({ stageCalls: WORLD_FRAME_STAGES.length, panelRenders: 9 + 3, rootRebuilds: 0 });

  // A sent intent the zone has not answered yet changes what the character pane shows, without a new projection.
  view.bags.reset(projection(3));
  view.run();
  expect(view.take()).toEqual({ stageCalls: WORLD_FRAME_STAGES.length, panelRenders: 3, rootRebuilds: 0 });
});

test("the registry keeps drawing order and roles, and a dirty mark rebuilds once", () => {
  const work = emptyFrameWork();
  const registry = new RootRegistry(work);
  const a = new Owner();
  const b = new Owner();
  const fixed = { root: new THREE.Group() };
  let shown = true;
  registry.register([rootEntry("world", () => a), fixedRootEntry("ground", () => fixed), rootEntry("inhabitant", () => b, () => shown)]);
  a.add(new THREE.Group());
  b.add(new THREE.Group());
  b.add(new THREE.Group());
  expect(registry.refresh()).toBe(true);
  expect(registry.roots.length).toBe(4);
  expect([0, 1, 2, 3].map((index) => registry.role(index))).toEqual(["world", "ground", "inhabitant", "inhabitant"]);
  expect(registry.roots[1]).toBe(fixed.root);
  expect(registry.refresh()).toBe(false);
  shown = false;
  registry.markDirty();
  expect(registry.refresh()).toBe(true);
  expect(registry.roots.length).toBe(2);
  expect(registry.refresh()).toBe(false);
  expect(work.rootRebuilds).toBe(2);
});
