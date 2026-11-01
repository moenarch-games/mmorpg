import * as THREE from "three";
import {
  DayCycle,
  DEFAULT_SKY,
  FogFalloff,
  Lighting,
  LitMaterial,
  loadToneMappingLuts,
  RAW_SUNLIGHT,
  type PointLight,
  type SunShadows,
  type Vec3,
} from "@moritzbrantner/three-lighting";
import type { RendererMaterialRequest } from "@moritzbrantner/three-d-renderer";

/**
 * Greyhaven's lighting on the web (#600): render-engine's `@moritzbrantner/three-lighting`, driven with the values
 * the native client gives render-engine (`crates/mmorpg-client/src/graphics/frame.rs`), so both clients light alike:
 * the zone hour places the sun and moon (`DayCycle`), Bevy's atmosphere draws the sky, filters them and lights the
 * scene, and the frame is exposed by sun elevation and tone mapped. Every lit mesh draws with a `LitMaterial`;
 * anything else (sprites, effects) is drawn as three draws it.
 */

/** Greyhaven's sun path (`DAY_CYCLE`): 60° above the southern horizon at noon, the moon 12 h behind. */
const DAY_CYCLE = new DayCycle({ noonElevation: THREE.MathUtils.degToRad(60), moonLagHours: 12 });
/** Moonlight, brighter than a real full moon so night stays playable (`MOON_LUX`, `MOON_COLOR`). */
const MOON_LUX = 2;
const MOON_COLOR: Vec3 = [0.75, 0.85, 1];
/** EV100 by sun elevation in degrees (`EXPOSURE`). */
const EXPOSURE: readonly (readonly [number, number])[] = [[-12, 2], [-4, 6], [2, 11], [15, 13.5], [60, 14.5]];
/** Emissive presentation colours glow at this luminance, cd/m² (`EMISSIVE_NITS`). */
export const EMISSIVE_NITS = 80;
/**
 * Sun cascades for the gameplay camera, the native `SUN_SHADOWS` out to the same 160 m but in three cascades instead
 * of four: three.js redraws the scene once per cascade, one draw call per mesh, and a fourth cascade cost the browser
 * about a fifth of its frame rate in Greyhaven (#600). Since render-engine#51 each cascade skips the instanced meshes
 * it does not reach, but the frame stays bound by draw calls, so three cascades remain.
 */
const SUN_SHADOWS: SunShadows = {
  numCascades: 3, minDistance: 0.2, firstCascadeFarBound: 12, maxDistance: 160, overlapProportion: 0.2, depthBias: 0.02, normalBias: 1.8,
};
/** Below this sun elevation (degrees) the sun casts no shadows (`SHADOWLESS_BELOW`). */
const SHADOWLESS_BELOW = -3;
/**
 * Aerial perspective (#526), as `mmorpg_client::environment`: atmospheric fog whose contrast falls to 5 % at
 * `FOG_VISIBILITY_METRES` (Bevy's Koschmieder threshold), red light fading and blue gathering first, so distant ranges
 * turn blue but keep their shading.
 */
const FOG_VISIBILITY_METRES = 3200;
const FOG_DENSITY = -Math.log(0.05) / FOG_VISIBILITY_METRES;
const FOG_EXTINCTION: Vec3 = [1.2 * FOG_DENSITY, FOG_DENSITY, 0.8 * FOG_DENSITY];
const FOG_INSCATTERING: Vec3 = [0.8 * FOG_DENSITY, FOG_DENSITY, 1.2 * FOG_DENSITY];
/** The presentation materials' surface (`standard_material`). */
const ROUGHNESS = 0.86;
const METALLIC = 0.02;

/**
 * Without the atmosphere (the character-selection stage, which keeps its page backdrop, or a browser lacking
 * `OES_texture_float_linear`/`EXT_color_buffer_float`) the sun is filtered by an analytic Earth atmosphere (Bevy's
 * Rayleigh, Mie and ozone optical depths over the Kasten–Young air mass), the moon replaces it once it outshines it,
 * and the sky's light is an ambient term by sun elevation, tuned against the native frames; the canvas is transparent
 * where nothing is drawn, so the CSS sky shows.
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

/**
 * One frame's lighting inputs: the hour, whether the atmosphere draws the sky (the world) or the page shows through
 * (the selection stage), the fog colour (`#rrggbb`, null for none) and the point lights.
 */
