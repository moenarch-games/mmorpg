import { expect, test } from "bun:test";
import * as THREE from "three";
import { FrameStages, RootRegistry, WORLD_FRAME_STAGES, emptyFrameWork, fixedRootEntry, rootEntry } from "../src/world/frame-stages";
import { Owner, projection, warmActionBar, warmWorldFrames, world } from "./support/work/world-frame";

// The `world-frame` budget (#1060, `.performance/contract.json`): a warm world frame with nothing changed runs each
// stage once, redraws no panel and rebuilds no root list. `support/work/world-frame.ts` wires the same pieces
// `WorldView.frame()` does and measures the frames; `scripts/work-evidence.ts` emits the same measurement (#1120).

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
    const { first, submitted, largestFrameStageCalls, panelRenders, rootRebuilds } = warmWorldFrames(frames);
    expect(first.rootRebuilds).toBe(1);
    expect(first.panelRenders).toBe(9 + 3);
    expect(submitted).toBe(2);
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

// The `world-frame` budget's `action_bar_decodes` (#1091): the hud stage hands a projection to the module's action bar
// once; frames that show the same projection again decode nothing.
for (const frames of [16, 64, 256]) {
  test(`${frames} warm hud frames over one projection decode the action bar once`, () => {
    const { bar, latest, first, redecoded } = warmActionBar(frames);
    expect(first).not.toBeNull();
    expect(redecoded).toBe(0);
    expect(bar.decodes).toBe(1);
    expect(bar.changed(projection(2))).toEqual(bar.changed(projection(3))!);
    expect(bar.decodes).toBe(3);
    bar.reset();
    expect(bar.changed(latest)).not.toBeNull();
    expect(bar.decodes).toBe(4);
  });
}
