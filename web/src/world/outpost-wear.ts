import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";

/** The Outpost kit's wear masks entry (asset-tooling docs/outpost-kit.md#wear-masks); older bundles lack it. */
export const OUTPOST_WEAR_MASKS = "outpost.kit.wear-masks";

/**
 * How each kit material ages, and which texture's mask it reads: plaster falls to wattle and daub, timber and planks
 * grey, stone gathers moss, and roofs gather moss and lose units.
 */
const KINDS: Readonly<Record<string, { kind: number; mask: string }>> = {
  "outpost-plaster": { kind: 0, mask: "plaster" },
  "outpost-timber": { kind: 1, mask: "wood" },
  "outpost-wood": { kind: 1, mask: "wood" },
  "outpost-stone": { kind: 2, mask: "stone" },
  "outpost-shingle": { kind: 3, mask: "shingle" },
  "outpost-slate": { kind: 3, mask: "slate" },
};

/**
 * Ages a sampled material by the draw's wear in [0, 1] (`vWear`) from its texture's mask: R the wear plaster falls to
 * daub from, or a roof unit's id, G how much full wear darkens it, B plaster's daub tone or the wear moss settles from. The
 * constants are the recipe's (`adapters/blender/outpost.py`, `weathered`), in linear light.
 */
const WEAR_FRAGMENT = /* glsl */ `
uniform sampler2D wearMask;
in float vWear;
flat in uint vPlacement;
// The share of roof units missing at full wear (the recipe's MISSING_SHARE).
#define MISSING_SHARE 0.07

MaterialSample sampleMaterial() {
    MaterialSample m = sampleCleanMaterial();
    float wear = clamp(vWear, 0.0, 1.0);
    if (wear <= 0.0) return m;
    vec3 mask = texture(wearMask, vUv).rgb;
    vec3 base = m.baseColor.rgb;
    float lum = dot(base, vec3(0.2126, 0.7152, 0.0722));
    float reveal = clamp((wear - mask.r) * 20.0, 0.0, 1.0);
#if OUTPOST_WEAR_KIND == 0
    // Plaster: dulled towards ochre, stained, and fallen to wattle and daub behind a broken edge.
    float rim = clamp((wear - mask.r) * 20.0 + 1.5, 0.0, 1.0) - reveal;
    base = mix(base, vec3(1.2, 0.975, 0.655) * lum, 0.35 * wear) * (1.0 - wear * mask.g) * (1.0 - 0.25 * rim);
    vec3 daub = mix(vec3(0.033, 0.023, 0.015), vec3(0.188, 0.156, 0.113), mask.b) * 0.85;
    base = mix(base, daub, reveal);
    m.diffuseOcclusion *= 1.0 - 0.35 * reveal;
    m.perceptualRoughness = mix(m.perceptualRoughness, 0.95, reveal);
    m.Nt = normalize(mix(m.Nt, vec3(0.0, 0.0, 1.0), reveal));
#elif OUTPOST_WEAR_KIND == 1
    // Timber and planks: greyed and grimy.
    base = mix(base, lum * vec3(1.012, 1.023, 1.056), 0.55 * wear) * (1.0 - wear * mask.g);
#else
    // Stone and roofs: grime, then moss from the joints out.
    base *= 1.0 - wear * mask.g;
    float moss = clamp((wear - mask.b) * 3.0, 0.0, 1.0) * wear;
    base = mix(base, vec3(0.047, 0.073, 0.015), 0.85 * moss);
    m.perceptualRoughness = clamp(m.perceptualRoughness + 0.1 * moss, 0.0, 1.0);
#if OUTPOST_WEAR_KIND == 3
    // Roofs: R is the unit's id. Each occurrence of a unit (its id, its 1 m tile, its module) goes missing at its own
    // random wear, so the holes scatter instead of repeating with the tile; more wear only adds holes. The id is read
    // unfiltered: filtering would blend neighbouring ids at the joints.
    ivec2 size = textureSize(wearMask, 0);
    ivec2 texel = clamp(ivec2(fract(vUv) * vec2(size)), ivec2(0), size - 1);
    uint unit = uint(texelFetch(wearMask, texel, 0).r * 64.0);
    uvec2 tile = uvec2(ivec2(floor(vUv)) + 32768);
    uint h = unit * 0x8da6b343u ^ tile.x * 0xd8163841u ^ tile.y * 0xcb1ab31fu ^ vPlacement * 0x165667b1u;
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15; h *= 0x846ca68bu; h ^= h >> 16;
    reveal = float(h >> 8) / 16777216.0 < MISSING_SHARE * wear ? 1.0 : 0.0;
    // Missing units leave a dark recess.
    base = mix(base, vec3(0.0027, 0.0024, 0.0022), reveal);
    m.diffuseOcclusion *= 1.0 - 0.7 * reveal;
    m.perceptualRoughness = mix(m.perceptualRoughness, 0.95, reveal);
    m.Nt = normalize(mix(m.Nt, vec3(0.0, 0.0, 1.0), reveal));
#endif
#endif
    m.baseColor.rgb = base;
    return m;
}

`;

