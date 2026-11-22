import type { IndexedMeshGeometry, Matrix4Values, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { EntityKind, EntityRef, EntityState, IngestedProjection, StrikeAim } from "../replication";
import type { Color } from "./scenery";
import { freeHandle } from "./zone-lifecycle";

/**
 * The shared unit presentation (#184): `mmorpg-presentation` draws procedural players,
 * creatures and NPCs, their locomotion and action poses, spell visuals, health bars and
 * target rings, and picks each imported model's clip, exactly as the native client does.
 * This adapter feeds it the received projections and the sampled units, turns its draws into
 * renderer nodes and keeps the geometry of resources still in use.
 */
export type UnitPresenterHandle = {
  modelReady(kind: string, clips: string): void;
  modelMotions(kind: string, descriptor: string): void;
  residentsReady(): void;
  setBlend(lowerSeconds: number, upperSeconds: number, lowerWeight: number, upperWeight: number): void;
  setBodyOverlay(shown: boolean): void;
  setInteraction(lootWindow: boolean, npc: number | undefined): void;
  setKitPlatforms(platforms: Uint32Array): void;
  setEffectKits(drawn: boolean): void;
  setCragDescriptor(json: string): void;
  setChannelOrigins(origins: Float64Array): void;
  /** Observes the session's projection `id` from its store (#1009); false once the store no longer retains it. */
  push(store: IngestedProjection["store"], id: number, seconds: number): boolean;
  reset(): void;
  prepare(ability: number, seconds: number): void;
  aimPreparation(aim: StrikeAim): void;
  preparedSent(seconds: number): void;
  abandonPreparation(): void;
  /** Sound cues and fades of the projections pushed since the last call (#447), as JSON. */
  takeAudio(): string;
  /** Lays one frame out in WASM memory (#1057): read through {@link directory} and {@link memory}. */
  frame(units: Float64Array, seconds: number, dt: number, reducedMotion: boolean, barYaw: number,
    choice: number, headwear: string, previewYaw: number): void;
  /** The address of the layout's directory: pointer, capacity and length of its words, numbers, models, vertices and indices. */
  directory(): number;
  memory(): WebAssembly.Memory;
  /** The last frame's newly interned strings, each followed by a newline. */
  freshStrings(): string;
};

export type ImportedModelKind = "Ranger" | "Wolf" | "Rat" | "Boar" | "Bear" | "Spider" | "Lurker" | "Lynx" | "Adder" | "Bat" | "Crayfish" | "Stag";
/** A shared presenter's `Loadout::name`; its socket items are `Loadout::attachments` (#1012). */
export type Loadout = string;
/** A drawn throwable in the right hand (`imported::Throwable`, #603). */
export type Throwable = "knife" | "flask" | "bomb";
/**
 * A crossbowman's crossbow (`imported::CrossbowCarry`, #607): in the left hand through the reload's
 * hand-off, the left hand on the crank, and the bolt in `hand_r.arrow`, in the groove or gone.
 */
export type CrossbowCarry = { leftHand: boolean; crank: boolean; bolt: "hand" | "groove" | null };
/** A resident's chore item on its routine (`imported::Carry::chore`, #904): its `humanoid-equipment` key and socket. */
export type ChoreCarry = { item: string; socket: string };
/**
 * A class starter outfit (asset-tooling #379, the shared `Outfit`): the `class.<id>` appearance
 * preset and the armour sockets whose outfit pieces the viewer's equipped drops replace.
 */
export type ClassOutfit = { preset: string; replaced: readonly string[] };

/** Where a presented unit stands, in metres and radians. */
export type PresentedUnit = {
  entity: EntityRef;
  /** The sampled unit it presents; null for one the presenter still shows without a sample. */
  state: EntityState | null;
  /** `unit-<kind>-<id>`, the prefix of its node IDs. */
  identity: string;
  feet: readonly [number, number, number];
  centreY: number;
  yaw: number;
  /** Nameplate anchor above its health bar. */
  label: readonly [number, number, number];
  /** Its procedural body nodes (empty for an imported model). */
  body: readonly RendererSceneNode[];
};

/** An imported model to skin this frame, with the clip and time the shared presenter chose. */
export type ImportedRequest = {
  id: string;
  kind: ImportedModelKind;
  entity: EntityRef;
  feet: readonly [number, number, number];
  yaw: number;
  /** The swim lean (Atlas 6.6.41) in radians about the turned model's X axis (Euler order `YXZ`); 0 upright. */
  pitch: number;
  heightMetres: number;
  /** The upper body's target clip and time: what the unit is seen doing. */
  clip: string;
  time: number;
  /** The legs' target clip and time. */
  lower: readonly [string, number];
  /** The layered clips to pose (Atlas 6.6.45). */
  layers: readonly ClipLayer[];
  /** Linear RGB colour factor: corpse darkening and the companion's green (#296); null as authored. */
  tint: readonly [number, number, number] | null;
  /** What the unit carries: a player its class weapons (asset-tooling #375), a guard its sword. */
  loadout: Loadout | null;
  /** A named resident's `humanoid-appearance` preset id (#418). */
  resident: string | null;
  /** The Ranger figure a resident is drawn on, by its character key (`imported::Figure`, #493). */
  figure: string | null;
  /** A varied resident's look seed (its NPC id; asset-tooling #380): each guard is his own man. */
  variety: number | null;
  /** A player's class starter outfit (asset-tooling #379). */
  outfit: ClassOutfit | null;
  /** The living viewer: its imported model wears the viewer's chosen headwear (#374). */
  headwear: boolean;
  /**
   * How far its loose gear swings (Atlas 6.6.39): a model-frame quaternion `[x, y, z, w]` for the
   * items on hanging sockets, or null where they hang as authored.
   */
  swing: readonly [number, number, number, number] | null;
  /**
   * The spring's drive of the bundle's accessory-bone chains (Atlas 6.6.39, `Swing::chain`): `[pitch, roll]`
   * by depth from each chain's root as a share of its bound, or null where they hang as authored.
   */
  chains: readonly number[] | null;
  /**
   * A bow carrier's shot (#556): the bow cycle clip and time whose `bow.drawMeters` the nock
   * follows, with the bow in the left hand (`hand_l.bow`); null keeps it on `back.bow.stowed`.
   */
  bow: BowShot | null;
  /**
   * A dagger holder's carry (`imported::Carry`, #603): each hand's dagger sheathed (left on
   * `hip_l.stowed`, right on `hip_r.stowed`) and the throwable drawn into `hand_r.weapon`; before
   * the throw's draw its item hangs on `belt.throwable` (`belted`, #611).
   */
  sheathed: readonly [boolean, boolean];
  /** Explicit player mode; null for non-player animation-driven carry. */
  weaponDrawn: boolean | null;
  throwable: Throwable | null;
  belted: Throwable | null;
  /** A crossbowman's crossbow (#607); null for anyone else. */
  crossbow: CrossbowCarry | null;
  /**
   * Where the spell of the caster clip shown starts (`imported::Channel`, #609): `point` metres in
   * the `joint`'s glTF frame, the palm or the staff's head; null outside the caster clips. Its posed
   * world position goes back through {@link SharedUnits.setChannelOrigins}, where beams start (#612).
   */
  channel: SpellChannel | null;
  /** The chore item a resident holds on its routine (#904); null or absent for empty hands. */
  chore?: ChoreCarry | null;
};

/** A caster clip's `cast.channelJoint` and `cast.channelPoint` (asset-tooling #420). */
export type SpellChannel = { joint: "hand_l" | "hand_r"; point: readonly [number, number, number] };

/** The bow cycle clip a unit's upper body shows and its time (`imported::BowShot`, #556). */
export type BowShot = { clip: string; time: number };

/** Which joints a layer drives: every joint, the `spine_01` subtree, the rest (pelvis, legs), or one hand's fingers (a grip, asset-tooling #285). */
export type LayerMask = "all" | "upper" | "lower" | "fingers_l" | "fingers_r";
/** One weighted clip sample; a joint averages the layers that drive it, the rest of a total below 1 stays at rest. */
export type ClipLayer = { clip: string; time: number; weight: number; mask: LayerMask };

/** How the body regions blend (the Action Lab, #384): crossfade seconds and overlay weights, each 0–1. */
export type AnimationBlend = { lowerSeconds: number; upperSeconds: number; lowerWeight: number; upperWeight: number };
export const DEFAULT_BLEND: AnimationBlend = { lowerSeconds: 0.15, upperSeconds: 0.1, lowerWeight: 1, upperWeight: 1 };
/** The viewer's layered animation (#384): each region's target clip and time, and the posed layers. */
export type ViewerAnimation = { upper: readonly [string, number]; lower: readonly [string, number]; layers: readonly ClipLayer[] };

/** A platform a kit piece draws (#591): its box's bottom centre this frame, metres. */
export type PlatformBase = { platform: number; base: [number, number, number] };

/**
 * A strike or spell effect to play from its bundle (#894): the entry `key` under the column-major `model` (metres), its
 * `clip` at `time` seconds (a `Loop` clip wraps), recoloured by `tint` (0–1 sRGB channels) when set.
 */
export type PresentedEffect = { key: string; model: readonly number[]; clip: string | null; time: number; tint: readonly [number, number, number] | null };

/**
 * One frame's presentation. Its nodes, units and their arrays are the presenter's own, updated in place by the next
 * frame (#1057); the imported requests, platforms and effects are new each frame.
 */
export type UnitFrameNodes = {
  /** The units' procedural draws, without effects. */
  nodes: readonly RendererSceneNode[];
  /** The presenter's spell and strike effect draws (`fx-` ids). */
  effectNodes: readonly RendererSceneNode[];
  units: readonly PresentedUnit[]; imported: ImportedRequest[]; platforms: PlatformBase[]; effects: PresentedEffect[];
  /** The viewer's camera kick from a blow it struck or took (#900), metres; zero under reduced motion. */
  kick: readonly [number, number, number];
};

/** The viewer's own look: class and sex as an appearance code, and headwear. */
export type ViewerLook = {
  classId: "warden" | "ranger" | "arcanist" | "necromancer" | "rogue" | "cleric" | "druid" | null; sex: "female" | "male"; headwear: string | null;
  /** The viewer's hairstyle and colour (#376); hidden under the hood and helm. */
  hair?: "buzzed" | "parted" | "long" | "bald"; hairColor?: "black" | "brown" | "blond";
  /** The viewer's face (#411): bundle face feature id to option id. */
  face?: Readonly<Record<string, string>>;
  /** The viewer's physique (#635): slider key to its value in [0, 1]; absent draws the average body. */
  physique?: Readonly<Record<string, number>>;
};

/**
 * The frame layout `mmorpg-wasm` writes (#1057, `crates/mmorpg-wasm/src/unit_layout.rs`, which documents its records):
 * header words, then draw, geometry, unit, imported, platform and effect records read with one cursor into `words`
 * and one into `numbers`. Strings are interned: a record holds an index into the strings sent so far.
 */
const NONE = 0xffff_ffff;
const HEADER = { stringsReset: 0, freshStrings: 1, draws: 2, geometries: 3, units: 4, imported: 5, platforms: 6, effects: 7, words: 8 } as const;
const DRAW_UNLIT = 1;
const DRAW_DOUBLE_SIDED = 1 << 1;
const IMPORTED = {
  headwear: 1, sheathedLeft: 1 << 1, sheathedRight: 1 << 2, weaponMode: 1 << 3, weaponDrawn: 1 << 4, tint: 1 << 5, swing: 1 << 6,
  chains: 1 << 7, bow: 1 << 8, crossbow: 1 << 9, crossbowLeftHand: 1 << 10, crossbowCrank: 1 << 11, channel: 1 << 12,
} as const;
/** The directory's buffers, each `[pointer, capacity, length]`. */
const SLOT = { words: 0, numbers: 1, models: 2, vertices: 3, indices: 4 } as const;

/** A renderer node the presenter keeps per draw id and updates in place; absent options stay `undefined`. */
type UnitNode = {
  id: string; name: string; userData: Record<string, unknown> | undefined; geometry: IndexedMeshGeometry; color: Color; modelMatrix: Matrix4Values;
  unlit: true | undefined; emissive: Color | undefined; opacity: number | undefined; doubleSided: true | undefined;
};
type MutableUnit = Omit<PresentedUnit, "feet" | "label" | "body"> & { feet: [number, number, number]; label: [number, number, number]; body: RendererSceneNode[] };

const KIND_CODE: Readonly<Record<EntityKind, number>> = { player: 0, creature: 1, npc: 2, minion: 3 };
const KINDS: readonly EntityKind[] = ["player", "creature", "npc", "minion"];
/** A unit's number key: its kind code above its 32-bit id. */
function entityNumber(kindCode: number, id: number): number {
  return kindCode * 0x1_0000_0000 + id;
}

/** Health bars cast no shadow; breakable pieces, their hoops and structure pieces only into the two near sun cascades. */
const NO_SHADOW = { notShadowCaster: true };
const NEAR_SHADOW = { shadowCascades: 2 };
function shadowUserData(name: string): Record<string, unknown> | undefined {
  if (name.endsWith("-health")) { return NO_SHADOW; }
  if (name.endsWith("-piece") || name.endsWith("-hoop") || name === "structure") { return NEAR_SHADOW; }
  return undefined;
}
const CLASS_CODE = { warden: 0, ranger: 1, arcanist: 2, necromancer: 3, rogue: 4, cleric: 5, druid: 6 } as const;

function triples(values: Float32Array, start: number, count: number): [number, number, number][] {
  const out: [number, number, number][] = new Array(count);
  for (let index = 0; index < count; index += 1) {
    const at = start + index * 3;
    out[index] = [values[at]!, values[at + 1]!, values[at + 2]!];
  }
  return out;
}

/** The appearance code `1 + class × 2 + sex` the shared presenter reads, 0 for no class. */
export function appearanceCode(look: Pick<ViewerLook, "classId" | "sex">): number {
  return look.classId === null ? 0 : 1 + CLASS_CODE[look.classId] * 2 + (look.sex === "male" ? 1 : 0);
}

export class SharedUnits {
  readonly #handle: UnitPresenterHandle;
  /** The strings the presenter interned, by index (#1057), and what each names once derived. */
  #strings: string[] = [];
  #nodeNames: (string | undefined)[] = [];
  #colors = new Map<number, Color>();
  #outfits = new Map<number, ClassOutfit>();
  /** Geometry by resource-key index: those drawn in the previous frame, which the presenter will not resend. */
  #geometries: (IndexedMeshGeometry | undefined)[] = [];
  #geometryFrame: number[] = [];
  #heldKeys: number[] = [];
  #drawnKeys: number[] = [];
  /** Renderer nodes and presented units by draw-id and node index, kept and updated in place with the frame each was last seen in. */
  #nodes = new Map<number, { node: UnitNode; frame: number }>();
  #units = new Map<number, { unit: MutableUnit; frame: number }>();
  /** This frame's sampled states and presented units by {@link entityNumber}. */
  #sampled = new Map<number, EntityState>();
  #byEntity = new Map<number, PresentedUnit>();
  #frameNumber = 0;
  #input = new Float64Array(0);
  #draws: RendererSceneNode[] = [];
  #frame: UnitFrameNodes & { nodes: RendererSceneNode[]; effectNodes: RendererSceneNode[]; units: PresentedUnit[]; kick: [number, number, number] } = {
    nodes: [], effectNodes: [], units: [], imported: [], platforms: [], effects: [], kick: [0, 0, 0],
  };
  /** Views over the layout's buffers in WASM memory, rebuilt when memory grows or a buffer moves. */
  #memory: WebAssembly.Memory | null = null;
  #directory = new Uint32Array(0);
  #placed = new Float64Array(10);
  #words = new Uint32Array(0);
  #numbers = new Float64Array(0);
  #models = new Float32Array(0);
  #vertices = new Float32Array(0);
  #indices = new Uint32Array(0);
  readonly #sweepNode = (entry: { frame: number }, id: number): void => { if (entry.frame !== this.#frameNumber) { this.#nodes.delete(id); } };
  readonly #sweepUnit = (entry: { frame: number }, id: number): void => { if (entry.frame !== this.#frameNumber) { this.#units.delete(id); } };

  constructor(handle: UnitPresenterHandle) {
    this.#handle = handle;
  }

  /** Frees the presenter's WASM handle (#1005); the presenter is not used afterwards. */
  dispose(): void {
    freeHandle(this.#handle);
  }

  /**
   * An imported model finished loading, with its clip lengths in seconds and, for the Ranger, its
   * `humanoid-clips` descriptor JSON: the traversal clips' speeds and authored root paths (#500).
   */
  modelReady(kind: ImportedModelKind, clips: readonly (readonly [string, number])[], descriptor: string | null = null): void {
    this.#handle.modelReady(kind, JSON.stringify(clips));
    if (descriptor !== null) { this.#handle.modelMotions(kind, descriptor); }
  }

  /** The residents' appearance presets loaded (#418): named residents become Rangers. */
  residentsReady(): void {
    this.#handle.residentsReady();
  }

  /** Tunes the layered blend; the presenter clamps it. Presentation only. */
  setBlend(blend: AnimationBlend): void {
    this.#handle.setBlend(blend.lowerSeconds, blend.upperSeconds, blend.lowerWeight, blend.upperWeight);
  }

  /** Shows or hides the development body overlay (#39): each unit's authoritative body over its model. */
  setBodyOverlay(shown: boolean): void {
    this.#handle.setBodyOverlay(shown);
  }

  /** What the page's windows are open toward this frame (#619): the viewer's loot kneel and the NPC it talks to. */
  setInteraction(lootWindow: boolean, npc: number | null): void {
    this.#handle.setInteraction(lootWindow, npc ?? undefined);
  }

  /** The crag kit's descriptor once it is drawn (#668): climbers on its climbable faces are drawn out to its rock. */
  setCragDescriptor(json: string): void {
    this.#handle.setCragDescriptor(json);
  }

  /** The platforms a kit draws (the fortress lifts, #591): no procedural pillar; each frame's `platforms` places them. */
  setKitPlatforms(platforms: readonly number[]): void {
    this.#handle.setKitPlatforms(Uint32Array.from(platforms));
  }

  /** Whether the page draws the strike and spell effect bundles (#894): each frame's `effects` then lists what to play. */
  setEffectKits(drawn: boolean): void {
    this.#handle.setEffectKits(drawn);
  }

  /**
   * Where each caster's spell starts in the world this frame, metres: its `channel` point on the posed
   * hand (#612). The next frames' beams, drains and chains start there; a unit left out uses its chest.
   */
  setChannelOrigins(origins: readonly { entity: EntityRef; point: readonly [number, number, number] }[]): void {
    const flat = new Float64Array(origins.length * 5);
    origins.forEach(({ entity, point }, index) => { flat.set([KIND_CODE[entity.kind], entity.id, ...point], index * 5); });
    this.#handle.setChannelOrigins(flat);
  }

  /** Observes a projection a session ingested (#1009), decoded once in its store. */
  push(projection: IngestedProjection, seconds: number): boolean {
    return this.#handle.push(projection.store, projection.id, seconds);
  }

  /** Drops every per-unit state; the next frame's layout sends every string and geometry again. */
  reset(): void {
    this.#handle.reset();
    this.#forget();
  }

  /** The unit presented for `entity` in the last frame, if any. */
  unitOf(entity: EntityRef): PresentedUnit | undefined {
    return this.#byEntity.get(entityNumber(KIND_CODE[entity.kind], entity.id));
  }

  /**
   * The local player pressed or began aiming an attack (Atlas 6.6.10): the viewer winds up at
   * once and holds short of the swing until the zone accepts (the swing continues) or refuses
   * (the wind-up fades). Presentation only; the zone still decides every outcome.
   */
  prepare(ability: number, seconds: number): void {
    this.#handle.prepare(ability, seconds);
  }

  /** The directed strike being prepared aims at `aim` (#154): its wind-up shows that slash (#582). */
  aimPreparation(aim: StrikeAim): void {
    this.#handle.aimPreparation(aim);
  }

  /** The prepared input went to the zone; an unanswered wind-up lets go shortly after. */
  preparedSent(seconds: number): void {
    this.#handle.preparedSent(seconds);
  }

  /** Blur, menus, a retired input or a superseding gesture: no swing. */
  abandonPreparation(): void {
    this.#handle.abandonPreparation();
  }

  /** The shared presenter's sound operations since the last call, for `WorldSound.apply`. */
  takeAudio(): string {
    return this.#handle.takeAudio();
  }

  /**
   * One frame of the shared presenter (#1057): it lays the frame out in WASM memory and this reads it in place, keeping
   * its renderer nodes and units by draw id and updating them without parsing, matrix copies or new node objects.
   */
  frame(units: readonly EntityState[], options: { seconds: number; dt: number; reducedMotion: boolean; barYaw: number;
    look: ViewerLook; previewYaw?: number }): UnitFrameNodes {
    if (this.#input.length < units.length * 6) { this.#input = new Float64Array(units.length * 12); }
    const input = this.#input;
    this.#sampled.clear();
    for (let index = 0; index < units.length; index += 1) {
      const entity = units[index]!;
      const at = index * 6;
      const kind = KIND_CODE[entity.kind];
      input[at] = kind; input[at + 1] = entity.entityId;
      input[at + 2] = entity.position[0]; input[at + 3] = entity.position[1]; input[at + 4] = entity.position[2];
      input[at + 5] = entity.facing;
      this.#sampled.set(entityNumber(kind, entity.entityId), entity);
    }
    this.#handle.frame(input.subarray(0, units.length * 6), options.seconds, options.dt, options.reducedMotion, options.barYaw,
      appearanceCode(options.look), options.look.headwear ?? "", options.previewYaw ?? Number.NaN);
    this.#frameNumber += 1;
    this.#view();
    if (this.#words[HEADER.stringsReset] === 1) { this.#forget(); }
    if (this.#words[HEADER.freshStrings]! > 0) {
      const fresh = this.#handle.freshStrings().split("\n");
      for (let index = 0; index < fresh.length - 1; index += 1) { this.#strings.push(fresh[index]!); }
      // Returning the strings may have grown WASM memory, detaching the views.
      this.#view();
    }
    const words = this.#words;
    const numbers = this.#numbers;
    const frame = this.#frame;
    const cursor = { word: HEADER.words, number: 3 };
    frame.kick[0] = numbers[0]!; frame.kick[1] = numbers[1]!; frame.kick[2] = numbers[2]!;
    this.#readDraws(words[HEADER.draws]!, words[HEADER.geometries]!, cursor);
    this.#readUnits(words[HEADER.units]!, cursor);
    frame.imported = this.#readImported(words[HEADER.imported]!, cursor);
    frame.platforms = [];
    for (let index = 0; index < words[HEADER.platforms]!; index += 1) {
      const at = cursor.number;
      frame.platforms.push({ platform: words[cursor.word]!, base: [numbers[at]!, numbers[at + 1]!, numbers[at + 2]!] });
      cursor.word += 1; cursor.number += 3;
    }
    frame.effects = [];
    for (let index = 0; index < words[HEADER.effects]!; index += 1) {
      const at = cursor.number;
      const clip = words[cursor.word + 1]!;
      frame.effects.push({
        key: this.#strings[words[cursor.word]!]!, clip: clip === NONE ? null : this.#strings[clip]!, time: numbers[at]!,
        tint: Number.isNaN(numbers[at + 1]!) ? null : [numbers[at + 1]!, numbers[at + 2]!, numbers[at + 3]!],
        model: Array.from(numbers.subarray(at + 4, at + 20)),
      });
      cursor.word += 2; cursor.number += 20;
    }
    this.#nodes.forEach(this.#sweepNode);
    this.#units.forEach(this.#sweepUnit);
    return frame;
  }

  /** Points the views at the layout's buffers, rebuilding one only when memory grew or the buffer moved. */
  #view(): void {
    const memory = this.#memory ??= this.#handle.memory();
    const buffer = memory.buffer;
    if (this.#directory.buffer !== buffer) {
      this.#directory = new Uint32Array(buffer, this.#handle.directory(), 15);
      this.#placed.fill(-1);
    }
    const directory = this.#directory;
    const moved = (slot: number): boolean => {
      const pointer = directory[slot * 3]!;
      const capacity = directory[slot * 3 + 1]!;
      if (this.#placed[slot * 2] === pointer && this.#placed[slot * 2 + 1] === capacity) { return false; }
      this.#placed[slot * 2] = pointer;
      this.#placed[slot * 2 + 1] = capacity;
      return true;
    };
    const view = <T>(slot: number, make: (pointer: number, capacity: number) => T): T | null =>
      moved(slot) ? make(directory[slot * 3]!, directory[slot * 3 + 1]!) : null;
    this.#words = view(SLOT.words, (p, c) => new Uint32Array(buffer, p, c)) ?? this.#words;
    this.#numbers = view(SLOT.numbers, (p, c) => new Float64Array(buffer, p, c)) ?? this.#numbers;
    this.#models = view(SLOT.models, (p, c) => new Float32Array(buffer, p, c)) ?? this.#models;
    this.#vertices = view(SLOT.vertices, (p, c) => new Float32Array(buffer, p, c)) ?? this.#vertices;
    this.#indices = view(SLOT.indices, (p, c) => new Uint32Array(buffer, p, c)) ?? this.#indices;
  }

  /** Drops every interned string and what is kept by one. */
  #forget(): void {
    this.#strings = [];
    this.#nodeNames = [];
    this.#outfits.clear();
    this.#geometries = [];
    this.#geometryFrame = [];
    this.#heldKeys.length = 0;
    this.#drawnKeys.length = 0;
    this.#nodes.clear();
    this.#units.clear();
  }

  /** The draw records and the new geometries behind them, onto the kept nodes. */
  #readDraws(draws: number, geometries: number, cursor: { word: number; number: number }): void {
    const words = this.#words;
    const numbers = this.#numbers;
    const strings = this.#strings;
    let at = cursor.word + draws * 5;
    for (let index = 0; index < geometries; index += 1, at += 6) {
      const key = words[at]!;
      const start = words[at + 1]!;
      const count = words[at + 2]!;
      const indices = new Array<number>(words[at + 5]!);
      for (let offset = 0; offset < indices.length; offset += 1) { indices[offset] = this.#indices[words[at + 4]! + offset]!; }
      const geometry: IndexedMeshGeometry = {
        kind: "mesh", resourceKey: strings[key]!,
        positions: triples(this.#vertices, start, count),
        normals: triples(this.#vertices, start + count * 3, count),
        indices,
      };
      this.#geometries[key] = words[at + 3] === 1 ? { ...geometry, colors: triples(this.#vertices, start + count * 6, count) } : geometry;
    }
    const all = this.#draws;
    const { nodes, effectNodes } = this.#frame;
    all.length = 0; nodes.length = 0; effectNodes.length = 0;
    const drawn = this.#drawnKeys;
    drawn.length = 0;
    for (let index = 0; index < draws; index += 1) {
      const word = cursor.word + index * 5;
      const id = words[word]!;
      const key = words[word + 1]!;
      const geometry = this.#geometries[key];
      if (!geometry) {
        throw new Error(`The shared unit presenter drew ${strings[key]} without its geometry.`);
      }
      if (this.#geometryFrame[key] !== this.#frameNumber) {
        this.#geometryFrame[key] = this.#frameNumber;
        drawn.push(key);
      }
      let entry = this.#nodes.get(id);
      if (!entry) {
        const name = this.#nodeName(id);
        entry = {
          node: {
            id: strings[id]!, name, userData: shadowUserData(name), geometry, color: "#000000", modelMatrix: new Array<number>(16).fill(0) as Matrix4Values,
            unlit: undefined, emissive: undefined, opacity: undefined, doubleSided: undefined,
          },
          frame: 0,
        };
        this.#nodes.set(id, entry);
      }
      entry.frame = this.#frameNumber;
      const node = entry.node;
      node.geometry = geometry;
      node.color = this.#color(words[word + 2]!);
      const emissive = words[word + 3]!;
      const flags = words[word + 4]!;
      node.emissive = emissive === NONE ? undefined : this.#color(emissive);
      node.unlit = emissive === NONE && (flags & DRAW_UNLIT) !== 0 ? true : undefined;
      node.doubleSided = (flags & DRAW_DOUBLE_SIDED) !== 0 ? true : undefined;
      const opacity = numbers[cursor.number + index]!;
      node.opacity = opacity < 1 ? opacity : undefined;
      const matrix = node.modelMatrix;
      const models = this.#models;
      for (let offset = 0, from = index * 16; offset < 16; offset += 1) { matrix[offset] = models[from + offset]!; }
      const rendered = node as unknown as RendererSceneNode;
      all.push(rendered);
      (node.id.startsWith("fx-") ? effectNodes : nodes).push(rendered);
    }
    // The presenter resends what was not drawn in the previous frame, so only this frame's geometry is kept.
    for (const key of this.#heldKeys) {
      if (this.#geometryFrame[key] !== this.#frameNumber) { this.#geometries[key] = undefined; }
    }
    this.#drawnKeys = this.#heldKeys;
    this.#heldKeys = drawn;
    cursor.word = at;
    cursor.number += draws;
  }

  #readUnits(count: number, cursor: { word: number; number: number }): void {
    const words = this.#words;
    const numbers = this.#numbers;
    const units = this.#frame.units;
    units.length = 0;
    this.#byEntity.clear();
    for (let index = 0; index < count; index += 1, cursor.word += 5, cursor.number += 8) {
      const word = cursor.word;
      const at = cursor.number;
      const kind = words[word]!;
      const id = words[word + 1]!;
      const nodeId = words[word + 2]!;
      const state = this.#sampled.get(entityNumber(kind, id)) ?? null;
      let entry = this.#units.get(nodeId);
      if (!entry) {
        entry = {
          unit: { entity: { kind: KINDS[kind]!, id }, state, identity: this.#strings[nodeId]!, feet: [0, 0, 0], centreY: 0, yaw: 0, label: [0, 0, 0], body: [] },
          frame: 0,
        };
        this.#units.set(nodeId, entry);
      }
      entry.frame = this.#frameNumber;
      const unit = entry.unit;
      unit.state = state;
      unit.feet[0] = numbers[at]!; unit.feet[1] = numbers[at + 1]!; unit.feet[2] = numbers[at + 2]!;
      unit.centreY = numbers[at + 3]!;
      unit.yaw = numbers[at + 4]!;
      unit.label[0] = numbers[at + 5]!; unit.label[1] = numbers[at + 6]!; unit.label[2] = numbers[at + 7]!;
      unit.body.length = 0;
      for (let draw = words[word + 3]!; draw < words[word + 4]!; draw += 1) { unit.body.push(this.#draws[draw]!); }
      units.push(unit);
      this.#byEntity.set(entityNumber(kind, id), unit);
    }
  }

  /** The imported model requests, new objects each frame: the model kit keeps their tint and layers by reference. */
  #readImported(count: number, cursor: { word: number; number: number }): ImportedRequest[] {
    const words = this.#words;
    const numbers = this.#numbers;
    const strings = this.#strings;
    const text = (word: number): string | null => (words[word] === NONE ? null : strings[words[word]!]!);
    const requests: ImportedRequest[] = [];
    for (let index = 0; index < count; index += 1) {
      const w = cursor.word;
      let n = cursor.number;
      const flags = words[w + 4]!;
      const has = (bit: number): boolean => (flags & bit) !== 0;
      const layerCount = words[w + 19]!;
      const layers: ClipLayer[] = [];
      const feet: [number, number, number] = [numbers[n]!, numbers[n + 1]!, numbers[n + 2]!];
      const yaw = numbers[n + 3]!; const pitch = numbers[n + 4]!; const heightMetres = numbers[n + 5]!;
      const time = numbers[n + 6]!; const lowerTime = numbers[n + 7]!;
      n += 8;
      for (let layer = 0; layer < layerCount; layer += 1, n += 2) {
        const word = w + 20 + layer * 2;
        layers.push({ clip: strings[words[word]!]!, mask: strings[words[word + 1]!]! as LayerMask, time: numbers[n]!, weight: numbers[n + 1]! });
      }
      const take = (length: number): number[] => { const values = Array.from(numbers.subarray(n, n + length)); n += length; return values; };
      const tint = has(IMPORTED.tint) ? take(3) as [number, number, number] : null;
      const swing = has(IMPORTED.swing) ? take(4) as [number, number, number, number] : null;
      const chains = has(IMPORTED.chains) ? take(6) : null;
      const bowTime = has(IMPORTED.bow) ? take(1)[0]! : 0;
      const channelPoint = has(IMPORTED.channel) ? take(3) as [number, number, number] : null;
      const variety = words[w + 10]!;
      const chore = text(w + 17);
      requests.push({
        id: strings[words[w]!]!, kind: strings[words[w + 1]!]! as ImportedModelKind, entity: { kind: KINDS[words[w + 2]!]!, id: words[w + 3]! },
        feet, yaw, pitch, heightMetres, clip: strings[words[w + 5]!]!, time, lower: [strings[words[w + 6]!]!, lowerTime], layers, tint,
        loadout: text(w + 7), resident: text(w + 8), figure: text(w + 9), variety: variety === NONE ? null : variety,
        outfit: words[w + 11] === NONE ? null : this.#outfit(words[w + 11]!), headwear: has(IMPORTED.headwear), swing, chains,
        bow: has(IMPORTED.bow) ? { clip: text(w + 12)!, time: bowTime } : null,
        sheathed: [has(IMPORTED.sheathedLeft), has(IMPORTED.sheathedRight)],
        weaponDrawn: has(IMPORTED.weaponMode) ? has(IMPORTED.weaponDrawn) : null,
        throwable: text(w + 13) as Throwable | null, belted: text(w + 14) as Throwable | null,
        crossbow: has(IMPORTED.crossbow)
          ? { leftHand: has(IMPORTED.crossbowLeftHand), crank: has(IMPORTED.crossbowCrank), bolt: text(w + 15) as CrossbowCarry["bolt"] }
          : null,
        channel: channelPoint ? { joint: text(w + 16) as SpellChannel["joint"], point: channelPoint } : null,
        chore: chore === null ? null : { item: chore, socket: text(w + 18)! },
      });
      cursor.word = w + 20 + layerCount * 2;
      cursor.number = n;
    }
    return requests;
  }

  /** A node's name without the unit and part numbers (`unit-creature-12-piece-3` → `unit-creature-piece`), so the draw profile groups each kind of part. */
  #nodeName(id: number): string {
    return this.#nodeNames[id] ??= this.#strings[id]!.replace(/[-:]\d+(?=[-:]|$)/g, "");
  }

  #color(rgb: number): Color {
    let color = this.#colors.get(rgb);
    if (color === undefined) {
      color = `#${rgb.toString(16).padStart(6, "0")}`;
      this.#colors.set(rgb, color);
    }
    return color;
  }

  /** A class outfit, `preset;socket,socket`, parsed once per interned string. */
  #outfit(id: number): ClassOutfit {
    let outfit = this.#outfits.get(id);
    if (!outfit) {
      const [preset, sockets] = this.#strings[id]!.split(";") as [string, string];
      outfit = { preset, replaced: sockets === "" ? [] : sockets.split(",") };
      this.#outfits.set(id, outfit);
    }
    return outfit;
  }
}
