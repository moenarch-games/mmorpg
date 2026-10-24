import type { CharacterClassId, CharacterPreview, CharacterSex } from "./character-selection";

export type CharacterWeaponStyle = "sword" | "dagger" | "bow" | "staff";

export type CharacterVisualProfile = {
  bodyRadius: number;
  bodyHeight: number;
  chestSize: readonly [number, number, number];
  shoulderSpan: number;
  armRadius: number;
  headRadius: number;
  /** Hip width relative to shoulder width; the in-world humanoid reads its build from it. */
  hipRatio: number;
  bodyColor: `#${string}`;
  chestColor: `#${string}`;
  shoulderColor: `#${string}`;
  cloakColor: `#${string}`;
  weapon: CharacterWeaponStyle;
  weaponColor: `#${string}`;
};

const CLASS_VISUALS: Record<CharacterClassId, Omit<CharacterVisualProfile,
  "bodyRadius" | "bodyHeight" | "chestSize" | "shoulderSpan" | "armRadius" | "headRadius" | "hipRatio">> = {
  warden: {
    bodyColor: "#718d84",
    chestColor: "#58746b",
    shoulderColor: "#6e8f84",
    cloakColor: "#425c56",
    weapon: "sword",
    weaponColor: "#d3d6cf",
  },
  ranger: {
    bodyColor: "#748465",
    chestColor: "#617653",
    shoulderColor: "#788868",
    cloakColor: "#465a42",
    weapon: "bow",
    weaponColor: "#a88355",
  },
  arcanist: {
    bodyColor: "#777895",
    chestColor: "#62657e",
    shoulderColor: "#8580a0",
    cloakColor: "#4d4d69",
    weapon: "staff",
    weaponColor: "#bd895f",
  },
  // Violet-black robes and a bone-white staff (#301), the in-world look's colours.
  necromancer: {
    bodyColor: "#3d3548",
    chestColor: "#2c2536",
    shoulderColor: "#4b3e5e",
    cloakColor: "#1f1a28",
    weapon: "staff",
    weaponColor: "#e6dfc8",
  },
  // Charcoal and dark leather with a steel dagger (#302), the in-world look's colours.
  rogue: {
    bodyColor: "#45413d",
    chestColor: "#3a2e26",
    shoulderColor: "#4f4038",
    cloakColor: "#2a2826",
    weapon: "dagger",
    weaponColor: "#c4c9cc",
  },
  // White robes with gold trim and a white staff (#303), the in-world look's colours.
  cleric: {
    bodyColor: "#e8e4da",
    chestColor: "#f2efe6",
    shoulderColor: "#d9b44a",
    cloakColor: "#c9a23e",
    weapon: "staff",
    weaponColor: "#f4f1e8",
  },
};

const SEX_FRAME: Record<CharacterSex, Pick<CharacterVisualProfile,
  "bodyRadius" | "bodyHeight" | "chestSize" | "shoulderSpan" | "armRadius" | "headRadius" | "hipRatio">> = {
  male: {
    bodyRadius: 0.48,
    bodyHeight: 1.7,
    chestSize: [0.86, 0.82, 0.5],
    shoulderSpan: 0.52,
    armRadius: 0.16,
    headRadius: 0.34,
    hipRatio: 0.72,
  },
  female: {
    bodyRadius: 0.44,
    bodyHeight: 1.66,
    chestSize: [0.79, 0.8, 0.48],
    shoulderSpan: 0.47,
    armRadius: 0.15,
    headRadius: 0.33,
    hipRatio: 0.94,
  },
};

export function characterVisualProfile(
  character: Pick<CharacterPreview, "classId" | "sex">,
): CharacterVisualProfile {
  const classVisuals = CLASS_VISUALS[character.classId];
  const frame = SEX_FRAME[character.sex];
  return {
    ...frame,
    ...classVisuals,
    chestSize: [frame.chestSize[0], frame.chestSize[1], frame.chestSize[2]],
  };
}
