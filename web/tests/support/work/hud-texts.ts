import { CombatHud } from "../../../src/world/units/combat-hud";
import { hudTextReads, hudTexts } from "../../../src/world/units/hud-texts";
import { playerEntity, testSnapshot } from "../snapshots";

// The `hud-texts` measurement (#1278): `web/tests/hud-texts.test.ts` asserts its bounds and `web/scripts/work-evidence.ts`
// emits it as work evidence. The combat HUD's status and feedback lines and the class HUD's target name come from one
// `mmorpg-wasm` call per projection, however many frames show it (Atlas 7.1.i1256).

/** Projections each measurement shows. */
export const PROJECTIONS = 8;

/**
 * `PROJECTIONS` projections, each shown for `frames` frames to the combat HUD and the class HUD's target name: the most
 * HUD text reads into WASM any one projection made.
 */
export function hudTextReadsPerProjection(frames: number): number {
  const hud = new CombatHud({ textContent: "" }, { textContent: "" });
  let largest = 0;
  for (let tick = 1; tick <= PROJECTIONS; tick += 1) {
    const projection = testSnapshot({ zoneId: 1, contentRevision: 7n, viewerId: 1, tick: BigInt(tick), acknowledgedSequence: 0,
      entities: [playerEntity(1, [tick * 10, 90, 0])], events: [{ kind: "miss", source: { kind: "player", id: 1 }, target: { kind: "player", id: 2 } }] });
    const before = hudTextReads();
    for (let frame = 0; frame < frames; frame += 1) {
      hud.update(projection, tick * 1_000 + frame);
      hudTexts(projection);
    }
    largest = Math.max(largest, hudTextReads() - before);
  }
  return largest;
}