/** Each texture's wear mask from the bundle's wear-masks entry: the base colour of `outpost-wear-<texture>`, as data. */
export function wearMasks(gltf: GLTF): Map<string, THREE.Texture> {
  const masks = new Map<string, THREE.Texture>();
  gltf.scene.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) { return; }
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
      const map = (material as THREE.MeshStandardMaterial).map;
      const name = material.name.match(/^outpost-wear-([a-z]+)$/)?.[1];
      if (!map || !name) { continue; }
      // Thresholds and amounts, not colour: no sRGB decode.
      map.colorSpace = THREE.NoColorSpace;
      map.needsUpdate = true;
      masks.set(name, map);
    }
  });
  return masks;
}

const patched = new WeakSet<THREE.Material>();

/**
 * Makes every kit `LitMaterial` under `roots` that has a mask in `masks` age by its draw's wear, which a
 * `BatchedMesh` carries in its instance colour's alpha ([`StaticBatches`]): all wear levels then share one material,
 * and so one batch. A draw without an instance colour stays pristine. Returns how many materials age.
 */
export function applyOutpostWear(roots: readonly THREE.Object3D[], masks: ReadonlyMap<string, THREE.Texture>): number {
  let count = 0;
  for (const root of roots) {
    root.traverse((object) => {
      if (!(object instanceof THREE.Mesh)) { return; }
      for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
        const kind = KINDS[material.name];
        const mask = kind && masks.get(kind.mask);
        if (!kind || !mask || patched.has(material) || !(material instanceof THREE.ShaderMaterial)) { continue; }
        patched.add(material);
        count += 1;
        material.uniforms.wearMask = { value: mask };
        material.defines = { ...material.defines, OUTPOST_WEAR_KIND: kind.kind };
        material.onBeforeCompile = (shader) => {
          shader.vertexShader = replaceOnce(shader.vertexShader, "out vec4 vColor;", "out vec4 vColor;\nout float vWear;\nflat out uint vPlacement;");
          shader.vertexShader = replaceOnce(shader.vertexShader, "    vColor = color;", [
            "    vColor = color;",
            "#ifdef USE_BATCHING_COLOR",
            "    vWear = getBatchingColor(getIndirectIndex(gl_DrawID)).a;",
            "#else",
            "    vWear = 0.0;",
            "#endif",
            "    // The module's placement, to the centimetre: its roof units go missing independently of other modules'.",
            "    ivec3 placed = ivec3(floor(litObjectMatrix()[3].xyz * 100.0 + 0.5));",
            "    vPlacement = uint(placed.x) * 73856093u ^ uint(placed.y) * 19349663u ^ uint(placed.z) * 83492791u;",
          ].join("\n"));
          shader.fragmentShader = replaceOnce(shader.fragmentShader, "MaterialSample sampleMaterial() {", "MaterialSample sampleCleanMaterial() {");
          shader.fragmentShader = replaceOnce(shader.fragmentShader, "vec3 litRadiance(", `${WEAR_FRAGMENT}vec3 litRadiance(`);
        };
        material.customProgramCacheKey = () => `outpost-wear-${kind.kind}`;
        material.needsUpdate = true;
      }
    });
  }
  return count;
}

function replaceOnce(source: string, anchor: string, replacement: string): string {
  const at = source.indexOf(anchor);
  if (at < 0 || source.indexOf(anchor, at + anchor.length) >= 0) {
    throw new Error(`The lit shader no longer has exactly one "${anchor}".`);
  }
  return source.slice(0, at) + replacement + source.slice(at + anchor.length);
}
