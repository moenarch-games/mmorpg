import { expect, test } from "bun:test";
import { panelViews } from "../src/world/units/panel-views";
import { nextTabTarget } from "../src/world/units/targeting";
import { vendorSales } from "../src/world/units/vendor-state";
import { panelViewReadsPerProjection } from "./support/work/panel-views";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

// The `panel-views` operation of `.performance/contract.json` (#1280): one WASM read per projection, however many frames
// and panels show it. `support/work/panel-views.ts` measures it; `scripts/work-evidence.ts` emits the same measurement.
for (const frames of [1, 4, 16]) {
  test(`${frames} frames of a projection read its panel views once`, () => {
    expect(panelViewReadsPerProjection(frames)).toBe(1);
  });
}

test("a sale is the whole stack, worded by the shared item rules (#1280)", () => {
  const [sale] = vendorSales([null, { itemId: 1, quantity: 3 }]);
  expect(sale).toEqual({ slot: 1, quantity: 3, value: 3, text: "Torn Fur × 3 · 3 copper", label: "Sell Torn Fur × 3 for 3 copper" });
});

test("depths loot rows count down rounding up and name the viewer's answer (#1280)", () => {
  const view = (tick: bigint) => testSnapshot({
    zoneId: 1, contentRevision: 7n, viewerId: 1, tick, acknowledgedSequence: 0, entities: [playerEntity(1, [0, 90, 0])],
    viewer: HEALTHY_VIEWER,
    depthsLoot: { runId: 9n, drops: [
      { drop: 0, itemId: 1, itemName: "Torn Fur", state: { kind: "rolling", deadline: 200n, answer: null, mayNeed: false } },
      { drop: 1, itemId: 1, itemName: "Torn Fur", state: { kind: "rolling", deadline: 200n, answer: "greed", mayNeed: true } },
    ] },
  });
  const loot = panelViews(view(139n)).depthsLoot!;
  expect(loot.runId).toBe(9n);
  expect(loot.rows.map((row) => row.line)).toEqual(["2 s", "You chose Greed. Waiting for the others…"]);
  expect(loot.rows[0]!.buttons.map((button) => [button.action, button.enabled])).toEqual([["need", false], ["greed", true], ["pass", true]]);
});

test("tab skips the dead and the unattackable and breaks distance ties by kind", () => {
  const unit = (kind: "player" | "creature", id: number, x: number, flags: { attackable: boolean; dead?: boolean }) =>
    ({ ...playerEntity(id, [x, 90, 0]), kind, flags: { ...playerEntity(id, [x, 90, 0]).flags, attackable: flags.attackable, dead: flags.dead ?? false } });
  const projection = testSnapshot({
    zoneId: 1, contentRevision: 7n, viewerId: 1, tick: 1n, acknowledgedSequence: 0,
    entities: [playerEntity(1, [0, 90, 0]), unit("creature", 5, 300, { attackable: true }), unit("player", 4, 300, { attackable: true }),
      unit("creature", 3, 50, { attackable: true, dead: true })],
  });
  expect(nextTabTarget(projection)).toEqual({ kind: "player", id: 4 });
});
