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
import { profileRenderPasses } from "./frame-profile";
import { HeldMatrices } from "./held-matrices";
import { shadowLodReady } from "./shadow-lod";
import { loadWasm, wasm } from "../wasm";

/**
 * Greyhaven's lighting on the web (#600): render-engine's `@moritzbrantner/three-lighting`, driven by the shared
 * `mmorpg_presentation::lighting` (#1059) like the native client's render-engine scene
 * (`crates/mmorpg-client/src/graphics/frame.rs`), so both clients light alike:
 * the zone hour places the sun and moon (`DayCycle`), Bevy's atmosphere draws the sky, filters them and lights the
 * scene, and the frame is exposed by sun elevation and tone mapped. Every lit mesh draws with a `LitMaterial`;
 * anything else (sprites, effects) is drawn as three draws it.
 */

/** The shared lighting constants (`mmorpg_presentation::lighting`), read from the module once it loaded. */
export type LightingTables = {
  noonElevationDegrees: number; moonLagHours: number; moonLux: number; moonColor: Vec3; bounceColor: Vec3;
  /** Aerial perspective (#526): contrast falls to 5 % here; the per-metre coefficients below redden distance least. */
  fogVisibilityMetres: number; fogExtinction: Vec3; fogInscattering: Vec3;
};

let tables: LightingTables | null = null;

/** The shared lighting constants; needs the module loaded. */
export function lightingTables(): LightingTables {
  tables ??= JSON.parse(wasm.lightingTables()) as LightingTables;
  return tables;
}

/** Emissive presentation colours glow at this luminance, cd/m² (`EMISSIVE_NITS`). */
export const EMISSIVE_NITS = 80;
/**
 * Sun cascades for the gameplay camera, the native `SUN_SHADOWS` out to the same 160 m but in three cascades instead
 * of four: three.js redraws the scene once per cascade, one draw call per mesh, and a fourth cascade cost the browser
 * about a fifth of its frame rate in Greyhaven (#600). Since #615 the static scenery draws in batches (about 1 100
 * draw calls a frame instead of 2 700), but a fourth cascade still costs about a tenth of the frame rate (45 instead of
 * 50 fps in Chrome on an RTX 3060 Ti), so three cascades remain.
 */
export const SUN_SHADOWS: SunShadows = {
  numCascades: 3, minDistance: 0.2, firstCascadeFarBound: 12, maxDistance: 160, overlapProportion: 0.2, depthBias: 0.02, normalBias: 1.8,
};
/**
 * The far cascades take turns (#1083): the near one is redrawn every frame, the two far ones every other frame each,
 * so a frame draws two cascades. A kept cascade holds its map and matrices (no swimming) and is redrawn early when
 * the view leaves it (three-lighting `cascadeUpdateIntervals`).
 */
export const CASCADE_UPDATE_INTERVALS: readonly number[] = [1, 2, 2];
/**
 * A caster (or instance) whose bounding radius is under this many texels of a cascade casts nothing there (#1083):
 * under the Gaussian filter it would leave a faint smudge of a few texels (three-lighting `minCasterTexels`).
 */
export const MIN_CASTER_TEXELS = 2;
/** The presentation materials' surface (`standard_material`). */
const ROUGHNESS = 0.86;
const METALLIC = 0.02;

/**
 * One frame's shared `SceneLight` (`wasm.sceneLight`'s layout): everything but the sun's and moon's directions. Without
 * the atmosphere (the character-selection stage, which keeps its page backdrop, or a browser lacking
 * `OES_texture_float_linear`/`EXT_color_buffer_float`) its analytic sunlight and sky ambient light the scene; the
 * canvas is then transparent where nothing is drawn, so the CSS sky shows.
 */
type SceneLight = {
  elevation: number; ev100: number; transmission: number; environmentIntensity: number; bounceNits: number; sunShadows: boolean;
  sunlight: Vec3; ambientColor: Vec3; ambientNits: number;
};