export type LightingFrame = { hour: number; sky: boolean; fog: string | null; pointLights: Float32Array | null };

const negate = ([x, y, z]: Vec3): Vec3 => [-x, -y, -z];

/** The page's `Lighting`: the day clock's sun and moon, the sky, fog and the scenery's point lights. */
export class GreyhavenLighting {
  #lighting: Lighting | null = null;
  #frame: LightingFrame | null = null;
  /** Whether the browser can draw the atmosphere; known at the first draw. */
  #skySupported: boolean | null = null;

  /** Loads the tone-mapping LUTs from `base`, where `vite.config.ts` copies them (`luts/`); nothing draws before. */
  constructor(base: string) {
    void loadToneMappingLuts({
      tonyMcMapface: `${base}luts/tony_mc_mapface.ktx2`,
      agx: `${base}luts/AgX-default_contrast.ktx2`,
    }).then((luts) => {
      const lighting = new Lighting(luts);
      lighting.tonemapping = "tonyMcMapface";
      lighting.shadowFilter = "gaussian";
      this.#lighting = lighting;
    });
  }

  /** Whether the last frame's sky is the atmosphere's; otherwise the page (the CSS sky) shows through. */
  get drawsSky(): boolean {
    return this.#frame?.sky === true && this.#skySupported !== false;
  }

  /** Sets the lights for the next frame. */
  apply(frame: LightingFrame): void {
    this.#frame = frame;
  }

  /** The native client's `frame::scene` for `frame`. */
  #configure(lighting: Lighting, frame: LightingFrame): void {
    const { hour, fog, pointLights } = frame;
    const sun = DAY_CYCLE.toSun(hour);
    const elevation = THREE.MathUtils.radToDeg(Math.asin(Math.min(1, Math.max(-1, sun[1]))));
    lighting.ev100 = ev100(elevation);
    const shadows = elevation > SHADOWLESS_BELOW ? SUN_SHADOWS : null;
    if (this.drawsSky) {
      lighting.sky = DEFAULT_SKY;
      lighting.clearColor = [0, 0, 0];
      lighting.clearAlpha = 1;
      lighting.sun = { direction: negate(sun), color: [1, 1, 1], illuminance: RAW_SUNLIGHT, shadows };
      lighting.moon = { direction: negate(DAY_CYCLE.toMoon(hour)), color: MOON_COLOR, illuminance: MOON_LUX };
      // The sky's environment light is the ambient light.
      lighting.ambient = { color: [1, 1, 1], brightness: 0 };
    } else {
      lighting.sky = null;
      lighting.moon = null;
      lighting.clearAlpha = 0;
      const light = sunlight(elevation);
      const lux = Math.max(...light);
      // One directional light: the sun while it outshines the moon, then the moon.
      if (lux > MOON_LUX) {
        lighting.sun = { direction: negate(sun), color: light.map((channel) => channel / lux) as unknown as Vec3, illuminance: lux, shadows };
      } else {
        const moon = DAY_CYCLE.toMoon(hour);
        lighting.sun = moon[1] > 0 ? { direction: negate(moon), color: MOON_COLOR, illuminance: MOON_LUX, shadows: null } : null;
      }
      const twilight = Math.max(0, 1 - Math.abs(elevation) / 8);
      lighting.ambient = {
        color: SKY_COLOR.map((channel, index) => channel + (TWILIGHT_COLOR[index]! - channel) * twilight) as unknown as Vec3,
        brightness: interpolate(SKY_AMBIENT, elevation),
      };
    }
    lighting.fog = fog === null ? null : {
      color: [...linearRgb(fog), 1],
      falloff: FogFalloff.atmospheric(FOG_EXTINCTION, FOG_INSCATTERING),
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
    const lighting = this.#lighting;
    if (!lighting || !this.#frame) {
      renderer.clear();
      return;
    }
    this.#skySupported ??= renderer.extensions.has("OES_texture_float_linear") && renderer.extensions.has("EXT_color_buffer_float");
    this.#configure(lighting, this.#frame);
    lighting.render(renderer, scene, camera);
  }
}
