import * as THREE from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";

/**
 * Merges a unit model's meshes into one `SkinnedMesh` per material class (#615): a Quaternius or Ranger unit is 10–23
 * meshes with one material each, and every mesh is one draw in the main pass and again in every sun cascade.
 *
 * Meshes merge when their materials differ only in base colour (and alpha), which moves into the vertex `color`
 * attribute the lit material multiplies by (glTF `COLOR_0`): same textures, metallic, roughness, emissive, alpha
 * test and sides. Two kinds merge further: plain materials (no map) across metallic and roughness, which move into a
 * small palette texture (glTF's metallic-roughness texture, one texel per pair), and armour plates whose only map is
 * their own baked occlusion, which share one atlas of those maps (compressed blocks copied tile by tile, UVs moved
 * into each tile). Each merged vertex keeps its own joints and weights; a rigid mesh (a plate, a held item, a hair
 * cut on its socket) becomes a joint itself, so it still follows its holder when a bow, dagger or crossbow moves
 * between sockets or a hanging item swings. The merged meshes' skeleton names every source's joints with that
 * source's inverse bind matrices, and their geometry sits in each source's bind space (`bindMatrix` and morph targets
 * baked: faces, age and build are set once per look), so the bind matrix is the identity.
 *
 * Every skin of the unit draws from that one skeleton where it can (its joints lead it in order), so the unit's
 * joint palette is computed and uploaded once a frame rather than once per skinned mesh (`SkeletonUtils.clone` gives
 * each mesh its own).
 *
 * Left as they are: hidden meshes, meshes under `keep` (holders whose visibility toggles, e.g. a throwable), blended
 * materials (they sort per object, back to front) and multi-material meshes. Each merged source is replaced by an
 * empty node with its name and transform, so joints, sockets and holders under it and name lookups stay put.
 * Presentation only.
 */
