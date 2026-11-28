import { DepthsLootPanel, type DepthsLootElements } from "../../../src/world/units/depths-loot-panel";
import { panelViewReads, panelViews } from "../../../src/world/units/panel-views";
import { WaystonePanel, type WaystoneElements } from "../../../src/world/units/waystone-panel";
import { playerEntity, testSnapshot } from "../snapshots";

// The `panel-views` measurement (#1280): `web/tests/panel-views.test.ts` asserts its bounds and `web/scripts/work-evidence.ts`
// emits it as work evidence. The minimap's dot classes and raid disturbance, the unit frames' auras, the waystone panel
// and the depths loot rows come from one `mmorpg-wasm` call per projection, however many frames and panels show it
// (Atlas 7.1.i1256).

/** Projections each measurement shows. */
export const PROJECTIONS = 8;

const node = () => ({ hidden: true, textContent: "", replaceChildren() {}, addEventListener() {}, append() {} });

/**
 * `PROJECTIONS` projections, each shown for `frames` frames to the waystone and loot panels, the class HUD's two unit
 * frames and the minimap: the most panel view reads into WASM any one projection made.
 */
export function panelViewReadsPerProjection(frames: number): number {
  const waystone = new WaystonePanel({ panel: node(), title: node(), destinations: node(), status: node() } as unknown as WaystoneElements, () => {});
  const loot = new DepthsLootPanel({ panel: node(), list: node() } as unknown as DepthsLootElements, () => {});
  let largest = 0;
  for (let tick = 1; tick <= PROJECTIONS; tick += 1) {
    const projection = testSnapshot({ zoneId: 1, contentRevision: 7n, viewerId: 1, tick: BigInt(tick), acknowledgedSequence: 0,
      entities: [playerEntity(1, [tick * 10, 90, 0]), playerEntity(2, [tick * 10 + 300, 90, 0])] });
    const before = panelViewReads();
    for (let frame = 0; frame < frames; frame += 1) {
      waystone.update(projection);
      loot.update(projection);
      panelViews(projection).playerAuras;
      panelViews(projection).targetAuras;
      panelViews(projection).minimapDots;
    }
    largest = Math.max(largest, panelViewReads() - before);
  }
  return largest;
}
