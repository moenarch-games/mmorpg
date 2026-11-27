import { ProjectionDecoder, type ZoneSnapshot } from "../../../src/replication";
import { wasm } from "../../../src/wasm";
import { combatTexts, viewerInterrupted } from "../../../src/world/units/combat-text";
import { shrineStatus } from "../../../src/world/units/quest-state";
import { playerEntity, testSnapshot } from "../snapshots";
import { encodeSnapshot } from "../wire";

// The `projection-rules` measurement (#1257): `web/tests/projection-rules.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. Each tick the page hands its projection view to a dozen
// client rules (action bar, chat log, combat text, quest views, ...); they read the projection the session's store
// decoded instead of deserialising the page object again, which cost thousands of calls into the page per rule.

/** The rules each tick's view goes through here, as the HUD hands it to them. */
export const RULES: readonly ((view: ZoneSnapshot) => unknown)[] = [viewerInterrupted, combatTexts, shrineStatus];

function projection(tick: number): Uint8Array {
  return encodeSnapshot(testSnapshot({ zoneId: 1, contentRevision: 7n, viewerId: 1, tick: BigInt(tick), acknowledgedSequence: 0,
    entities: [playerEntity(1, [tick * 10, 90, 0])] }));
}

/** Deserialisations since the page loaded (`projectionViewDeserialisations`). */
function deserialised(): number {
  return wasm.projectionViewDeserialisations();
}

/**
 * `ticks` projections ingested by one session, each view read by every rule: how many projections the rules
 * deserialised from the page objects, and the rule reads made.
 */
export function ingestedRuleReads(ticks: number): { deserialisations: number; reads: number } {
  const decoder = new ProjectionDecoder();
  const before = deserialised();
  let reads = 0;
  for (let tick = 1; tick <= ticks; tick += 1) {
    const view = decoder.decode(projection(tick));
    for (const rule of RULES) { rule(view); reads += 1; }
  }
  return { deserialisations: deserialised() - before, reads };
}

/** The same rules over a copy no store handed out: each read deserialises it. */
export function copiedRuleReads(): number {
  const view = new ProjectionDecoder().decode(projection(1));
  const before = deserialised();
  for (const rule of RULES) { rule(structuredClone(view)); }
  return deserialised() - before;
}
