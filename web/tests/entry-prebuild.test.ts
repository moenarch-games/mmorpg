import { expect, test } from "bun:test";
import { MAX_COMPOSE_ROUNDS, MAX_PREPARES_PER_ENTRY } from "../src/world/entry-prebuild";
import { STATIC_PARTS, harness, prebuildThenEnter } from "./support/work/entry-prebuild";

// The `entry-prebuild` budget (#855, `.performance/contract.json`): the selection screen builds the static scene once,
// one part per idle slice, composes and prepares the selected character's entry view, and a world frame after that
// builds no static scene. Switching characters composes and prepares again without rebuilding the static scene. The
// measurement lives in `support/work/entry-prebuild.ts`, which `scripts/work-evidence.ts` emits as work evidence (#1120).

test("the contract's scale points: static parts stay constant and prepares follow the entries selected", async () => {
  for (const entries of [1, 2, 3]) {
    expect(await prebuildThenEnter(entries)).toEqual({ staticParts: STATIC_PARTS, prepares: entries, frameStaticBuilds: 0 });
  }
});

test("the prebuild builds the static scene in slices, then composes and prepares the entry view", async () => {
  const { prebuild, idle, frame, counts } = harness();
  prebuild.request({ at: 1 }, "1", true);
  const slices = await idle();
  expect(counts.parts).toBe(STATIC_PARTS);
  expect(counts.composes).toBe(2);
  expect(counts.prepares).toBe(1);
  // One step per idle slice: the static parts, the compose rounds, the prepare.
  expect(slices).toBe(STATIC_PARTS + 2 + 1);
  expect(prebuild.settled).toBe(true);
  expect(prebuild.prepared).toBe(true);
  expect(prebuild.work).toEqual({ staticParts: STATIC_PARTS, composeRounds: 2, prepares: 1, slices: STATIC_PARTS + 3 });
  // Enter World: the world frame builds no static scene once the prebuild completed.
  frame();
  expect(counts.frameStaticBuilds).toBe(0);
});

test("the view is prepared only once entry art allows it, and asking again redoes nothing", async () => {
  const { prebuild, idle, counts } = harness();
  prebuild.request({ at: 1 }, "1", false);
  await idle();
  expect(counts.prepares).toBe(0);
  expect(prebuild.settled).toBe(true);
  prebuild.request({ at: 1 }, "1", true);
  await idle();
  expect(counts.prepares).toBe(1);
  for (let index = 0; index < 8; index += 1) { prebuild.request({ at: 1 }, "1", true); }
  expect(await idle()).toBe(0);
  expect(counts).toMatchObject({ parts: STATIC_PARTS, composes: 2, prepares: 1 });
});

test("switching characters composes and prepares again without rebuilding the static scene", async () => {
  const { prebuild, idle, counts } = harness();
  prebuild.request({ at: 1 }, "1", true);
  await idle();
  prebuild.request({ at: 2 }, "2", true);
  await idle();
  expect(counts.parts).toBe(STATIC_PARTS);
  expect(counts.composes).toBe(4);
  expect(counts.prepares).toBe(2);
  expect(counts.preparedAt).toEqual([1, 2]);
  // Back to the first character: its view is composed and prepared again, still without a static build.
  prebuild.request({ at: 1 }, "1", true);
  await idle();
  expect(counts.parts).toBe(STATIC_PARTS);
  expect(counts.prepares).toBe(3);
});

test("a compose settles within its round limit, and new roots re-prepare a bounded number of times", async () => {
  const { prebuild, idle, counts, addRoot } = harness(1_000);
  prebuild.request({ at: 1 }, "1", true);
  await idle();
  expect(counts.composes).toBe(MAX_COMPOSE_ROUNDS);
  expect(counts.prepares).toBe(1);
  for (let index = 0; index < 10; index += 1) {
    addRoot();
    prebuild.request({ at: 1 }, "1", true);
    await idle();
  }
  expect(counts.prepares).toBe(MAX_PREPARES_PER_ENTRY);
});

test("entering the world pauses the prebuild; a static scene left unfinished is built by the first frame", async () => {
  const { prebuild, idle, frame, counts, slices } = harness();
  prebuild.request(null, null, false);
  slices.shift()!();
  prebuild.pause();
  await idle();
  expect(counts.parts).toBe(1);
  frame();
  expect(counts.frameStaticBuilds).toBe(1);
});
