import type { AuraKind } from "../../replication";
import type { AbilityRecord, ContentCatalog } from "../catalog";

/**
 * Presentation facts the exported catalog does not carry: range, a one-line description, an icon
 * and the spell visual. They describe abilities for a reader and never decide an outcome; the
 * zone validates range, resource and cooldowns. `local-zone-wasm.test.ts` holds this table to the
 * exported catalog so a catalog change cannot leave an ability undescribed.
 */
export type AbilityRange = "self" | "melee" | { units: number };

export type SpellVisual = "projectile" | "ring" | "bubble" | "shards" | "swing";

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
const FLAME = "M12 2C14 7 19 9 19 15 19 19 16 22 12 22 8 22 5 19 5 15 5 12 7 10 8 8 9 10 10 11 11 10L12 8 11 5Z";
const FLAKE = "M11 2 13 2 13 10 19 6 20 8 14 12 20 16 19 18 13 14 13 22 11 22 11 14 5 18 4 16 10 12 4 8 5 6 11 10Z";
const ORB = "M12 2C18 2 22 6 22 12 22 18 18 22 12 22 6 22 2 18 2 12 2 6 6 2 12 2ZM12 6C9 6 6 9 6 12 6 15 9 18 12 18 15 18 18 15 18 12 18 9 15 6 12 6Z";
const SHARDS = "M4 2 8 10 3 12ZM11 2 15 12 9 14ZM18 2 22 10 17 14ZM7 15 11 22 4 20ZM15 16 19 22 12 22Z";
const BANDAGE = "M3 9 15 3 21 15 9 21ZM10 10 14 12 12 16 8 14Z";
const SPLAT = "M12 3L15 3 17 6 20 7 22 10 19 13 19 16 17 19 14 18 12 21 9 19 6 20 5 16 3 13 4 10 6 8 8 5 10 4Z";
const SKULL = "M12 2C17 2 20 5 20 10 20 13 18 15 17 16L17 19 15 19 15 21 13 21 13 19 11 19 11 21 9 21 9 19 7 19 7 16C6 15 4 13 4 10 4 5 7 2 12 2ZM9 9C8 9 7 10 7 11 7 12 8 13 9 13 10 13 11 12 11 11 11 10 10 9 9 9ZM15 9C14 9 13 10 13 11 13 12 14 13 15 13 16 13 17 12 17 11 17 10 16 9 15 9Z";