function sceneLight(toSunUp: number, interior: number, sky: boolean): SceneLight {
  const v = wasm.sceneLight(toSunUp, interior, sky, RAW_SUNLIGHT);
  return {
    elevation: v[0]!, ev100: v[1]!, transmission: v[2]!, environmentIntensity: v[3]!, bounceNits: v[4]!, sunShadows: v[5] === 1,
    sunlight: [v[6]!, v[7]!, v[8]!], ambientColor: [v[9]!, v[10]!, v[11]!], ambientNits: v[12]!,
  };
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
  const material = new LitMaterial({
    baseColor: linearRgb(node.color ?? 0xffffff),
    alpha: opacity,
    alphaMode: opacity < 1 ? "blend" : "opaque",
    emissive: linearRgb(node.emissive ?? 0x000000).map((channel) => channel * EMISSIVE_NITS) as unknown as Vec3,
    perceptualRoughness: opacity < 1 && node.unlit !== true ? 0.3 : ROUGHNESS,
    metallic: opacity < 1 ? 0 : METALLIC,
    doubleSided: node.doubleSided === true,
    unlit: node.unlit === true,
  });
  // Blended water/foam receives shadows without stamping an opaque silhouette on its bed.
  // The visible blend shader ignores this cutoff; shadow variants share the uniform.
  if (opacity < 1) material.uniforms.alphaCutoff = { value: 1 };
  return material;
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
export type LightingFrame = { hour: number; sky: boolean; interior: number; fog: string | null; pointLights: Float32Array | null };

const negate = ([x, y, z]: Vec3): Vec3 => [-x, -y, -z];

/** The page's `Lighting`: the day clock's sun and moon, the sky, fog and the scenery's point lights. */
export class GreyhavenLighting {
  #lighting: Lighting | null = null;
  #frame: LightingFrame | null = null;
  /** Whether the browser can draw the atmosphere; known at the first draw. */
  #skySupported: boolean | null = null;
  /** Greyhaven's sun path (`mmorpg_presentation::lighting`), once the module loaded. */
  #dayCycle: DayCycle | null = null;
  /** The direction sunlight travelled at the last configured frame while the sun cast shadows. */
  #sunShadow: Vec3 | null = null;

  /**
   * Loads the tone-mapping LUTs from `base`, where `vite.config.ts` copies them (`luts/`), and the module that owns the
   * lighting curves; nothing draws before.
   */
  constructor(base: string) {
    void Promise.all([loadToneMappingLuts({
      tonyMcMapface: `${base}luts/tony_mc_mapface.ktx2`,
      agx: `${base}luts/AgX-default_contrast.ktx2`,
    }), loadWasm(), shadowLodReady]).then(([luts]) => {
      const lighting = new Lighting(luts);
      lighting.tonemapping = "tonyMcMapface";
      lighting.shadowFilter = "gaussian";
      lighting.cascadeUpdateIntervals = [...CASCADE_UPDATE_INTERVALS];
      lighting.minCasterTexels = MIN_CASTER_TEXELS;
      const shared = lightingTables();
      this.#dayCycle = new DayCycle({ noonElevation: THREE.MathUtils.degToRad(shared.noonElevationDegrees), moonLagHours: shared.moonLagHours });
      this.#lighting = lighting;
    });
  }

  /** Whether the lighting has loaded, so `prepare` compiles its own passes' programs (#855). */
  get ready(): boolean {
    return this.#lighting !== null && this.#dayCycle !== null;
  }

  /** Whether the last frame's sky is the atmosphere's; otherwise the page (the CSS sky) shows through. */
  get drawsSky(): boolean {
    return this.#frame?.sky === true && this.#skySupported !== false;
  }

  /**
   * The direction sunlight travelled at the last drawn frame while the sun cast shadows, or null (#1038): the placed
   * kits keep the off-screen casters whose shadow it may throw into view.
   */
  get sunShadowDirection(): Vec3 | null {
    return this.#sunShadow;
  }

  /** Sets the lights for the next frame. */
  apply(frame: LightingFrame): void {
    this.#frame = frame;
  }

  /** The native client's `frame::scene` for `frame`: the shared `SceneLight` bound to three. */
  #configure(lighting: Lighting, dayCycle: DayCycle, frame: LightingFrame): void {
    const { hour, fog, pointLights } = frame;
    const shared = lightingTables();
    const sun = dayCycle.toSun(hour);
    const light = sceneLight(sun[1], frame.interior, frame.sky);
    const { transmission } = light;
    lighting.ev100 = light.ev100;
    const shadows = light.sunShadows ? SUN_SHADOWS : null;
    this.#sunShadow = null;
    if (this.drawsSky) {
      lighting.sky = { ...DEFAULT_SKY, environmentIntensity: light.environmentIntensity };
      lighting.clearColor = [0, 0, 0];
      lighting.clearAlpha = 1;
      lighting.sun = { direction: negate(sun), color: [1, 1, 1], illuminance: RAW_SUNLIGHT * transmission, shadows };
      if (shadows) { this.#sunShadow = negate(sun); }
      lighting.moon = { direction: negate(dayCycle.toMoon(hour)), color: shared.moonColor, illuminance: shared.moonLux * transmission };
      // The sky's environment light is the ambient light, plus the sunlit ground's bounce by day.
      lighting.ambient = { color: shared.bounceColor, brightness: light.bounceNits };
    } else {
      lighting.sky = null;
      lighting.moon = null;
      lighting.clearAlpha = 0;
      const lux = Math.max(...light.sunlight);
      // One directional light: the sun while it outshines the moon, then the moon.
      if (lux > shared.moonLux) {
        lighting.sun = { direction: negate(sun), color: light.sunlight.map((channel) => channel / lux) as unknown as Vec3, illuminance: lux * transmission, shadows };
        if (shadows) { this.#sunShadow = negate(sun); }
      } else {
        const moon = dayCycle.toMoon(hour);
        lighting.sun = moon[1] > 0 ? { direction: negate(moon), color: shared.moonColor, illuminance: shared.moonLux * transmission, shadows: null } : null;
      }
      lighting.ambient = { color: light.ambientColor, brightness: light.ambientNits };
    }
    lighting.fog = fog === null ? null : {
      color: [...linearRgb(fog), 1],
      falloff: FogFalloff.atmospheric(shared.fogExtinction, shared.fogInscattering),
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
    const dayCycle = this.#dayCycle;
    if (!lighting || !dayCycle || !this.#frame) {
      renderer.clear();
      return;
    }
    this.#skySupported ??= renderer.extensions.has("OES_texture_float_linear") && renderer.extensions.has("EXT_color_buffer_float");
    this.#configure(lighting, dayCycle, this.#frame);
    onceAcrossPasses(scene, () => profileRenderPasses(renderer, scene, camera, lighting.sky !== null, () => lighting.render(renderer, scene, camera)));
  }

  /**
   * The renderer's `prepare` hook (#855): creates every program `draw` would draw the frame `apply` last set with (its
   * sky, sun cascades, local-light views, HDR and tone-mapping passes, with the defines each pass gives the lit
   * materials), drawing nothing, so that frame creates none. With `KHR_parallel_shader_compile` it resolves once they
   * have linked. Before the lighting loaded, three's own programs.
   */
  prepare(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.PerspectiveCamera): Promise<unknown> {
    const lighting = this.#lighting;
    const dayCycle = this.#dayCycle;
    if (!lighting || !dayCycle || !this.#frame) { return renderer.compileAsync(scene, camera); }
    this.#skySupported ??= renderer.extensions.has("OES_texture_float_linear") && renderer.extensions.has("EXT_color_buffer_float");
    this.#configure(lighting, dayCycle, this.#frame);
    const linked = lighting.prepare(renderer, scene, camera);
    if (renderer.extensions.has("KHR_parallel_shader_compile")) { return linked; }
    // Without the extension the browser links each program in the order it was created, and the first use of one waits
    // for it. Waiting here would only put that wait in front of the first frame, plus a timer yield per program; once
    // the programs exist the GPU process links them while the page goes on to that frame.
    linked.catch((error: unknown) => console.warn("Preparing the lighting's programs failed.", error));
    return Promise.resolve();
  }
}

const skeletonUpdate = THREE.Skeleton.prototype.update;
/** Per held skeleton, the pose revision its bone texture holds (`HeldMatrices`, #1257). */
const uploadedPose = new WeakMap<THREE.Skeleton, number>();
/** The frame's skeletons already brought up to date; cleared after each frame's passes. */
const updatedSkeletons = new Set<THREE.Skeleton>();
/** Skeletons whose bone matrices were recomputed (and bone textures re-uploaded) since the last `takeSkeletonUpdates`. */
let skeletonUpdates = 0;

/** Bone matrix recomputations (each a bone texture upload) since the last call (#1257). */
export function takeSkeletonUpdates(): number {
  const updates = skeletonUpdates;
  skeletonUpdates = 0;
  return updates;
}

function skeletonUpdateOnce(this: THREE.Skeleton): void {
  if (updatedSkeletons.has(this)) { return; }
  updatedSkeletons.add(this);
  // A held skeleton whose root was not posed since its texture last uploaded keeps it.
  const pose = HeldMatrices.poseOf(this);
  if (pose !== undefined) {
    if (uploadedPose.get(this) === pose) { return; }
    uploadedPose.set(this, pose);
  }
  skeletonUpdates += 1;
  skeletonUpdate.call(this);
}

/** The scene's world matrices, already current for the frame's passes. */
function matricesCurrent(): void {}

/**
 * Runs `passes` (one `renderer.render` per sun cascade, local shadow view and the main pass) with the scene's world
 * matrices and skeletons brought up to date once: three redoes both in every `render` call, though nothing moves
 * between the passes of one frame, recomputing every world matrix and re-uploading every bone texture each time. The
 * lighting's own frame asks the scene for its world matrices once more before its passes (render-engine's `#frame`);
 * they were brought up to date just before, so that second walk of every node is skipped too (#1257).
 */
function onceAcrossPasses(scene: THREE.Scene, passes: () => void): void {
  const autoUpdate = scene.matrixWorldAutoUpdate;
  // Unconditionally, as the lighting's frame did.
  scene.updateMatrixWorld();
  scene.matrixWorldAutoUpdate = false;
  const ownUpdate = Object.hasOwn(scene, "updateMatrixWorld") ? scene.updateMatrixWorld : null;
  scene.updateMatrixWorld = matricesCurrent;
  THREE.Skeleton.prototype.update = skeletonUpdateOnce;
  try {
    passes();
  } finally {
    THREE.Skeleton.prototype.update = skeletonUpdate;
    updatedSkeletons.clear();
    if (ownUpdate) { scene.updateMatrixWorld = ownUpdate; } else { delete (scene as Partial<THREE.Scene>).updateMatrixWorld; }
    scene.matrixWorldAutoUpdate = autoUpdate;
  }
}
