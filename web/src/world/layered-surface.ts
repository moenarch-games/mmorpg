import * as THREE from "three";
import { litMaterial } from "./lighting";

/** A layered kit's surface (#732) as `mmorpg-wasm`'s `kitSurface` gives it, at Greyhaven's weathering. */
export type LayeredSurface = {
  /** The glTF vertex attribute of the mask: R soil, G moss, B grass, A cavity. */
  maskAttribute: string;
  /** The pieces' shared untextured material, which the blend replaces. */
  material: string;
  /** The bundle entry holding the shared library's materials. */
  library: string;
  layers: Record<"limestone" | "moss" | "soil", { material: string; tileMetres: number }>;
  weathering: string;
  moss: number;
  cavity: number;
  mossRamp: number;
  mossCover: number;
  cavityColour: [number, number, number];
};

/** Material `name`'s textures in the library scene. */
function libraryMaps(library: THREE.Object3D, name: string): { map: THREE.Texture; normalMap: THREE.Texture | null } {
  let found: THREE.MeshStandardMaterial | undefined;
  library.traverse((object) => {
    if (found || !(object instanceof THREE.Mesh)) { return; }
    const materials = (Array.isArray(object.material) ? object.material : [object.material]) as THREE.MeshStandardMaterial[];
    found = materials.find((material) => material.name === name);
  });
  if (!found?.map) { throw new Error(`the surface library has no ${name} texture`); }
  for (const texture of [found.map, found.normalMap]) {
    if (texture) { texture.wrapS = texture.wrapT = THREE.RepeatWrapping; }
  }
  return { map: found.map, normalMap: found.normalMap };
}

/**
 * The blend a layered kit's pieces draw with (#732, `mmorpg_presentation::surface`): the shared library's limestone,
 * moss and soil, sampled triplanar in the piece's object space at each layer's `tileMetres`, mixed by the piece's
 * per-vertex mask in the bake's order (moss by G past the weathering's threshold, soil by R, moss by B) and darkened
 * toward the cavity colour by the weathering's strength times A; the limestone's normal map perturbs the normal
 * triplanar too. One `LitMaterial` (#600) patched in place, so the world's lighting, shadows and the levels'
 * cross-fade copy treat it like any other. The native client draws the same blend from the same numbers.
 */
