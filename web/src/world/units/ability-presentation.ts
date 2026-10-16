import type { AuraKind } from "../../replication";
import type { AbilityRecord, ContentCatalog } from "../catalog";

/**
 * Presentation facts the exported catalog does not carry: range, a one-line description, an icon
 * and the spell visual. They describe abilities for a reader and never decide an outcome; the
 * zone validates range, resource and cooldowns. `local-zone-wasm.test.ts` holds this table to the
 * exported catalog so a catalog change cannot leave an ability undescribed.
 */
export type AbilityRange = "self" | "melee" | { units: number };

export type SpellVisual = "projectile" | "ring" | "bubble" | "shards" | "arrow" | "swing";

/** The body motion the caster plays: arms raised, bow drawn or a weapon swung. */
export type CasterPose = "cast" | "draw" | "swing";

export type AbilityPresentation = {
  range: AbilityRange;
  description: string;
  /** SVG path data in a 24 × 24 box, filled with the slot's accent colour. */
  glyph: string;
  accent: `#${string}`;
  visual: SpellVisual | null;
  pose: CasterPose | null;
};

/** Melee reach in core units (`PLAYER_REACH_UNITS`). */
export const MELEE_REACH_UNITS = 250;
const RANGED: AbilityRange = { units: 3_000 };

const SWORD = "M12 2 14 4 14 14 17 14 17 16 14 16 14 19 16 21 16 22 8 22 8 21 10 19 10 16 7 16 7 14 10 14 10 4Z";
const SHIELD = "M12 2 20 5 20 12C20 17 16 20 12 22 8 20 4 17 4 12L4 5Z";
const HEART = "M12 21 4 13C1 10 3 4 8 4 10 4 11 5 12 7 13 5 14 4 16 4 21 4 23 10 20 13Z";
const CLEAVE = "M3 3 7 3 21 17 21 21 17 21 3 7ZM15 3 21 3 21 9Z";
const ARROW = "M3 19 15 7 13 5 21 3 19 11 17 9 5 21Z";
const SNAKE = "M6 4C12 2 18 5 16 9 14 13 8 11 8 15 8 18 14 17 18 20L19 22C12 22 5 20 6 15 7 10 14 12 13 9 12 7 8 8 6 7Z";
const FOOT = "M5 12C5 8 8 6 12 6L19 6 19 18 12 18C8 18 5 16 5 12ZM19 8 22 8 22 16 19 16Z";
const BOLT = "M13 2 5 13 11 13 9 22 19 9 13 9Z";
const FLAME = "M12 2C14 7 19 9 19 15 19 19 16 22 12 22 8 22 5 19 5 15 5 12 7 10 8 8 9 10 10 11 11 10 12 8 11 5Z";
const FLAKE = "M11 2 13 2 13 10 19 6 20 8 14 12 20 16 19 18 13 14 13 22 11 22 11 14 5 18 4 16 10 12 4 8 5 6 11 10Z";
const ORB = "M12 2C18 2 22 6 22 12 22 18 18 22 12 22 6 22 2 18 2 12 2 6 6 2 12 2ZM12 6C9 6 6 9 6 12 6 15 9 18 12 18 15 18 18 15 18 12 18 9 15 6 12 6Z";
const SHARDS = "M4 2 8 10 3 12ZM11 2 15 12 9 14ZM18 2 22 10 17 14ZM7 15 11 22 4 20ZM15 16 19 22 12 22Z";
const BANDAGE = "M3 9 15 3 21 15 9 21ZM10 10 14 12 12 16 8 14Z";
const SPLAT = "M12 3C15 3 17 6 20 7 22 10 19 13 19 16 17 19 14 18 12 21 9 19 6 20 5 16 3 13 4 10 6 8 8 5 10 4Z";