export function mergeUnitMeshes(model: THREE.Object3D, keep: ReadonlySet<THREE.Object3D>): MergedUnit {
  model.updateMatrixWorld(true);
  const groups = new Map<string, { material: THREE.Material; sources: THREE.Mesh[] }>();
  const skinned: THREE.SkinnedMesh[] = [];
  const visit = (object: THREE.Object3D) => {
    if (!object.visible || keep.has(object)) { return; }
    if (object instanceof THREE.SkinnedMesh) { skinned.push(object); }
    if (object instanceof THREE.Mesh && !(object instanceof THREE.InstancedMesh)) {
      const key = mergeKey(object);
      if (key !== null) {
        let group = groups.get(key);
        if (!group) { groups.set(key, group = { material: object.material as THREE.Material, sources: [] }); }
        group.sources.push(object);
      }
    }
    for (const child of object.children) { visit(child); }
  };
  visit(model);
  // One joint palette for the whole unit, led by its largest skin, so it is computed and uploaded once a frame.
  const bones: THREE.Object3D[] = [];
  const inverses: THREE.Matrix4[] = [];
  const slots = new Map<THREE.Object3D, { inverse: THREE.Matrix4; slot: number }[]>();
  const slot = (bone: THREE.Object3D, inverse: THREE.Matrix4): number => {
    const known = slots.get(bone) ?? [];
    const found = known.find((entry) => entry.inverse.equals(inverse));
    if (found) { return found.slot; }
    bones.push(bone);
    inverses.push(inverse.clone());
    known.push({ inverse, slot: bones.length - 1 });
    slots.set(bone, known);
    return bones.length - 1;
  };
  const lead = skinned.reduce<THREE.SkinnedMesh | null>((best, mesh) => (best && best.skeleton.bones.length >= mesh.skeleton.bones.length ? best : mesh), null);
  lead?.skeleton.bones.forEach((bone, i) => slot(bone, lead.skeleton.boneInverses[i]!));
  const merged: { mesh: THREE.SkinnedMesh; sources: THREE.Mesh[] }[] = [];
  for (const { material, sources } of groups.values()) {
    if (sources.length < 2) { continue; }
    const standard = material instanceof THREE.MeshStandardMaterial ? material : null;
    // Plates each with their own baked occlusion (and no other map) share one tiled occlusion texture.
    const occlusion = standard?.aoMap && tileable(standard)
      ? occlusionAtlas(sources.map((source) => (source.material as THREE.MeshStandardMaterial).aoMap!)) : null;
    // Plain materials (no map) of several metallic and roughness factors read them from a texel each.
    const surfaces = standard && plain(standard) ? surfacePalette(sources.map((source) => source.material as THREE.MeshStandardMaterial)) : null;
    const textured = surfaces !== null || sources.some((source) => source.geometry.getAttribute("uv") !== undefined);
    const tangents = sources.every((source) => source.geometry.getAttribute("tangent") !== undefined);
    const geometries = sources.map((source, i) => {
      const stand = standIn(source);
      const tile = occlusion?.tiles[i];
      const texel = surfaces?.texels[i];
      const uvOf: UvOf | null = tile ? (u, v) => [(tile.column + u) / tile.columns, (tile.row + v) / tile.rows]
        : texel ? () => texel : null;
      if (source instanceof THREE.SkinnedMesh) {
        const map = source.skeleton.bones.map((bone, i) => slot(bone, source.skeleton.boneInverses[i]!));
        return bindGeometry(source, source.bindMatrix, map, null, textured, tangents, uvOf);
      }
      // A rigid mesh rides its stand-in as a joint: bound where it is now, it moves as the stand-in moves.
      const at = slot(stand, source.matrixWorld.clone().invert());
      return bindGeometry(source, source.matrixWorld, null, at, textured, tangents, uvOf);
    });
    const geometry = mergeGeometries(geometries);
    for (const part of geometries) { part.dispose(); }
    if (!geometry) { throw new Error("A unit's meshes failed to merge."); }
    const own = material.clone();
    if ("color" in own && own.color instanceof THREE.Color) { own.color.setRGB(1, 1, 1); }
    if (own instanceof THREE.MeshStandardMaterial) {
      if (occlusion) { own.aoMap = occlusion.texture; }
      if (surfaces) {
        own.metalness = 1;
        own.roughness = 1;
        own.roughnessMap = own.metalnessMap = surfaces.texture;
      }
    }
    own.opacity = 1;
    const mesh = new THREE.SkinnedMesh(geometry, own);
    mesh.name = `merged:${sources.map((source) => source.name).join("+")}`.slice(0, 120);
    merged.push({ mesh, sources });
  }
  const skeleton = new THREE.Skeleton(bones as THREE.Bone[], inverses);
  for (const { mesh } of merged) {
    mesh.bind(skeleton, new THREE.Matrix4());
    model.add(mesh);
  }
  const gone = new Set(merged.flatMap(({ sources }) => sources));
  for (const mesh of skinned) {
    // A skin whose joints lead the palette in order (the body's, an outfit's) draws from it as it is.
    const own = mesh.skeleton;
    if (!gone.has(mesh) && own.bones.every((bone, i) => bones[i] === bone && inverses[i]!.equals(own.boneInverses[i]!))) {
      mesh.skeleton = skeleton;
    }
  }
  return { meshes: merged.map(({ mesh }) => mesh), skeleton };
}

/** A unit's merged meshes and its shared joint palette, which it frees when dropped. */
export type MergedUnit = { meshes: THREE.SkinnedMesh[]; skeleton: THREE.Skeleton };

/** The material class a mesh merges in, or null when it stays its own draw. */
function mergeKey(mesh: THREE.Mesh): string | null {
  const material = mesh.material;
  if (Array.isArray(material) || material.transparent || mesh.geometry.getAttribute("position") === undefined) { return null; }
  const id = (texture: THREE.Texture | null | undefined) => texture?.uuid ?? "-";
  const common = [material.alphaTest, material.side];
  if (material instanceof THREE.MeshBasicMaterial) { return ["basic", id(material.map), ...common].join("|"); }
  if (!(material instanceof THREE.MeshStandardMaterial)) { return null; }
  const { emissive, emissiveIntensity: strength } = material;
  const glow = [emissive.r * strength, emissive.g * strength, emissive.b * strength];
  // Plain materials merge across metallic and roughness, which move into a palette texture.
  if (plain(material)) { return ["plain", ...common, ...glow].join("|"); }
  const occlusion = tileable(material) ? `tile:${tileLayout(material.aoMap!)}` : id(material.aoMap);
  return [
    "standard", id(material.map), ...common, material.metalness, material.roughness,
    id(material.roughnessMap ?? material.metalnessMap), id(material.normalMap), occlusion, ...glow, id(material.emissiveMap),
  ].join("|");
}

