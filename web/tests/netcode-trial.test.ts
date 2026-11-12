import { describe, expect, test } from "bun:test";
import { wasm } from "../src/wasm";
import { TICK_HZ, type ZoneSnapshot } from "../src/replication";
import {
  linkTicks, NetcodeTrialSource, netcodeTrialFromQuery, type NetcodeOption, type TrialZoneHandle,
} from "../src/world/battle-royale/netcode-trial";

const TICK = 1 / TICK_HZ;
const WARDEN_STANDARD = new Uint16Array(6);

function trial(option: NetcodeOption, roundTripMs: number): NetcodeTrialSource {
  return new NetcodeTrialSource(
    () => wasm.LocalZone.forArenaTrial(0, 1, WARDEN_STANDARD, option === "rewind") as unknown as TrialZoneHandle,
    () => new wasm.ArenaMovementPrediction(),
    { option, roundTripMs },
  );
}

/** The sampled own body's Z, the way the view draws it. */
function ownZ(source: NetcodeTrialSource, viewer: number): number {
  return source.sample().find((entity) => entity.kind === "player" && entity.entityId === viewer)!.position[2];
}

describe("arena netcode trial (#680)", () => {
  test("options and simulated delays come from the URL and split into whole ticks", () => {
    expect(netcodeTrialFromQuery("?netcode=rewind&delay=150")).toEqual({ option: "rewind", roundTripMs: 150 });
    expect(netcodeTrialFromQuery("?netcode=predicted")).toEqual({ option: "predicted", roundTripMs: 0 });
    expect(netcodeTrialFromQuery("?netcode=rollback&delay=50")).toBeNull();
    expect(linkTicks(50)).toEqual({ up: 1, down: 2 });
    expect(linkTicks(100)).toEqual({ up: 3, down: 3 });
    expect(linkTicks(150)).toEqual({ up: 4, down: 5 });
  });

  test("the baseline answers a command a round trip later; prediction moves at once and agrees", async () => {
    const results: Record<string, { acknowledgedAt: number; firstMove: number }> = {};
    for (const option of ["baseline", "predicted"] as const) {
      const source = trial(option, 100);
      const viewer = await source.join();
      for (let tick = 0; tick < 10; tick += 1) { source.advance(TICK); }
      const start = ownZ(source, viewer);
      const sequence = source.sendCommand({ kind: "move", forward: 1, strafe: 0, facing: 0 })!;
      let acknowledgedAt = -1;
      let firstMove = -1;
      for (let tick = 1; tick <= 60; tick += 1) {
        const received: readonly ZoneSnapshot[] = source.advance(TICK);
        if (acknowledgedAt < 0 && received.some((snapshot) => snapshot.acknowledgedSequence >= sequence)) { acknowledgedAt = tick; }
        if (firstMove < 0 && ownZ(source, viewer) > start + 1) { firstMove = tick; }
      }
      results[option] = { acknowledgedAt, firstMove };
      source.sendCommand({ kind: "move", forward: 0, strafe: 0, facing: 0 });
      for (let tick = 0; tick < 30; tick += 1) { source.advance(TICK); }
      // At rest the prediction and the zone's own answer agree.
      const latest = source.latestProjection()!;
      const zoneZ = latest.entities.find((entity) => entity.kind === "player" && entity.entityId === viewer)!.position[2];
      expect(Math.abs(ownZ(source, viewer) - zoneZ)).toBeLessThanOrEqual(1);
      source.leave();
    }
    // 100 ms is 3 ticks up and 3 back, after the command waits for the next tick: the zone
    // acknowledges on the round trip either way.
    expect(results.baseline!.acknowledgedAt).toBe(7);
    expect(results.predicted!.acknowledgedAt).toBe(7);
    // Only the prediction moves the own body before the zone's answer arrives.
    expect(results.predicted!.firstMove).toBeLessThan(3);
    expect(results.baseline!.firstMove).toBeGreaterThan(7);
  });

  test("a match with rewind admits a sparring partner that closes in", async () => {
    const source = trial("rewind", 150);
    const viewer = await source.join();
    const partnerDistance = () => {
      const [own, partner] = [viewer, viewer + 1].map((id) =>
        source.latestProjection()!.entities.find((entity) => entity.kind === "player" && entity.entityId === id)!);
      return Math.hypot(own!.position[0] - partner!.position[0], own!.position[2] - partner!.position[2]);
    };
    const before = partnerDistance();
    for (let tick = 0; tick < 120; tick += 1) { source.advance(TICK); }
    expect(partnerDistance()).toBeLessThan(Math.max(before - 50, 260));
    source.leave();
  });
});