/** Player abilities 1–12 and creature abilities 13–14, by catalog ID. */
export const ABILITY_PRESENTATION: ReadonlyMap<number, AbilityPresentation> = new Map<number, AbilityPresentation>([
  [1, { range: "melee", description: "A heavy weapon blow at your target.", glyph: SWORD, accent: "#e0a15c", visual: "swing", pose: "swing" }],
  [2, { range: "melee", description: "Slams your shield into the target: interrupts its cast and stuns it.", glyph: SHIELD, accent: "#9fb4c8", visual: null, pose: "swing" }],
  [3, { range: "self", description: "Rallies your spirit and heals you over time.", glyph: HEART, accent: "#7fd08a", visual: null, pose: null }],
  [4, { range: "melee", description: "A sweeping blow that also hits creatures near your target.", glyph: CLEAVE, accent: "#d9a066", visual: "swing", pose: "swing" }],
  [5, { range: RANGED, description: "A carefully placed shot at your target.", glyph: ARROW, accent: "#c9b36a", visual: "arrow", pose: "draw" }],
  [6, { range: RANGED, description: "A poisoned shot that damages the target over time.", glyph: SNAKE, accent: "#7fc05a", visual: "arrow", pose: "draw" }],
  [7, { range: RANGED, description: "A stunning shot that slows the target's movement.", glyph: FOOT, accent: "#b0a58c", visual: "arrow", pose: "draw" }],
  [8, { range: "self", description: "Quickens your attacks for a short time.", glyph: BOLT, accent: "#f2c14e", visual: null, pose: null }],
  [9, { range: RANGED, description: "Hurls a bolt of fire at your target after a short cast.", glyph: FLAME, accent: "#ff8a3d", visual: "projectile", pose: "cast" }],
  [10, { range: "self", description: "A burst of frost around you that damages and roots nearby creatures.", glyph: FLAKE, accent: "#8fd8ff", visual: "ring", pose: "cast" }],
  [11, { range: "self", description: "A shield of arcane force that absorbs damage.", glyph: ORB, accent: "#b69cff", visual: "bubble", pose: "cast" }],
  [12, { range: RANGED, description: "Channel a storm of ice shards onto the area around your target.", glyph: SHARDS, accent: "#a8e4ff", visual: "shards", pose: "cast" }],
  [13, { range: { units: 2_000 }, description: "A bolt of muck flung at its target.", glyph: SPLAT, accent: "#7d9a4a", visual: "projectile", pose: null }],
  [14, { range: "self", description: "Binds wounds when badly hurt.", glyph: BANDAGE, accent: "#e7dcc0", visual: null, pose: null }],
]);

export function presentationOf(ability: number): AbilityPresentation | undefined {
  return ABILITY_PRESENTATION.get(ability);
}

/** The maximum XZ centre distance in units, or null for a self-centred ability. */
export function reachUnits(range: AbilityRange): number | null {
  if (range === "self") {
    return null;
  }
  return range === "melee" ? MELEE_REACH_UNITS : range.units;
}

export type AuraPolarity = "buff" | "debuff";

const BUFF_KINDS: ReadonlySet<AuraKind> = new Set(["heal-over-time", "absorb", "haste"]);

/** Heals, shields and haste help their bearer; damage over time, roots, snares and stuns hurt it. */
export function auraPolarity(kind: AuraKind): AuraPolarity {
  return BUFF_KINDS.has(kind) ? "buff" : "debuff";
}

/** The icon facts for an ability ID, with a neutral fallback for IDs this build does not describe. */
export function glyphOf(ability: number): { glyph: string; accent: `#${string}` } {
  const entry = ABILITY_PRESENTATION.get(ability);
  return entry ?? { glyph: ORB, accent: "#9aa4ad" };
}

/** The catalog abilities this table does not describe (empty when complete). */
export function undescribedAbilities(catalog: ContentCatalog): readonly AbilityRecord[] {
  return [...catalog.abilities.values()].filter((ability) => !ABILITY_PRESENTATION.has(ability.id));
}
