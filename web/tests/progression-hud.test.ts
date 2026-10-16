import { expect, test } from "bun:test";
import { ProgressionHud } from "../src/world/units/progression-hud";
import { FEEDBACK_MS } from "../src/world/units/combat-hud";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

function fixture() {
  const bar = { max: 1, value: 0 };
  const status = { textContent: "" };
  const feedback = { textContent: "" };
  return { bar, status, feedback, hud: new ProgressionHud(bar, status, feedback) };
}

function snapshot(tick: number, level = 1, experience = 0, threshold = 100) {
  return testSnapshot({ zoneId: 1, tick: BigInt(tick), contentRevision: 3n, acknowledgedSequence: 0, viewerId: 1,
    entities: [playerEntity(1, [0, 90, 0])],
    viewer: { ...HEALTHY_VIEWER, level, experience, experienceToNextLevel: threshold },
  });
}

test("displays exact received XP and threshold with no local curve", () => {
  const { hud, bar, status, feedback } = fixture();
  hud.update(snapshot(1, 1, 37, 100), 0);
  expect(bar).toEqual({ max: 100, value: 37 });
  expect(status.textContent).toBe("Level 1 · 37 / 100 XP");
  expect(feedback.textContent).toBe("");
});

test("skipped levels produce feedback from durable self state without events", () => {
  const { hud, feedback } = fixture();
  hud.update(snapshot(1), 0);
  hud.update(snapshot(20, 3, 12, 300), 100);
  expect(feedback.textContent).toBe("You reached level 3!");
  hud.update(snapshot(20, 3, 12, 300), 100 + FEEDBACK_MS);
  expect(feedback.textContent).toBe("");
});

test("stale snapshots cannot rewind XP or extend feedback", () => {
  const { hud, bar, status, feedback } = fixture();
  hud.update(snapshot(2), 0);
  hud.update(snapshot(4, 2, 10, 200), 10);
  hud.update(snapshot(3, 1, 90, 100), 20);
  expect(bar).toEqual({ max: 200, value: 10 });
  expect(status.textContent).toBe("Level 2 · 10 / 200 XP");
  hud.update(snapshot(4, 2, 10, 200), FEEDBACK_MS + 10);
  expect(feedback.textContent).toBe("");
});

test("the cap is a completed accessible bar and reset starts a fresh session", () => {
  const { hud, bar, status, feedback } = fixture();
  hud.update(snapshot(40, 10, 0, 0), 0);
  expect(bar).toEqual({ max: 1, value: 1 });
  expect(status.textContent).toBe("Level 10 · Maximum level");
  expect(feedback.textContent).toBe("");
  hud.reset();
  expect(bar).toEqual({ max: 1, value: 0 });
  expect(status.textContent).toBe("");
  hud.update(snapshot(1, 4, 25, 400), 100);
  expect(feedback.textContent).toBe("");
  expect(bar).toEqual({ max: 400, value: 25 });
});
