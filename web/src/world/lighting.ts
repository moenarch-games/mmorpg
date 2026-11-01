import * as THREE from "three";
import {
  FogFalloff,
  Lighting,
  LitMaterial,
  loadToneMappingLuts,
  type PointLight,
  type SunShadows,
  type Vec3,
} from "@moritzbrantner/three-lighting";
import type { RendererMaterialRequest } from "@moritzbrantner/three-d-renderer";

/**
 * Greyhaven's lighting on the web (#600): render-engine's `@moritzbrantner/three-lighting`, driven with the values
 * the native client gives render-engine (`crates/mmorpg-client/src/graphics/frame.rs`), so both clients light alike.
 * Every lit mesh draws with a `LitMaterial`; anything else (sprites, effects) is drawn as three draws it.
 */

/** Greyhaven's sun path (`DAY_CYCLE`): 60° above the southern horizon at noon, the moon 12 h behind. */
const NOON_ELEVATION = THREE.MathUtils.degToRad(60);
const MOON_LAG_HOURS = 12;
/** Bevy `lux::RAW_SUNLIGHT`: sunlight above the atmosphere. */
const RAW_SUNLIGHT = 130_000;
/** Moonlight, brighter than a real full moon so night stays playable (`MOON_LUX`, `MOON_COLOR`). */
const MOON_LUX = 2;
const MOON_COLOR: Vec3 = [0.75, 0.85, 1];
/** EV100 by sun elevation in degrees (`EXPOSURE`). */
const EXPOSURE: readonly (readonly [number, number])[] = [[-12, 2], [-4, 6], [2, 11], [15, 13.5], [60, 14.5]];
/** Emissive presentation colours glow at this luminance, cd/m² (`EMISSIVE_NITS`). */
export const EMISSIVE_NITS = 80;
/** Sun cascades sized for the gameplay camera (`SUN_SHADOWS`). */
const SUN_SHADOWS: SunShadows = {
  numCascades: 4, minDistance: 0.2, firstCascadeFarBound: 12, maxDistance: 160, overlapProportion: 0.2, depthBias: 0.02, normalBias: 1.8,
};
/** Below this sun elevation (degrees) the sun casts no shadows (`SHADOWLESS_BELOW`). */
const SHADOWLESS_BELOW = -3;
/** The presentation materials' surface (`standard_material`). */
const ROUGHNESS = 0.86;
const METALLIC = 0.02;

/**
 * TEMPORARY until render-engine's atmosphere sky reaches three.js (moritzbrantner/render-engine#39): the native
 * client's sun passes through Bevy's atmosphere and its ambient light is the sky's. Here the sun is filtered by an
 * analytic Earth atmosphere (Bevy's Rayleigh, Mie and ozone optical depths over the Kasten–Young air mass) and the
 * sky light is an ambient term by sun elevation, both tuned against the native frames at noon, dusk and night. The
 * CSS sky stays behind the transparent canvas.
 */
const ZENITH_OPTICAL_DEPTH: Vec3 = [0.0663, 0.1468, 0.2762];
/** Sky ambient light, cd/m², by sun elevation in degrees, and its colour. */
const SKY_AMBIENT: readonly (readonly [number, number])[] = [[-12, 0.35], [-4, 3], [0, 900], [3, 2000], [8, 3500], [15, 6000], [60, 11000]];
const SKY_COLOR: Vec3 = [0.8, 0.88, 1];
const TWILIGHT_COLOR: Vec3 = [0.86, 0.84, 0.88];

function interpolate(table: readonly (readonly [number, number])[], x: number): number {
  const first = table[0]!;
  const last = table[table.length - 1]!;
  if (x <= first[0]) { return first[1]; }
  for (let index = 1; index < table.length; index += 1) {
    const [x1, y1] = table[index]!;
    const [x0, y0] = table[index - 1]!;
    if (x <= x1) { return y0 + (y1 - y0) * (x - x0) / (x1 - x0); }
  }
  return last[1];
}

/** EV100 at a sun elevation in degrees, like the native client's `ev100`. */
export function ev100(elevation: number): number {
  return interpolate(EXPOSURE, elevation);
}

/** Unit direction towards the sun at `hours` (render-engine `DayCycle::to_sun`). */
export function toSun(hours: number): Vec3 {
  const angle = (hours - 6) / 12 * Math.PI;
  return [Math.cos(angle), Math.sin(NOON_ELEVATION) * Math.sin(angle), Math.cos(NOON_ELEVATION) * Math.sin(angle)];
}

/** Sunlight reaching the ground at an elevation in degrees, linear RGB lux (zero below the horizon). */
export function sunlight(elevation: number): Vec3 {
  if (elevation <= -0.5) { return [0, 0, 0]; }
  const e = Math.max(elevation, 0);
  const airMass = 1 / (Math.sin(THREE.MathUtils.degToRad(e)) + 0.50572 * (e + 6.07995) ** -1.6364);
  // Fades out over the last half degree as the disc sets.
  const setting = Math.min(1, (elevation + 0.5) / 0.5);
  return ZENITH_OPTICAL_DEPTH.map((depth) => RAW_SUNLIGHT * setting * Math.exp(-depth * airMass)) as unknown as Vec3;
}

/** Linear 0–1 channels of an sRGB `#rrggbb` colour or 0xrrggbb number. */
export function linearRgb(color: string | number): Vec3 {
  const value = new THREE.Color(color);
  return [value.r, value.g, value.b];
}

function linear(channel: number): number {
  return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
}