/** Whether `material` has no map at all: its surface is its factors. */
function plain(material: THREE.MeshStandardMaterial): boolean {
  return !material.map && !material.normalMap && !material.roughnessMap && !material.metalnessMap && !material.aoMap
    && !material.emissiveMap;
}

/** Maps a source vertex's UV (0, 0 without one) to its merged UV. */
type UvOf = (u: number, v: number) => [number, number];

/** Surface palettes per set of metallic and roughness pairs, shared by every unit with that set (#615). */
const palettes = new Map<string, THREE.DataTexture>();

/**
 * A row of texels holding `materials`' roughness (green) and metallic (blue) factors as glTF's metallic-roughness
 * texture does, exact (32-bit float, nearest), and the texel centre each material's vertices take; null when they
 * all share one pair, which the factors carry as they are.
 */
function surfacePalette(materials: readonly THREE.MeshStandardMaterial[]): { texture: THREE.DataTexture; texels: [number, number][] } | null {
  const pairs = [...new Set(materials.map((material) => `${material.roughness}:${material.metalness}`))];
  if (pairs.length < 2) { return null; }
  const key = pairs.join("+");
  let texture = palettes.get(key);
  if (!texture) {
    const data = new Float32Array(pairs.length * 4);
    pairs.forEach((pair, i) => {
      const [roughness, metalness] = pair.split(":").map(Number);
      data.set([0, roughness!, metalness!, 1], i * 4);
    });
    texture = new THREE.DataTexture(data, pairs.length, 1, THREE.RGBAFormat, THREE.FloatType);
    texture.name = "surface-palette";
    texture.magFilter = texture.minFilter = THREE.NearestFilter;
    texture.colorSpace = THREE.NoColorSpace;
    texture.needsUpdate = true;
    palettes.set(key, texture);
  }
  const texels = materials.map((material): [number, number] => [
    (pairs.indexOf(`${material.roughness}:${material.metalness}`) + 0.5) / pairs.length, 0.5,
  ]);
  return { texture, texels };
}

/** Whether `material`'s occlusion map can share a tiled atlas: its only map, and one whose blocks can be copied. */
function tileable(material: THREE.MeshStandardMaterial): boolean {
  return material.aoMap !== null && tileLayout(material.aoMap) !== null && !material.map && !material.normalMap
    && !material.roughnessMap && !material.metalnessMap && !material.emissiveMap;
}

/**
 * The layout an occlusion atlas tile must share, or null when `texture` cannot tile: a compressed texture still on the
 * CPU, square, with a full mip chain of 4×4 blocks (8 or 16 bytes each, BC/ETC2/ASTC 4×4).
 */
function tileLayout(texture: THREE.Texture): string | null {
  if (!(texture instanceof THREE.CompressedTexture)) { return null; }
  const levels = texture.mipmaps as { data: ArrayBufferView; width: number; height: number }[];
  const top = levels[0];
  if (!top || top.width !== top.height || top.width < 16 || !Number.isInteger(Math.log2(top.width))) { return null; }
  const blocks = (top.width / 4) * (top.height / 4);
  const bytes = top.data.byteLength / blocks;
  if (bytes !== 8 && bytes !== 16) { return null; }
  return [texture.format, texture.type, top.width, levels.length, bytes, texture.colorSpace, texture.channel,
    texture.magFilter, texture.minFilter].join(":");
}

/** One mesh's place in an occlusion atlas: its column and row of `columns` × `rows` equal tiles. */
type Tile = { column: number; row: number; columns: number; rows: number };

/** Atlases built per set of occlusion maps, shared by every unit wearing that set (#615). */
const atlases = new Map<string, THREE.CompressedTexture>();

/**
 * One texture holding `textures` (equal layouts, see {@link tileLayout}) as a grid of tiles, built by copying their
 * compressed blocks row by row, and each texture's tile. Levels stop where a tile is one block wide; the tiles are
 * clamped to their edge only by the UVs inside them (baked occlusion islands keep off the border).
 */
