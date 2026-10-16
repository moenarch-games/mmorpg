import type { RendererEnvironment } from "@moritzbrantner/three-d-renderer";
import type { Color } from "./scenery";

/** Palette and renderer environment for the cosmetic twenty-minute day. */
export type EnvironmentStyle = {
  palette: typeof PALETTE;
  /** Terrain colours by `mmorpg-scenery` biome name; unknown biomes keep their exported base colour. */
  biomeColors: Readonly<Record<string, Color>>;
  sky: SkyStops;
  /** Legacy palette metadata; the renderer now applies aerial perspective through `fog`. */
  haze: { color: Color; bands: readonly { from: number; mix: number }[] };
  water: { color: Color; opacity: number; bedColor: Color; shoreColor: Color };
  /** Camera-distance fog and directional light defaults at noon. */
  fog: { color: Color; nearMetres: number; farMetres: number };
  sun: { direction: readonly [number, number, number]; color: Color };
};

export type SkyStops = { zenith: Color; upper: Color; horizon: Color; ground: Color };

const PALETTE = {
  plaster: "#e3d6b8",
  timber: "#5c3f28",
  roofThatch: "#c19a4f",
  roofRed: "#a3472f",
  roofSlate: "#55606f",
  roofWood: "#6d4a2d",
  stone: "#aaa396",
  stoneDark: "#7c766c",
  stoneLight: "#c7c0b0",
  wood: "#8d6641",
  woodDark: "#5f4430",
  woodLight: "#b08a5b",
  iron: "#55585e",
  barnRed: "#9b3b2c",
  trim: "#efe6d2",
  window: "#2e3a4a",
  windowLit: "#f2c46a",
  door: "#4a3222",
  canvas: "#d9cbad",
  canvasDark: "#a8472f",
  banner: "#2f5f8f",
  waystone: "#8e98a6",
  rune: "#7fe6ff",
  lampGlow: "#ffd98a",
  fire: "#ff8a2a",
  fireCore: "#ffe27a",
  ember: "#3b2a22",
  trunk: "#6a4a30",
  birchBark: "#e7e2d6",
  oakLeaves: "#4e7d32",
  oakLeavesLight: "#6f9c3c",
  pineNeedles: "#2e5a36",
  pineNeedlesLight: "#3e6e3f",
  birchLeaves: "#8fb54e",
  bush: "#4b7a34",
  bushLight: "#5f8f3a",
  grass: "#6f9f44",
  grassDark: "#5b8a39",
  reeds: "#8e9a52",
  cattail: "#6b4a2e",
  crop: "#d8bd5a",
  cropStem: "#93a74b",
  soil: "#6e4d32",
  rock: "#9a948a",
  rockDark: "#77726a",
  moss: "#62804a",
  flowers: ["#f2d64b", "#b98ae0", "#f4f1ea", "#e5675c"] as const,
} as const;

export const ENVIRONMENT: EnvironmentStyle = {
  palette: PALETTE,
  biomeColors: {
    meadow: "#6a9a42",
    hub: "#739a48",
    woods: "#58803c",
    hollow: "#94805f",
    farmland: "#7a5836",
    road: "#a4865c",
    plaza: "#a8987a",
    shore: "#cdbb88",
    "lake bed": "#5e5a44",
    foothills: "#5d8a44",
    highland: "#6f7d52",
    rock: "#8b877e",
    snow: "#eef1f5",
  },
  sky: { zenith: "#3f78c4", upper: "#78a9dd", horizon: "#cfe0ea", ground: "#b8c9c8" },
  haze: {
    color: "#b9cddb",
    bands: [
      { from: 0, mix: 0 },
      { from: 230, mix: 0.18 },
      { from: 300, mix: 0.32 },
      { from: 380, mix: 0.46 },
      { from: 470, mix: 0.6 },
    ],
  },
  water: { color: "#3f86a0", opacity: 0.72, bedColor: "#4f5a44", shoreColor: "#cdbb88" },
  fog: { color: "#b9cddb", nearMetres: 140, farMetres: 620 },
  sun: { direction: [10, 18, 8], color: "#fff4df" },
};

function channels(color: Color): [number, number, number] {
  const value = Number.parseInt(color.slice(1), 16);
  return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff];
}

function toHex([red, green, blue]: readonly number[]): Color {
  const byte = (channel: number | undefined) => Math.round(Math.min(255, Math.max(0, channel ?? 0))).toString(16).padStart(2, "0");
  return `#${byte(red)}${byte(green)}${byte(blue)}`;
}

/** Linear blend of two `#rrggbb` colours in sRGB, `t` clamped to [0, 1]. */
export function mixColor(from: Color, to: Color, t: number): Color {
  const amount = Math.min(1, Math.max(0, Number.isFinite(t) ? t : 0));
  const a = channels(from);
  const b = channels(to);
  return toHex(a.map((channel, index) => channel + (b[index]! - channel) * amount));
}

