import { test } from "node:test";
import assert from "node:assert/strict";
import { FixedTickClock, MAX_CATCH_UP_TICKS, frameDeltaSeconds } from "../src/demo-clock";
import { TICK_HZ } from "../src/replication";

test("fixed ticks come from accumulated time, not from frame count", () => {
  const clock = new FixedTickClock();
  let ticks = 0;
  // 1,440 frames at 144 Hz are ten seconds: ten seconds of ticks, give or take float rounding.
  for (let frame = 0; frame < 1_440; frame += 1) {
    ticks += clock.advance(1 / 144);
  }
  assert.ok(ticks === 10 * TICK_HZ || ticks === 10 * TICK_HZ - 1, `${ticks}`);
  assert.ok(clock.fraction >= 0 && clock.fraction < 1);
  assert.equal(clock.advance(0), 0);
});

test("the fraction interpolates within a tick and resets with the stream", () => {
  const clock = new FixedTickClock();
  assert.equal(clock.advance(0.5 / TICK_HZ), 0);
  assert.ok(Math.abs(clock.fraction - 0.5) < 1e-9);
  assert.equal(clock.advance(0.75 / TICK_HZ), 1);
  assert.ok(Math.abs(clock.fraction - 0.25) < 1e-9);
  clock.reset();
  assert.equal(clock.fraction, 0);
});

test("a long stall runs a bounded number of catch-up ticks", () => {
  const clock = new FixedTickClock();
  assert.equal(clock.advance(10), MAX_CATCH_UP_TICKS);
  assert.ok(clock.fraction < 1);
  assert.equal(clock.advance(0), 0, "the dropped backlog is not replayed later");
  for (const invalid of [NaN, Infinity, -1]) {
    assert.throws(() => clock.advance(invalid));
  }
});

test("frame delta clamps transition timestamp skew and long stalls", () => {
  assert.equal(frameDeltaSeconds(1000, 1001), 0);
  assert.equal(frameDeltaSeconds(1016, 1000), 0.016);
  assert.equal(frameDeltaSeconds(1200, 1000), 0.05);
  for (const invalid of [NaN, Infinity, -Infinity]) {
    assert.throws(() => frameDeltaSeconds(invalid, 0));
    assert.throws(() => frameDeltaSeconds(0, invalid));
  }
});