function occlusionAtlas(textures: readonly THREE.Texture[]): { texture: THREE.CompressedTexture; tiles: Tile[] } {
  const distinct = [...new Set(textures)];
  const columns = Math.ceil(Math.sqrt(distinct.length));
  const rows = Math.ceil(distinct.length / columns);
  const tiles = textures.map((texture) => {
    const index = distinct.indexOf(texture);
    return { column: index % columns, row: Math.floor(index / columns), columns, rows };
  });
  if (distinct.length === 1) { return { texture: distinct[0] as THREE.CompressedTexture, tiles }; }
  const key = distinct.map((texture) => texture.uuid).join("+");
  let atlas = atlases.get(key);
  if (!atlas) {
    const sources = distinct as THREE.CompressedTexture[];
    const first = sources[0]!;
    const top = (first.mipmaps as { data: ArrayBufferView; width: number }[])[0]!;
    const bytes = top.data.byteLength / (top.width / 4) ** 2;
    const mipmaps: { data: Uint8Array; width: number; height: number }[] = [];
    for (let level = 0; top.width >> level >= 4; level++) {
      const across = (top.width >> level) / 4;
      const row = across * bytes;
      const data = new Uint8Array(columns * rows * across * across * bytes);
      sources.forEach((source, index) => {
        const mip = (source.mipmaps as { data: ArrayBufferView }[])[level]!.data;
        const from = new Uint8Array(mip.buffer, mip.byteOffset, mip.byteLength);
        const column = index % columns;
        const tileRow = Math.floor(index / columns);
        for (let y = 0; y < across; y++) {
          data.set(from.subarray(y * row, (y + 1) * row), ((tileRow * across + y) * columns * across + column * across) * bytes);
        }
      });
      mipmaps.push({ data, width: columns * across * 4, height: rows * across * 4 });
    }
    atlas = new THREE.CompressedTexture(mipmaps, mipmaps[0]!.width, mipmaps[0]!.height, first.format, first.type);
    atlas.name = "occlusion-atlas";
    atlas.colorSpace = first.colorSpace;
    atlas.channel = first.channel;
    atlas.magFilter = first.magFilter;
    atlas.minFilter = first.minFilter;
    atlas.wrapS = atlas.wrapT = THREE.ClampToEdgeWrapping;
    atlas.generateMipmaps = false;
    atlas.needsUpdate = true;
    atlases.set(key, atlas);
  }
  return { texture: atlas, tiles };
}

/** Replaces `mesh` in its parent by an empty node with its name, transform and children; returns that node. */
function standIn(mesh: THREE.Mesh): THREE.Object3D {
  const stand = new THREE.Object3D();
  stand.name = mesh.name;
  stand.position.copy(mesh.position);
  stand.quaternion.copy(mesh.quaternion);
  stand.scale.copy(mesh.scale);
  stand.matrix.copy(mesh.matrix);
  stand.matrixWorld.copy(mesh.matrixWorld);
  stand.matrixAutoUpdate = mesh.matrixAutoUpdate;
  for (const child of [...mesh.children]) { stand.add(child); }
  const parent = mesh.parent!;
  parent.children[parent.children.indexOf(mesh)] = stand;
  stand.parent = parent;
  mesh.parent = null;
  return stand;
}

const v = new THREE.Vector3();
const n = new THREE.Vector3();
const t = new THREE.Vector4();

/**
 * `mesh`'s geometry in one merge layout: morphs baked at their influences, then positions, normals and tangents
 * through `bind`; the material's base colour and alpha in `color`; UVs (when `textured`) through `uvOf`; joints
 * remapped through `joints` (skinned) or all on the joint `rigid`.
 */
