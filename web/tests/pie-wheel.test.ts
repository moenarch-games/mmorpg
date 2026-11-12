import { describe, expect, test } from "bun:test";
import type { ZoneSnapshot } from "../src/replication";
import { clientPreferences } from "../src/settings/preferences";
import { MouseOwnerInput } from "../src/world/units/mouse-owner";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

function warden(): ZoneSnapshot {
  return {
    ...testSnapshot({ zoneId: 1, tick: 1n, contentRevision: 1n, acknowledgedSequence: 0, viewerId: 1, entities: [playerEntity(1, [0, 90, 0])] }),
    viewer: { ...HEALTHY_VIEWER, level: 10, classChoice: { classId: "warden", sex: "female" } },
  };
}

describe("radial wheels (#653)", () => {
  test("wheels are off by default and the fixed wheel's sectors come from Settings", () => {
    const wheels = clientPreferences({}).wheels;
    expect(wheels.enabled).toBe(false);
    expect(wheels.countUsage).toBe(false);
    expect(wheels.sectors).toHaveLength(8);
  });

  test("a flick picks the fixed sector it points at; the camera holds still", () => {
    const mouse = new MouseOwnerInput();
    const sectors = ["ability.slot1", "ability.slot2", "move.roll", "none", "emote.wave", "none", "target.next", "none"];
    expect(mouse.openWheel("fixed", sectors, warden())).toBe(true);
    expect(mouse.owner()).toBe("wheel");
    expect(mouse.motion(0, -40)).toBe(false);
    const view = mouse.wheel(warden());
    expect(view?.highlight).toBe(0);
    expect(view?.sectors[0]).toEqual({ id: "ability.slot1", label: "Heroic Strike" });
    expect(mouse.releaseWheel(80, false)).toEqual({ outcome: "pick", action: "ability.slot1" });
    expect(mouse.owner()).toBe("free-look");
  });

  test("tap to open, click to pick; right click cancels; no wheel over a held strike", () => {
    const mouse = new MouseOwnerInput();
    mouse.openWheel("fixed", ["emote.wave"], warden());
    expect(mouse.releaseWheel(60, false).outcome).toBe("open");
    mouse.motion(0, -40);
    expect(mouse.leftClick()).toEqual({ outcome: "confirm", ended: "wheel", ability: null, action: "emote.wave" });
    mouse.openWheel("context", [], warden());
    expect(mouse.rightClick()).toEqual({ outcome: "cancel", ended: "wheel", ability: null, action: null });
    expect(mouse.beginHold("strike").started).toBe(true);
    expect(mouse.openWheel("fixed", ["emote.wave"], warden())).toBe(false);
  });

  test("the opt-in counters count opens, picks, cancels and time per mouse owner", () => {
    const mouse = new MouseOwnerInput();
    mouse.setUsageCounting("");
    mouse.openWheel("fixed", ["emote.wave"], warden());
    mouse.rightClick();
    mouse.countUsage(500);
    const usage = JSON.parse(mouse.countUsage(0) ?? "{}");
    expect(usage.opens.fixed).toBe(1);
    expect(usage.cancels.fixed).toBe(1);
    expect(usage.owner_ms.free_look).toBe(500);
  });
});
