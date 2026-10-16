import type { NpcRole } from "../catalog";
import type { Color } from "../scenery";
import { addShield, addSword, type HumanoidSpec } from "./humanoid-body";
import { ball, box, child, post, tilt } from "./model-kit";

/**
 * NPC looks by role: compact humanoids in the 0.6 m × 1.8 m character box.
 * Colour and accessories tell the roles apart (guard green tabard and helmet,
 * vendor brown apron, quest giver blue cloak, spirit healer white robe); none
 * of them marks quests, which the HUD owns.
 */
const SKIN: Color = "#d7a582";
const GUARD_GREEN: Color = "#3d8a4c";

export const NPC_SPECS: Record<NpcRole, () => HumanoidSpec> = {
  guard: () => ({
    skin: SKIN, tunic: "#7d878c", trousers: "#4a5256", boots: "#2f2923", stance: "sword-and-shield", hunch: 0,
    accessories: (gear) => {
      const { head, spine, sx, sy, sz } = gear;
      gear.add("helmet", ball(0.14 * sy), "#a9b1b6", child(head, [0, 0.035 * sy, -0.005 * sz]));
      gear.add("helmet-nasal", box(0.025 * sx, 0.1 * sy, 0.02 * sz), "#a9b1b6", child(head, [0, -0.02 * sy, 0.135 * sz]));
      gear.add("tabard-front", box(0.3 * sx, 0.5 * sy, 0.03 * sz), GUARD_GREEN, child(spine, [0, 0.27 * sy, 0.125 * sz]));
      gear.add("tabard-back", box(0.3 * sx, 0.5 * sy, 0.03 * sz), GUARD_GREEN, child(spine, [0, 0.27 * sy, -0.125 * sz]));
      for (const [name, side] of [["left", 1], ["right", -1]] as const) {
        gear.add(`pauldron-${name}`, box(0.12 * sx, 0.06 * sy, 0.14 * sz), "#a9b1b6", child(spine, [side * 0.245 * sx, 0.56 * sy, 0]));
      }
      addSword(gear);
      addShield(gear, GUARD_GREEN);
    },
  }),
  vendor: () => ({
    skin: SKIN, tunic: "#d9c9a3", trousers: "#5a4632", boots: "#3a2a1e", stance: "staff", hunch: 0,
    accessories: (gear) => {
      const { head, spine, sx, sy, sz } = gear;
      gear.add("hair", ball(0.13 * sy), "#5a3b24", child(head, [0, 0.03 * sy, -0.025 * sz]));
      gear.add("apron", box(0.32 * sx, 0.42 * sy, 0.03 * sz), "#7a4f2c", child(spine, [0, 0.22 * sy, 0.125 * sz]));
      gear.add("satchel", box(0.06 * sx, 0.16 * sy, 0.2 * sz), "#6a4a30", child(spine, [-0.24 * sx, 0.1 * sy, 0]));
      gear.add("strap", box(0.04 * sx, 0.6 * sy, 0.28 * sz), "#6a4a30", child(spine, [0, 0.3 * sy, 0], tilt(0, -0.5)));
    },
  }),
  quest_giver: () => ({
    skin: SKIN, tunic: "#3f66b0", trousers: "#2f3f63", boots: "#2f2923", stance: "staff", hunch: 0,
    accessories: (gear) => {
      const { head, spine, sx, sy, sz } = gear;
      gear.add("hair", ball(0.13 * sy), "#8a8a8a", child(head, [0, 0.03 * sy, -0.025 * sz]));
      gear.add("cloak", box(0.44 * sx, 0.7 * sy, 0.03 * sz), "#2d4a86", child(spine, [0, 0.25 * sy, -0.14 * sz], tilt(gear.pose.cape)));
      gear.add("sash", box(0.44 * sx, 0.06 * sy, 0.27 * sz), "#d6b45a", child(spine, [0, 0.08 * sy, 0]));
      gear.add("scroll", post(0.03 * sx, 0.2 * sy), "#e8ddb8", child(gear.rightHand, [0, 0, 0.02 * sz], tilt(Math.PI / 2)));
    },
  }),
  spirit_healer: () => ({
    skin: "#e2cdb8", tunic: "#f1f1f4", trousers: "#f1f1f4", boots: "#d8d8de", stance: "staff", hunch: 0,
    accessories: (gear) => {
      const { head, pelvis, spine, sx, sy, sz } = gear;
      gear.add("robe", box(0.4 * sx, 0.8 * sy, 0.28 * sz), "#f7f7fa", child(pelvis, [0, -0.4 * sy, 0]));
      gear.add("hood", ball(0.145 * sy), "#e4e6ee", child(head, [0, 0.015 * sy, -0.035 * sz]));
      gear.add("sash", box(0.44 * sx, 0.06 * sy, 0.27 * sz), "#9fc4e8", child(spine, [0, 0.08 * sy, 0]));
      gear.add("orb", ball(0.05 * sy), "#bfe8ff", child(gear.rightHand, [0, -0.03 * sy, 0.01 * sz]));
    },
  }),
};