function bindGeometry(mesh: THREE.Mesh, bind: THREE.Matrix4, joints: number[] | null, rigid: number | null,
  textured: boolean, tangents: boolean, uvOf: UvOf | null): THREE.BufferGeometry {
  const source = mesh.geometry;
  const position = source.getAttribute("position");
  const normal = source.getAttribute("normal");
  const uv = source.getAttribute("uv");
  const tangent = source.getAttribute("tangent");
  const color = source.getAttribute("color");
  const skinIndex = source.getAttribute("skinIndex");
  const skinWeight = source.getAttribute("skinWeight");
  const count = position.count;
  const positions = new Float32Array(count * 3);
  const normals = new Float32Array(count * 3);
  for (let i = 0; i < count; i++) {
    positions.set([position.getX(i), position.getY(i), position.getZ(i)], i * 3);
    if (normal) { normals.set([normal.getX(i), normal.getY(i), normal.getZ(i)], i * 3); }
  }
  // Faces, age and build (#411, #380) are set once per look: bake them at their weights.
  const influences = mesh.morphTargetInfluences ?? [];
  const relative = source.morphTargetsRelative;
  for (const [name, into] of [["position", positions], ["normal", normals]] as const) {
    const targets = source.morphAttributes[name] ?? [];
    targets.forEach((target, k) => {
      const weight = influences[k] ?? 0;
      if (weight === 0) { return; }
      const base = name === "position" ? position : normal;
      for (let i = 0; i < count; i++) {
        for (let c = 0; c < 3; c++) {
          const value = target.getComponent(i, c);
          into[i * 3 + c]! += weight * (relative ? value : value - (base?.getComponent(i, c) ?? 0));
        }
      }
    });
  }
  const normalMatrix = new THREE.Matrix3().getNormalMatrix(bind);
  for (let i = 0; i < count; i++) {
    v.fromArray(positions, i * 3).applyMatrix4(bind).toArray(positions, i * 3);
    n.fromArray(normals, i * 3).applyMatrix3(normalMatrix).normalize().toArray(normals, i * 3);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(normals, 3));
  if (textured) {
    const uvs = new Float32Array(count * 2);
    for (let i = 0; i < count; i++) {
      const [u, v] = uv ? [uv.getX(i), uv.getY(i)] : [0, 0];
      uvs.set(uvOf ? uvOf(u, v) : [u, v], i * 2);
    }
    geometry.setAttribute("uv", new THREE.BufferAttribute(uvs, 2));
  }
  if (tangents && tangent) {
    const out = new Float32Array(count * 4);
    for (let i = 0; i < count; i++) {
      t.fromBufferAttribute(tangent as THREE.BufferAttribute, i);
      v.set(t.x, t.y, t.z).transformDirection(bind);
      out.set([v.x, v.y, v.z, t.w], i * 4);
    }
    geometry.setAttribute("tangent", new THREE.BufferAttribute(out, 4));
  }
  const material = mesh.material as THREE.MeshStandardMaterial;
  const base = material.color ?? new THREE.Color(1, 1, 1);
  const alpha = material.opacity ?? 1;
  const colors = new Float32Array(count * 4);
  for (let i = 0; i < count; i++) {
    const [r, g, b, a] = color
      ? [color.getX(i), color.getY(i), color.getZ(i), color.itemSize === 4 ? color.getW(i) : 1] : [1, 1, 1, 1];
    colors.set([r * base.r, g * base.g, b * base.b, a * alpha], i * 4);
  }
  geometry.setAttribute("color", new THREE.BufferAttribute(colors, 4));
  const indices = new Uint16Array(count * 4);
  const weights = new Float32Array(count * 4);
  for (let i = 0; i < count; i++) {
    if (joints && skinIndex && skinWeight) {
      for (let c = 0; c < 4; c++) {
        weights[i * 4 + c] = skinWeight.getComponent(i, c);
        indices[i * 4 + c] = joints[skinIndex.getComponent(i, c)] ?? 0;
      }
    } else {
      indices[i * 4] = rigid ?? 0;
      weights[i * 4] = 1;
    }
  }
  geometry.setAttribute("skinIndex", new THREE.BufferAttribute(indices, 4));
  geometry.setAttribute("skinWeight", new THREE.BufferAttribute(weights, 4));
  const index = source.index;
  const order = index ? Array.from({ length: index.count }, (_, i) => index.getX(i)) : Array.from({ length: count }, (_, i) => i);
  // A mirrored bind (negative determinant) flips the winding; keep the faces facing out.
  if (bind.determinant() < 0) {
    for (let i = 0; i + 2 < order.length; i += 3) { [order[i + 1], order[i + 2]] = [order[i + 2]!, order[i + 1]!]; }
  }
  geometry.setIndex(order);
  return geometry;
}