/** The `LitMaterial` for a presentation node or instance batch (the native `standard_material`). */
export function presentationMaterial({ node }: RendererMaterialRequest): THREE.Material {
  const opacity = node.opacity ?? 1;
  return new LitMaterial({
    baseColor: linearRgb(node.color ?? 0xffffff),
    alpha: opacity,
    alphaMode: opacity < 1 ? "blend" : "opaque",
    emissive: linearRgb(node.emissive ?? 0x000000).map((channel) => channel * EMISSIVE_NITS) as unknown as Vec3,
    perceptualRoughness: ROUGHNESS,
    metallic: METALLIC,
    doubleSided: node.doubleSided === true,
    unlit: node.unlit === true,
  });
}

const converted = new WeakMap<THREE.Material, LitMaterial>();

/** The `LitMaterial` for a glTF (or other three) material, one per source material. */
export function litMaterial(material: THREE.Material): THREE.Material {
  if ((material as Partial<LitMaterial>).isLitMaterial) { return material; }
  let lit = converted.get(material);
  if (!lit) {
    if (material instanceof THREE.MeshLambertMaterial || material instanceof THREE.MeshPhongMaterial) {
      lit = new LitMaterial({
        baseColor: [material.color.r, material.color.g, material.color.b], alpha: material.opacity,
        alphaMode: material.transparent ? "blend" : "opaque", perceptualRoughness: ROUGHNESS, metallic: METALLIC,
        doubleSided: material.side === THREE.DoubleSide,
      });
    } else {
      lit = LitMaterial.fromGltf(material);
    }
    lit.name = material.name;
    converted.set(material, lit);
  }
  return lit;
}

/** Gives every mesh under `root` the `LitMaterial` of its material (shared sources stay shared). */
export function litObject<T extends THREE.Object3D>(root: T): T {
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) { return; }
    object.material = Array.isArray(object.material) ? object.material.map(litMaterial) : litMaterial(object.material);
  });
  return root;
}

/** One frame's lighting inputs: the hour, the fog colour (`#rrggbb`, null for none) and the point lights. */
export type LightingFrame = { hour: number; fog: string | null; pointLights: Float32Array | null };

/** The page's `Lighting`: the day clock's sun or moon, sky light, fog and the scenery's point lights. */
export class GreyhavenLighting {
  readonly lighting: Lighting | null = null;
  /** Resolves once the tone-mapping LUTs loaded; nothing draws before. */
  readonly ready: Promise<void>;

  /** Loads the tone-mapping LUTs from `base`, where `vite.config.ts` copies them (`luts/`). */
  constructor(base: string) {
    this.ready = loadToneMappingLuts({
      tonyMcMapface: `${base}luts/tony_mc_mapface.ktx2`,
      agx: `${base}luts/AgX-default_contrast.ktx2`,
    }).then((luts) => {
      const lighting = new Lighting(luts);
      lighting.tonemapping = "tonyMcMapface";
      lighting.shadowFilter = "gaussian";
      // The CSS sky shows through where nothing is drawn (TEMPORARY, render-engine#39).
      lighting.clearAlpha = 0;
      (this as { lighting: Lighting | null }).lighting = lighting;
      if (this.#frame) { this.apply(this.#frame); }
    });
  }

  #frame: LightingFrame | null = null;

  /** Sets the lights for one frame, as the native client's `frame::scene` does. */
  apply(frame: LightingFrame): void {
    this.#frame = frame;
    const lighting = this.lighting;
    if (!lighting) { return; }
    const { hour, fog, pointLights } = frame;
    const sun = toSun(hour);
    const elevation = THREE.MathUtils.radToDeg(Math.asin(Math.min(1, Math.max(-1, sun[1]))));
    lighting.ev100 = ev100(elevation);
    const light = sunlight(elevation);
    const lux = Math.max(...light);
    // One directional light: the sun while it outshines the moon, then the moon (unshadowed, like the native moon).
    if (lux > MOON_LUX) {
      lighting.sun = {
        direction: [-sun[0], -sun[1], -sun[2]],
        color: light.map((channel) => channel / lux) as unknown as Vec3,
        illuminance: lux,
        shadows: elevation > SHADOWLESS_BELOW ? SUN_SHADOWS : null,
      };
    } else {
      const moon = toSun(hour - MOON_LAG_HOURS);
      lighting.sun = moon[1] > 0
        ? { direction: [-moon[0], -moon[1], -moon[2]], color: MOON_COLOR, illuminance: MOON_LUX, shadows: null }
        : null;
    }
    const twilight = Math.max(0, 1 - Math.abs(elevation) / 8);
    lighting.ambient = {
      color: SKY_COLOR.map((channel, index) => channel + (TWILIGHT_COLOR[index]! - channel) * twilight) as unknown as Vec3,
      brightness: interpolate(SKY_AMBIENT, elevation),
    };
    lighting.fog = fog === null ? null : {
      color: [...linearRgb(fog), 1],
      falloff: FogFalloff.linear(140, 620),
    };
    const lights: PointLight[] = [];
    if (pointLights) {
      for (let index = 0; index + 8 <= pointLights.length; index += 8) {
        lights.push({
          position: [pointLights[index]!, pointLights[index + 1]!, pointLights[index + 2]!],
          color: [linear(pointLights[index + 3]!), linear(pointLights[index + 4]!), linear(pointLights[index + 5]!)],
          intensity: pointLights[index + 6]!,
          range: pointLights[index + 7]!,
          radius: 0.1,
          shadows: null,
        });
      }
    }
    lighting.pointLights = lights;
  }

  /** The renderer's `draw` hook: the whole frame through `Lighting.render`, or nothing until it loaded. */
  draw(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.PerspectiveCamera): void {
    if (this.lighting) {
      this.lighting.render(renderer, scene, camera);
    } else {
      renderer.clear();
    }
  }
}
