/**
 * Work evidence for the browser operations of `.performance/contract.json` (#1120): `world-frame`, `scenery-frame`,
 * `ambient-clips` and `sun-shadow-casters`. Each is measured by the same helper in `tests/support/work/` that its bun test
 * asserts through, at every scale point the contract declares, and written as `{ "operations": [...] }` to the path
 * given as the first argument; `scripts/operation-work.py` merges it with the Rust examples' samples. Counters only;
 * nothing is timed.
 *
 * A new browser operation adds a helper under `tests/support/work/`, calls it from its test and adds its id to
 * `MEASUREMENTS` below.
 *
 *   bun scripts/work-evidence.ts <out.json>
 */
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// The test preload builds and instantiates `mmorpg-wasm` (bunfig.toml); the measurements import its generated bindings,
// so they load only after it has run.
await import("../tests/support/wasm");
const { shadowLodReady } = await import("../src/world/shadow-lod");
const { largestClipsSampledPerFrame } = await import("../tests/support/work/ambient-clips");
const { greyhavenScenery, warmSceneryFrames } = await import("../tests/support/work/scenery-frame");
const { largestFrameCascadeRedraws, unitShadowTrianglePercent } = await import("../tests/support/work/sun-shadow-casters");
const { warmActionBar, warmWorldFrames } = await import("../tests/support/work/world-frame");

type Point = Record<string, number>;
type Measure = (point: Point) => Record<string, number>;

/** The point's value of `dimension`, which the contract must declare. */
function at(point: Point, dimension: string): number {
  const value = point[dimension];
  if (value === undefined) { throw new Error(`scale point ${JSON.stringify(point)} has no ${dimension}`); }
  return value;
}

await shadowLodReady;
const { scene } = await greyhavenScenery();
const [nearShadow, middleShadow, farShadow] = unitShadowTrianglePercent();

const MEASUREMENTS: Record<string, Measure> = {
  "world-frame": (point) => {
    const frames = at(point, "frames");
    const { largestFrameStageCalls, panelRenders, rootRebuilds } = warmWorldFrames(frames);
    return {
      largest_frame_stage_calls: largestFrameStageCalls,
      panel_renders: panelRenders,
      root_rebuilds: rootRebuilds,
      action_bar_decodes: warmActionBar(frames).bar.decodes,
    };
  },
  "scenery-frame": (point) => {
    const { built, reused } = warmSceneryFrames(scene, at(point, "frames"));
    if (!reused) { throw new Error("scenery-frame: a warm frame handed out a batch the first frame did not"); }
    return { instance_batch_views_built: built };
  },
  "ambient-clips": (point) => ({
    largest_clips_sampled_per_frame: largestClipsSampledPerFrame(at(point, "placed"), at(point, "visible_near"), at(point, "visible_middle")),
  }),
  "sun-shadow-casters": (point) => ({
    largest_frame_cascade_redraws: largestFrameCascadeRedraws(at(point, "frames")),
    unit_near_shadow_triangle_percent: nearShadow!,
    // The contract bounds the middle and far cascades together.
    unit_far_shadow_triangle_percent: Math.max(middleShadow!, farShadow!),
  }),
};

const out = process.argv[2];
if (!out) { throw new Error("usage: bun scripts/work-evidence.ts <out.json>"); }
const contract = JSON.parse(readFileSync(join(import.meta.dir, "..", "..", ".performance", "contract.json"), "utf8")) as {
  operations: { id: string; scalePoints: Point[] }[];
};
const operations = Object.entries(MEASUREMENTS).map(([id, measure]) => {
  const declared = contract.operations.find((operation) => operation.id === id);
  if (!declared) { throw new Error(`.performance/contract.json no longer declares ${id}`); }
  return { id, samples: declared.scalePoints.map((point) => ({ dimensions: point, metrics: measure(point) })) };
});
writeFileSync(out, `${JSON.stringify({ operations })}\n`);