export function layeredSurfaceMaterial(surface: LayeredSurface, library: THREE.Object3D): THREE.Material {
  const limestone = libraryMaps(library, surface.layers.limestone.material);
  const moss = libraryMaps(library, surface.layers.moss.material);
  const soil = libraryMaps(library, surface.layers.soil.material);
  const material = litMaterial(new THREE.MeshStandardMaterial({ name: surface.material, roughness: 0.95, metalness: 0 })) as THREE.ShaderMaterial;
  Object.assign(material.uniforms, {
    surfaceLimestone: { value: limestone.map },
    surfaceLimestoneNormal: { value: limestone.normalMap ?? limestone.map },
    surfaceMoss: { value: moss.map },
    surfaceSoil: { value: soil.map },
    surfaceTiles: { value: new THREE.Vector3(surface.layers.limestone.tileMetres, surface.layers.moss.tileMetres, surface.layers.soil.tileMetres) },
    surfaceBlend: { value: new THREE.Vector4(surface.moss, surface.cavity, surface.mossRamp, surface.mossCover) },
    surfaceCavityColour: { value: new THREE.Vector3(...surface.cavityColour) },
  });
  const mask = surface.maskAttribute.toLowerCase();
  const normalMapped = limestone.normalMap ? 1 : 0;
  material.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader.replace("void main() {", `in vec4 ${mask};
out vec4 vSurfaceMask;
out vec3 vSurfacePosition;
out vec3 vSurfaceNormal;
out vec3 vSurfaceToWorld0;
out vec3 vSurfaceToWorld1;
out vec3 vSurfaceToWorld2;
void main() {
    vSurfaceMask = ${mask};
    vSurfacePosition = position;
    vSurfaceNormal = normal;
    mat3 surfaceToWorld = inverseTranspose3x3(mat3(litModelMatrix()));
    vSurfaceToWorld0 = surfaceToWorld[0];
    vSurfaceToWorld1 = surfaceToWorld[1];
    vSurfaceToWorld2 = surfaceToWorld[2];`);
    shader.fragmentShader = shader.fragmentShader
      .replace("MaterialSample sampleMaterial() {", `in vec4 vSurfaceMask;
in vec3 vSurfacePosition;
in vec3 vSurfaceNormal;
in vec3 vSurfaceToWorld0;
in vec3 vSurfaceToWorld1;
in vec3 vSurfaceToWorld2;
uniform sampler2D surfaceLimestone;
uniform sampler2D surfaceLimestoneNormal;
uniform sampler2D surfaceMoss;
uniform sampler2D surfaceSoil;
uniform vec3 surfaceTiles;
uniform vec4 surfaceBlend;
uniform vec3 surfaceCavityColour;
vec3 surfaceWeights() {
    vec3 w = pow(abs(normalize(vSurfaceNormal)), vec3(4.0));
    return w / (w.x + w.y + w.z);
}
vec3 surfaceSample(sampler2D map, float tile, vec3 w) {
    vec3 p = vSurfacePosition / tile;
    return texture(map, p.zy).rgb * w.x + texture(map, p.xz).rgb * w.y + texture(map, p.xy).rgb * w.z;
}
// The limestone's normal map, triplanar, as a world-space shading normal (whiteout-free, as the reference blend).
vec3 surfaceNormal(vec3 fallback) {
    if (${normalMapped} == 0) { return fallback; }
    vec3 w = surfaceWeights();
    vec3 p = vSurfacePosition / surfaceTiles.x;
    vec2 tx = texture(surfaceLimestoneNormal, p.zy).xy * 2.0 - 1.0;
    vec2 ty = texture(surfaceLimestoneNormal, p.xz).xy * 2.0 - 1.0;
    vec2 tz = texture(surfaceLimestoneNormal, p.xy).xy * 2.0 - 1.0;
    vec3 detailed = vec3(0.0, tx.y, tx.x) * w.x + vec3(ty.x, 0.0, ty.y) * w.y + vec3(tz, 0.0) * w.z + normalize(vSurfaceNormal);
    return normalize(mat3(vSurfaceToWorld0, vSurfaceToWorld1, vSurfaceToWorld2) * detailed);
}
MaterialSample sampleMaterial() {`)
      .replace("    m.baseColor = vColor * baseColor;", `    m.baseColor = vColor * baseColor;
    {
        vec3 w = surfaceWeights();
        vec3 surface = surfaceSample(surfaceLimestone, surfaceTiles.x, w);
        vec3 moss = surfaceSample(surfaceMoss, surfaceTiles.y, w);
        surface = mix(surface, moss, smoothstep(surfaceBlend.x, surfaceBlend.x + surfaceBlend.z, vSurfaceMask.g) * surfaceBlend.w);
        surface = mix(surface, surfaceSample(surfaceSoil, surfaceTiles.z, w), vSurfaceMask.r);
        surface = mix(surface, moss, vSurfaceMask.b);
        surface *= mix(vec3(1.0), surfaceCavityColour, clamp(surfaceBlend.y * vSurfaceMask.a, 0.0, 1.0));
        m.baseColor.rgb *= surface;
    }`)
      .replace("    color = vec4(litRadiance(worldNormal, N, m), m.baseColor.a);", `    N = surfaceNormal(N);
    color = vec4(litRadiance(worldNormal, N, m), m.baseColor.a);`);
  };
  material.customProgramCacheKey = () => `layered-surface-${mask}-${normalMapped}`;
  return material;
}

/** Gives every mesh under `roots` drawn with the surface's untextured `material` the blend instead. */
export function applyLayeredSurface(roots: readonly THREE.Object3D[], surface: LayeredSurface, blend: THREE.Material): void {
  for (const root of roots) {
    root.traverse((object) => {
      if (!(object instanceof THREE.Mesh)) { return; }
      const swap = (material: THREE.Material) => material.name === surface.material ? blend : material;
      object.material = Array.isArray(object.material) ? object.material.map(swap) : swap(object.material as THREE.Material);
    });
  }
}
