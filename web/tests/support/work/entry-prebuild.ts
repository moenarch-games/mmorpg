import { EntryPrebuild, type PrebuildTarget } from "../../../src/world/entry-prebuild";

// The `entry-prebuild` measurement (#855, #1120): `web/tests/entry-prebuild.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. The target below counts what `WorldView`'s prebuild target
// does: parts built, compose rounds and prepares, and the static builds a first world frame would run
// (`#inputStage` builds the remaining parts).

export const STATIC_PARTS = 4;

type Entry = { at: number };

export function harness(composeRoundsToSettle = 2) {
  const slices: (() => void)[] = [];
  const counts = { parts: 0, composes: 0, prepares: 0, frameStaticBuilds: 0, preparedAt: [] as number[] };
  let built = 0;
  let roots = 0;
  let streamed = 0;
  const target: PrebuildTarget<Entry> = {
    staticBuilt: () => built >= STATIC_PARTS,
    buildStaticPart: () => { built += 1; counts.parts += 1; },
    compose: () => { counts.composes += 1; streamed += 1; return streamed % composeRoundsToSettle === 0; },
    prepareKey: () => String(roots),
    prepare: (entry) => { counts.prepares += 1; counts.preparedAt.push(entry.at); return Promise.resolve(); },
  };
  const prebuild = new EntryPrebuild(target, (slice) => slices.push(slice));
  /** Runs idle slices (and the prepares' settling) until nothing is scheduled. */
  const idle = async (limit = 100): Promise<number> => {
    let ran = 0;
    for (;;) {
      await Promise.resolve();
      await Promise.resolve();
      const slice = slices.shift();
      if (!slice) {
        await new Promise((resolve) => setTimeout(resolve, 0));
        if (slices.length === 0) { return ran; }
        continue;
      }
      slice();
      ran += 1;
      if (ran > limit) { throw new Error("the prebuild did not settle"); }
    }
  };
  /** A world frame: it builds whatever static parts remain (`WorldView.#inputStage`). */
  const frame = () => {
    if (built < STATIC_PARTS) { counts.frameStaticBuilds += 1; built = STATIC_PARTS; }
  };
  return { prebuild, idle, frame, counts, slices, addRoot: () => { roots += 1; } };
}

/**
 * The selection screen prebuilds `entries` entry views in turn (the first, then one per character switch), then the
 * player enters the world: the static parts built, the entry views prepared and the static builds the first world frame ran.
 */
export async function prebuildThenEnter(entries: number) {
  const { prebuild, idle, frame, counts } = harness();
  for (let entry = 1; entry <= entries; entry += 1) {
    prebuild.request({ at: entry }, String(entry), true);
    await idle();
  }
  frame();
  return { staticParts: counts.parts, prepares: counts.prepares, frameStaticBuilds: counts.frameStaticBuilds };
}
