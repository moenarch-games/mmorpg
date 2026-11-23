import { describe, expect, test } from "bun:test";
import { loadoutAttachments } from "../src/world/wasm-runtime";

// The browser's former `LOADOUT_ITEMS`, which matched the native `Equipment::loadout` (#1012).
const BEFORE: Record<string, { key: string; socket: string }[]> = {
  "sword-and-shield": [
    { key: "equipment.sword.arming", socket: "hand_r.weapon" },
    { key: "equipment.shield.heater", socket: "hand_l.shield" },
  ],
  "sword-and-shield.redbrand": [
    { key: "equipment.sword.arming", socket: "hand_r.weapon" },
    { key: "equipment.shield.redbrand", socket: "hand_l.shield" },
  ],
  "bow-and-quiver": [
    { key: "equipment.bow.long", socket: "back.bow.stowed" },
    { key: "equipment.quiver.back", socket: "back.quiver" },
  ],
  sword: [{ key: "equipment.sword.arming", socket: "hand_r.weapon" }],
  dagger: [{ key: "equipment.dagger.blackened", socket: "hand_r.weapon" }],
  "dagger.twin": [
    { key: "equipment.dagger.blackened", socket: "hand_r.weapon" },
    { key: "equipment.dagger.blackened", socket: "hand_l.weapon" },
  ],
  "staff.bone": [{ key: "equipment.staff.bone", socket: "hand_r.staff" }],
  "staff.crystal-and-tome": [
    { key: "equipment.staff.crystal", socket: "hand_r.staff" },
    { key: "equipment.book.tome", socket: "hand_l.shield" },
  ],
  "mace.flanged": [{ key: "equipment.mace.flanged", socket: "hand_r.weapon" }],
  "staff.gnarled": [{ key: "equipment.staff.gnarled", socket: "hand_r.staff" }],
  spear: [{ key: "equipment.spear.leaf", socket: "hand_r.staff" }],
  "sword.garricks-cleaver": [{ key: "equipment.sword.garricks-cleaver", socket: "hand_r.weapon" }],
  "spear.hollowpine-hunter": [{ key: "equipment.spear.hollowpine-hunter", socket: "hand_r.staff" }],
  crossbow: [
    { key: "equipment.crossbow", socket: "hand_r.crossbow" },
    { key: "equipment.arrow", socket: "hand_r.arrow" },
  ],
  // The great weapons the heavy running set carries in the right hand (#1073, #1076).
  "warhammer.maul": [{ key: "equipment.warhammer.maul", socket: "hand_r.weapon" }],
  "hammer.great": [{ key: "equipment.hammer.great", socket: "hand_r.weapon" }],
  "axe.great": [{ key: "equipment.axe.great", socket: "hand_r.weapon" }],
  "sword.great": [{ key: "equipment.sword.great", socket: "hand_r.weapon" }],
};

describe("loadout attachments", () => {
  test("the shared table keeps every loadout's items on the sockets the browser drew them", () => {
    expect(loadoutAttachments()).toEqual(BEFORE);
  });
});
