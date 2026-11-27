import { expect, test } from "bun:test";
import { RULES, copiedRuleReads, ingestedRuleReads } from "./support/work/projection-rules";

// The `projection-rules` budget (#1257, `.performance/contract.json`): the client rules read an ingested projection's
// decoded copy, never the page's view again. `support/work/projection-rules.ts` measures it; `scripts/work-evidence.ts`
// emits the same measurement.
for (const ticks of [16, 32, 64]) {
  test(`${ticks} ingested projections reach every rule without a deserialisation`, () => {
    expect(ingestedRuleReads(ticks)).toEqual({ deserialisations: 0, reads: ticks * RULES.length });
  });
}

test("a projection no store handed out is still read, by deserialising it", () => {
  expect(copiedRuleReads()).toBe(RULES.length);
});
