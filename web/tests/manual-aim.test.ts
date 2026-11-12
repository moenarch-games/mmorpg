import { describe, expect, test } from "bun:test";
import type { ZoneSnapshot } from "../src/replication";
import { MouseOwnerInput } from "../src/world/units/mouse-owner";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

const BOMB_SLOT = 7;
const BLASTING_BOMB = 44;

function rogue(): ZoneSnapshot {
  return {
    ...testSnapshot({ zoneId: 1, tick: 1n, contentRevision: 1n, acknowledgedSequence: 0, viewerId: 1, entities: [playerEntity(1, [0, 90, 0])] }),
    viewer: { ...HEALTHY_VIEWER, level: 10, classChoice: { classId: "rogue", sex: "female" } },
  };
}

describe("manual bomb aim (#649)", () => {
  test("hold to aim from focus, release to throw once; right click throws nothing", () => {
    const mouse = new MouseOwnerInput();
    mouse.setFocus(true);
    expect(mouse.pressAim(BOMB_SLOT, BLASTING_BOMB, rogue(), false)).toBe("aim");
    expect(mouse.owner()).toBe("aim");
    expect(mouse.motion(20, 0)).toBe(true);
    const landing = mouse.aimLanding(rogue(), 0);
    expect(landing?.[2]).toBeGreaterThan(1_100);
    expect(mouse.leftClick()).toEqual({ outcome: "nothing", ended: "none", ability: null });
    expect(mouse.releaseAim(BOMB_SLOT, false)).toBe(BLASTING_BOMB);
    expect(mouse.owner()).toBe("focus");
    expect(mouse.releaseAim(BOMB_SLOT, false)).toBeUndefined();
    mouse.pressAim(BOMB_SLOT, BLASTING_BOMB, rogue(), false);
    expect(mouse.rightClick()).toEqual({ outcome: "cancel", ended: "aim", ability: null });
    expect(mouse.releaseAim(BOMB_SLOT, false)).toBeUndefined();
    mouse.pressAim(BOMB_SLOT, BLASTING_BOMB, rogue(), false);
    expect(mouse.releaseAim(BOMB_SLOT, true)).toBeUndefined();
    expect(mouse.owner()).toBe("focus");
    // Another ability, or an open panel, is used as usual.
    expect(mouse.pressAim(5, 42, rogue(), false)).toBe("use");
    expect(mouse.pressAim(BOMB_SLOT, BLASTING_BOMB, rogue(), true)).toBe("use");
  });

  test("the armed style keeps the aim past the release and throws on the left click", () => {
    const mouse = new MouseOwnerInput();
    mouse.setAimStyle("armed");
    expect(mouse.pressAim(BOMB_SLOT, BLASTING_BOMB, rogue(), false)).toBe("aim");
    expect(mouse.releaseAim(BOMB_SLOT, false)).toBeUndefined();
    expect(mouse.pressAim(BOMB_SLOT, BLASTING_BOMB, rogue(), false)).toBe("ignore");
    expect(mouse.leftClick()).toEqual({ outcome: "throw", ended: "aim", ability: BLASTING_BOMB });
    expect(mouse.owner()).toBe("free-look");
  });
});