/** Player abilities 1–12 and 16–17, creature abilities 13–15 and 18, by catalog ID. Shots (5–7, 15) fly as the zone's own projectiles (#286). */
export const ABILITY_PRESENTATION: ReadonlyMap<number, AbilityPresentation> = new Map<number, AbilityPresentation>([
  [1, { range: "melee", description: "A heavy weapon blow at your target.", glyph: SWORD, accent: "#e0a15c", visual: "swing", pose: "swing" }],
  [2, { range: "melee", description: "Slams your shield into the target: interrupts its cast, stuns it and knocks it back.", glyph: SHIELD, accent: "#9fb4c8", visual: null, pose: "swing" }],
  [3, { range: "self", description: "Rallies your spirit and heals you over time.", glyph: HEART, accent: "#7fd08a", visual: null, pose: null }],
  [4, { range: "melee", description: "A sweeping blow that also hits creatures near your target.", glyph: CLEAVE, accent: "#d9a066", visual: "swing", pose: "swing" }],
  [5, { range: RANGED, description: "A carefully placed shot at your target.", glyph: ARROW, accent: "#c9b36a", visual: null, pose: "draw" }],
  [6, { range: RANGED, description: "A poisoned shot that damages the target over time.", glyph: SNAKE, accent: "#7fc05a", visual: null, pose: "draw" }],
  [7, { range: RANGED, description: "A stunning shot that slows the target's movement.", glyph: FOOT, accent: "#b0a58c", visual: null, pose: "draw" }],
  [8, { range: "self", description: "Quickens your attacks for a short time.", glyph: BOLT, accent: "#f2c14e", visual: null, pose: null }],
  [9, { range: RANGED, description: "Hurls a bolt of fire at your target after a short cast.", glyph: FLAME, accent: "#ff8a3d", visual: "projectile", pose: "cast" }],
  [10, { range: "self", description: "A burst of frost around you that damages and roots nearby creatures.", glyph: FLAKE, accent: "#8fd8ff", visual: "ring", pose: "cast" }],
  [11, { range: "self", description: "A shield of arcane force that absorbs damage.", glyph: ORB, accent: "#b69cff", visual: "bubble", pose: "cast" }],
  [12, { range: RANGED, description: "Channel a storm of ice shards onto the area around your target.", glyph: SHARDS, accent: "#a8e4ff", visual: "shards", pose: "cast" }],
  [13, { range: { units: 2_000 }, description: "A bolt of muck flung at its target.", glyph: SPLAT, accent: "#7d9a4a", visual: "projectile", pose: "cast" }],
  [14, { range: "self", description: "Binds wounds when badly hurt.", glyph: BANDAGE, accent: "#e7dcc0", visual: null, pose: "cast" }],
  [15, { range: { units: 2_000 }, description: "A crossbow bolt at its target; walls and raised shields stop it.", glyph: ARROW, accent: "#8a6a4a", visual: null, pose: "draw" }],
  [16, { range: RANGED, description: "An explosion at your target that burns and hurls back every creature nearby; walls shield those behind them.", glyph: FLAME, accent: "#ff5a2a", visual: "ring", pose: "cast" }],
  [17, { range: { units: 1_500 }, description: "Rush at your target and knock it back on arrival; walls stop the charge.", glyph: FOOT, accent: "#c98a4a", visual: null, pose: "swing" }],
  [18, { range: "self", description: "A ground-shaking stomp that hurts and hurls back every player nearby; walls shield those behind them.", glyph: FOOT, accent: "#8a7a5a", visual: "ring", pose: "swing" }],
  [19, { range: RANGED, description: "Looses an arrow along your facing that pierces up to three enemies in a line, each hit weaker. Walls and raised shields stop it.", glyph: ARROW, accent: "#d8c27a", visual: null, pose: "draw" }],
  [21, { range: "self", description: "Breathes frost in a cone ahead of you: damages and slows every enemy within 8 m you can see.", glyph: FLAKE, accent: "#9fdcff", visual: null, pose: "cast" }],
  [22, { range: "self", description: "Channel a lightning beam along your facing for 2 s: it strikes every enemy on its line until a wall or raised shield. Turn to steer it.", glyph: BOLT, accent: "#c8dcff", visual: null, pose: "cast" }],
  [23, { range: RANGED, description: "Lightning strikes your target, then jumps twice to the nearest enemies within 8 m it can see.", glyph: BOLT, accent: "#b9a8ff", visual: null, pose: "cast" }],
  [20, { range: RANGED, description: "Looses an arrow along your facing that bounces off walls up to twice before striking the first enemy it meets.", glyph: ARROW, accent: "#a8c87a", visual: null, pose: "draw" }],
  [24, { range: "self", description: "Garrick winds up for 1.5 s, then whirls his blade around him: step out of the 4 m ring or be hurt and thrown back.", glyph: CLEAVE, accent: "#d8452c", visual: "ring", pose: "swing" }],
  [25, { range: "melee", description: "A Redbrand Brute raises its weapon for 2 s without turning, then smashes the 3.5 m arc ahead of it: step aside or behind it, or be hurt and knocked back.", glyph: SWORD, accent: "#a8452c", visual: "swing", pose: "swing" }],
  [26, { range: RANGED, description: "Hurls a bolt of shadow at your target after a short cast.", glyph: SKULL, accent: "#8a4dff", visual: "projectile", pose: "cast" }],
  [27, { range: RANGED, description: "A shadow missile that curses your target, damaging it over time.", glyph: SNAKE, accent: "#7dff8a", visual: "projectile", pose: "cast" }],
  [28, { range: RANGED, description: "Channel for 3 s to drain your target's life: each pulse damages it and heals you for the health it lost. Breaks if the target leaves sight or range.", glyph: HEART, accent: "#6fe07a", visual: null, pose: "cast" }],
  [29, { range: "self", description: "A ward of bone that absorbs damage.", glyph: SHIELD, accent: "#e6dfc8", visual: "bubble", pose: "cast" }],
  [30, { range: "melee", description: "A quick dagger strike at your target.", glyph: SWORD, accent: "#c4c9cc", visual: "swing", pose: "swing" }],
  [31, { range: "melee", description: "A poisoned cut that damages your target over time.", glyph: SNAKE, accent: "#8fd14f", visual: "swing", pose: "swing" }],
  [32, { range: "melee", description: "Gouges your target: interrupts its cast and stuns it without knocking it back.", glyph: SWORD, accent: "#d9b26a", visual: null, pose: "swing" }],
  [33, { range: "self", description: "Quickens your attacks for a short time.", glyph: BOLT, accent: "#f2d24e", visual: null, pose: null }],
  // The Cleric (#303): Renew and Divine Shield land on a selected friendly player, otherwise on you.
  [34, { range: RANGED, description: "Hurls a bolt of holy light at your target after a short cast. Costs no faith: dealing damage and healing build it.", glyph: ORB, accent: "#ffd65a", visual: "projectile", pose: "cast" }],
  [35, { range: "self", description: "Heals over time: your selected friendly player, or yourself without one.", glyph: HEART, accent: "#d8f0a0", visual: null, pose: "cast" }],
  [36, { range: "self", description: "Channel a beam of holy light along your facing for 2 s: it strikes every enemy on its line until a wall or raised shield. Turn to steer it.", glyph: BOLT, accent: "#ffe9a0", visual: null, pose: "cast" }],
  [37, { range: "self", description: "A holy ward that absorbs damage: on your selected friendly player, or yourself without one.", glyph: SHIELD, accent: "#fff3c8", visual: "bubble", pose: "cast" }],
  // The Druid (#304): Rejuvenation lands on a selected friendly player, otherwise on you.
  [38, { range: RANGED, description: "Hurls a bolt of nature magic at your target after a short cast.", glyph: ORB, accent: "#7fd65a", visual: "projectile", pose: "cast" }],
  [39, { range: RANGED, description: "Roots your target in place with grasping vines; damage breaks them after a moment.", glyph: SNAKE, accent: "#6b8a3a", visual: null, pose: "cast" }],
  [40, { range: "self", description: "Heals over time: your selected friendly player, or yourself without one.", glyph: HEART, accent: "#9ee07a", visual: null, pose: "cast" }],
  [41, { range: "self", description: "Your skin hardens into bark that absorbs damage.", glyph: SHIELD, accent: "#a07a4a", visual: "bubble", pose: "cast" }],
  // The Rogue's thrown weapons (#290): the zone's own knives, flasks and bombs fly.
  [42, { range: { units: 2_500 }, description: "Throws a knife at your target, or along your facing without one; walls and raised shields stop it.", glyph: SWORD, accent: "#c4c9cc", visual: null, pose: "swing" }],
  [43, { range: RANGED, description: "Lobs a venom flask along your facing that shatters on what it meets, poisoning every enemy within 3 m it can see.", glyph: SPLAT, accent: "#8fe05a", visual: null, pose: "swing" }],
  [44, { range: RANGED, description: "Lobs a bomb along your facing that bounces and rolls, then bursts when an enemy touches it or after 2.5 s, hurting and hurling back every enemy within 4 m it can see.", glyph: FLAME, accent: "#ffa23a", visual: null, pose: "swing" }],
  // The Necromancer's summoned minions (#295): Y attacks the target, U defends here, I follows, O retreats.
  [45, { range: "self", description: "Raises a skeleton that follows you and fights for a minute; up to four stand at once. Y sends them at your target, U holds this spot, I follows, O retreats.", glyph: SKULL, accent: "#e6dfc8", visual: null, pose: "cast" }],
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

export type AbilityArtwork = { regular: string; highDpi: string };

/** Exact owner-approved Warden derivatives, bundled by Vite; no runtime service. */
const WARDEN_ARTWORK: ReadonlyMap<number, AbilityArtwork> = new Map([
  [1, { regular: new URL("../../../assets/warden-icons/warden-1-64.png", import.meta.url).href, highDpi: new URL("../../../assets/warden-icons/warden-1-512.png", import.meta.url).href }],
  [2, { regular: new URL("../../../assets/warden-icons/warden-2-64.png", import.meta.url).href, highDpi: new URL("../../../assets/warden-icons/warden-2-512.png", import.meta.url).href }],
  [3, { regular: new URL("../../../assets/warden-icons/warden-3-64.png", import.meta.url).href, highDpi: new URL("../../../assets/warden-icons/warden-3-512.png", import.meta.url).href }],
  [4, { regular: new URL("../../../assets/warden-icons/warden-4-64.png", import.meta.url).href, highDpi: new URL("../../../assets/warden-icons/warden-4-512.png", import.meta.url).href }],
 ]);

export function artworkOf(ability: number): AbilityArtwork | null {
  return WARDEN_ARTWORK.get(ability) ?? null;
}

/** The catalog abilities this table does not describe (empty when complete). */
export function undescribedAbilities(catalog: ContentCatalog): readonly AbilityRecord[] {
  return [...catalog.abilities.values()].filter((ability) => !ABILITY_PRESENTATION.has(ability.id));
}