/** Rounds each channel to a multiple of `step`, merging near-identical tones into one batch. */
export function quantizeColor(color: Color, step: number): Color {
  return toHex(channels(color).map((channel) => Math.round(channel / step) * step));
}

/**
 * Merges near-identical colours into more common ones. Colours are visited
 * from most to least used (ties by value); each maps onto the nearest colour
 * already kept within `distance` (Euclidean over sRGB channels) or is kept
 * itself. The most common colours stay exact and no colour moves farther
 * than `distance`, so unlike coarser quantisation no hue drifts.
 */
export function mergeNearColors(colors: readonly Color[], distance: number): Map<Color, Color> {
  const counts = new Map<Color, number>();
  for (const color of colors) {
    counts.set(color, (counts.get(color) ?? 0) + 1);
  }
  const ordered = [...counts.entries()].sort(([left, leftCount], [right, rightCount]) =>
    rightCount - leftCount || left.localeCompare(right));
  const kept: { color: Color; rgb: [number, number, number] }[] = [];
  const merged = new Map<Color, Color>();
  for (const [color] of ordered) {
    const rgb = channels(color);
    let nearest: { color: Color; distance: number } | null = null;
    for (const candidate of kept) {
      const gap = Math.hypot(rgb[0] - candidate.rgb[0], rgb[1] - candidate.rgb[1], rgb[2] - candidate.rgb[2]);
      if (gap <= distance && (nearest === null || gap < nearest.distance)) {
        nearest = { color: candidate.color, distance: gap };
      }
    }
    if (nearest === null) {
      kept.push({ color, rgb });
    }
    merged.set(color, nearest?.color ?? color);
  }
  return merged;
}

/** Scales a colour's channels, e.g. 0.85 to darken; the result stays a valid colour. */
export function shadeColor(color: Color, factor: number): Color {
  return toHex(channels(color).map((channel) => channel * factor));
}

/** The haze mix of the band a distance falls in. */
export function hazeMix(style: EnvironmentStyle, distanceMetres: number): number {
  let mix = 0;
  for (const band of style.haze.bands) {
    if (distanceMetres >= band.from) {
      mix = band.mix;
    }
  }
  return mix;
}

/** A 20-minute day in real seconds. */
export const DAY_SECONDS = 20 * 60;

/** A periodic presentation clock; zero is noon, half a cycle is midnight. */
function cycle(dayFraction: number): { daylight: number; dusk: number; angle: number } {
  const fraction = Number.isFinite(dayFraction) ? dayFraction - Math.floor(dayFraction) : 0;
  const angle = fraction * 2 * Math.PI;
  const elevation = Math.cos(angle);
  const amount = Math.min(1, Math.max(0, (elevation + 0.15) / 0.8));
  const daylight = amount * amount * (3 - 2 * amount);
  return { daylight, dusk: (1 - elevation * elevation) ** 3, angle };
}

/** Sky, distance fog and lights share the same smooth cosmetic cycle. */
export function skyAt(style: EnvironmentStyle, dayFraction: number): SkyStops {
  const { daylight, dusk } = cycle(dayFraction);
  const blend = (day: Color, night: Color, twilight: Color) => mixColor(mixColor(night, day, daylight), twilight, dusk * 0.55);
  return {
    zenith: blend(style.sky.zenith, "#10223f", "#554f78"),
    upper: blend(style.sky.upper, "#1a3553", "#986f89"),
    horizon: blend(style.sky.horizon, "#38475e", "#e4a176"),
    ground: blend(style.sky.ground, "#26394a", "#8e7774"),
  };
}

export function environmentAt(style: EnvironmentStyle, dayFraction: number): RendererEnvironment {
  const { daylight, dusk, angle } = cycle(dayFraction);
  const sky = skyAt(style, dayFraction);
  return {
    background: sky.horizon,
    sky: {
      skyColor: mixColor("#8fa4c9", style.sky.upper, daylight),
      groundColor: mixColor("#56657b", style.sky.ground, daylight),
      intensity: 0.9 + daylight * 0.8,
    },
    sun: {
      direction: [Math.sin(angle) * 18 + 10 * Math.cos(angle), 6 + 12 * Math.cos(angle) ** 2, 8],
      color: mixColor(mixColor("#9cb8eb", style.sun.color, daylight), "#ffc08a", dusk * 0.7),
      intensity: 0.35 + daylight * 1.85,
    },
    fog: { color: mixColor(sky.horizon, style.fog.color, daylight), near: style.fog.nearMetres, far: style.fog.farMetres },
  };
}
