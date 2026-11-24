import * as THREE from "three";
import type {
  Matrix4Values,
  RendererCamera,
  RendererEnvironment,
  RendererInstanceBatch,
  RendererSceneNode,
  RendererWorkObservations,
  ThreeSceneRenderer,
} from "@moritzbrantner/three-d-renderer";
import type { WorldCommand } from "../command-wire";
import { entityKey, sameEntity } from "../entity-ref";
import type { EntityRef } from "../entity-ref";
import { ingestedProjection, type EntityState, type StrikeHeight, type ZoneSnapshot } from "../replication";
import { DebugOverlay, FrameRate } from "./debug-overlay";
import { ENVIRONMENT, environmentAt } from "./environment";
import { wasm } from "../wasm";
import { Minimap, dispositionOf, type MinimapElements, type MinimapUnit } from "./minimap";
import { MinimapPresenter } from "./minimap-presenter";
import { Exploration, minimapExploration } from "./exploration";
import type { MapSelf, WorldMap } from "./world-map";
import { MovementOutbox, type MovementInput } from "./movement-outbox";
import {
  IDLE_INTENT,
  OrbitCamera,
  PIXELS_PER_WHEEL_LINE,
  clearSightLine,
  movementInput,
  pointerOwner,
  type CameraMode,
  type CameraView,
  type DragMode,
  type HeldIntent,
  type PointerOwner,
  type Vec3,
} from "./orbit-camera";
import { SceneryFrame, buildSceneryScene, type DrawnKits, type SceneryScene } from "./scenery-nodes";
import { markStartup } from "../startup-marks";
import { EntryPrebuild, STATIC_PARTS, type DerivedParts, type EntryPreview, type PrebuildWork } from "./entry-prebuild";
import { FrameStages, PanelGate, RootRegistry, emptyFrameWork, fixedRootEntry, rootEntry, type FrameWork } from "./frame-stages";
import type { GreyhavenLighting } from "./lighting";
import { ImpactEffects } from "./impact-effects";
import { KitEffects } from "./kit-effects";
import { RaidSmoke } from "./raid-smoke";
import { type AidTask, reachableAidTask } from "./raid-aid";
import { type RaidPrepSpots, reachablePrepStep } from "./raid-prep";
import { RaidSigns } from "./raid-signs";
import { WalkInBuildings } from "./walk-in-buildings";
import { ZoneLifecycle } from "./zone-lifecycle";
import { KitModules, OUTPOST_BUNDLE, SETTLEMENT_BUNDLE, type KitPlacement } from "./kit-modules";
import { useBundleKtx2Loader } from "./static-bundle";
import { BatchedScenery } from "./static-batches";
import { CompositionKit } from "./composition-kit";
import { Ground } from "./ground";
import { Grass } from "./grass";
import { PlacedKit, type PlacedKitSource } from "./placed-kit";
import { AmbientAnimals } from "./ambient-animals";
import { ArtPrefetch } from "./art-prefetch";
import { isOutpostArea, kitNearArea, kitNearPoint, shelteredOutpost } from "./crater-streaming";
import { SkyLayer } from "./sky";
import { SharedUnits, type AnimationBlend, type ImportedModelKind, type Loadout, type ViewerAnimation, type ViewerLook } from "./unit-presenter";

import { BagsPanel, type BagsElements } from "./units/bags-panel";
import { CharacterPane, type CharacterElements } from "./units/character-pane";
import { abilitySlots, cancelCast, useAbilitySlot } from "./units/abilities";
import { PowerDrawInput } from "./units/power-draw";
import { HEROIC_STRIKE_ABILITY, canStrikeDirected, directedStrikeCommand } from "./units/directed-strike";
import { MouseOwnerInput, type EndedGesture, type WheelKindName } from "./units/mouse-owner";
import type { WheelPreferences, WorldDetail } from "../settings/preferences";
import { ShieldGuardHold, canGuard, guardCommand } from "./units/shield-guard";
import { ClassHud } from "./units/class-hud";
import { CombatHud } from "./units/combat-hud";
import { LootPanel, type LootElements } from "./units/loot-panel";
import { WaystonePanel, type WaystoneElements } from "./units/waystone-panel";
import { VendorPanel, type VendorElements } from "./units/vendor-panel";
import { QuestPanel, type MarkerPoint, type QuestElements } from "./units/quest-panel";
import { evidenceInReach } from "./units/quest-state";
import type { PartyRequestInput } from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import { ChatFrame, type ChatElements } from "./units/chat-frame";
import { DepthsLootPanel, type DepthsLootElements } from "./units/depths-loot-panel";
import { PartyFrame, type PartyElements } from "./units/party-frame";
import { nearestAnimal, projectedUnit, type ProjectedUnit } from "./units/projected-units";
import { ProgressionHud } from "./units/progression-hud";
import { PointerClick, combatFocus } from "./units/targeting";
import { WorldPicking } from "./units/world-picking";
import { QuaterniusUnits, type FaceFeature, type Hair, type Headwear } from "./units/quaternius-units";
import type { ContentCatalog } from "./catalog";
import type { Area } from "./scenery";
import type { WorldSource, ZoneWorld } from "./world-source";
import type { AudioPreferences, WorldSound } from "./world-sound";
import { touchContext, type TouchContext } from "../input/touch-layout";
import { zoneConstants } from "../zone-constants";
import { profileFacts, profilePhase, profiling } from "./frame-profile";

/**
 * The in-world presentation: static scenery, animated units, the orbit
 * camera and its input, the CSS sky, the minimap and the F3 overlay. It reads
 * decoded player-scoped projections from the world source and sends intent
 * through the movement outbox; it owns no gameplay rules.
 */
export type WorldViewElements = {
  canvas: HTMLCanvasElement;
  sky: HTMLElement;
  minimap: MinimapElements;
  /** The world map's host (M). */
  worldMap: HTMLElement;
  overlay: HTMLElement;
  areaName: HTMLElement;
  /** The viewer's health and target line. */
  unitStatus: HTMLElement;
  /** The latest combat feedback line. */
  combatFeedback: HTMLElement;
  experienceBar: HTMLProgressElement;
  experienceStatus: HTMLElement;
  progressionFeedback: HTMLElement;
  bags: BagsElements;
  character: CharacterElements;
  loot: LootElements;
  waystone: WaystoneElements;
  vendor: VendorElements;
  quests: QuestElements;
  depthsLoot: DepthsLootElements;
  party: PartyElements;
  chat: ChatElements;
  /** Action bar, unit frames, cast bars and combat text. */
  classHud: HTMLElement;
};

/**
 * Where the opt-in radial wheel usage counters (#653, Atlas 5.2.i653-2) live: this browser's local
 * storage only, as JSON (opens, picks, cancels per wheel; milliseconds per mouse owner). Nothing is
 * ever sent; read or copy it from the browser's storage tools.
 */
export const WHEEL_USAGE_KEY = "mmorpg.wheelUsage.v1";

function readWheelUsage(): string | null {
  try {
    return localStorage.getItem(WHEEL_USAGE_KEY);
  } catch {
    return null;
  }
}

/** A targeting or attack intent, resolved against the latest projection when it is sent. */
export type Intent = (projection: ZoneSnapshot) => WorldCommand | null;

/** Outside every named area the HUD names the zone itself. */
const ZONE_NAME = "Greyhaven Vale";
/**
 * The selection turntable's class weapons, as the shared presenter's `Loadout::of_class`; the
 * Rogue's follows `imported::ROGUE_LOADOUT` (two daggers, #603).
 */
const CLASS_LOADOUTS: Readonly<Record<NonNullable<ViewerLook["classId"]>, Loadout>> = {
  warden: "sword-and-shield", ranger: "bow-and-quiver", arcanist: "staff.crystal-and-tome", necromancer: "staff.bone",
  rogue: "dagger.twin", cleric: "mace.flanged", druid: "staff.gnarled",
};
/** Quest markers float this far above an NPC's feet (metres). */
const MARKER_HEIGHT_METRES = 2.3;

/** Debug-only camera poses for screenshots; they never touch the simulation. */
export const DEBUG_VIEWPOINTS: Readonly<Record<string, { eye: Vec3; target: Vec3 }>> = {
  hub: { eye: [5, 8.5, 31], target: [-14, 3, 4] },
  grass: { eye: [-23, 16, 29], target: [-23, 0, 11] },
  relief: { eye: [20, 8, 39], target: [0, 0, 18] },
  woods: { eye: [-41, 18, 16], target: [-69, 14, 0] },
  hollow: { eye: [6, 24, -64], target: [0, 14, -98] },
  farm: { eye: [46, 12, 16], target: [72, 3, 40] },
  // Stillwater moved west in #644; keep this pose matched to native ground shots/profiling.
  lake: { eye: [-56, 33, -39], target: [-87, 11, -53] },
  vale: { eye: [0, 70, 95], target: [0, 0, -10] },
};

/** A debug map view of one XZ rectangle in metres (`mmorpg-world-studio capture`). */
type PlanView = { minX: number; minZ: number; maxX: number; maxZ: number; groundOnly?: boolean };
/** Above every terrain height and structure, and well inside the plan's depth range. */
const PLAN_EYE_HEIGHT_METRES = 200;
const PLAN_DEPTH_METRES = 400;
/** Reported to the level-of-detail selection in a map view, so it always picks the finest level. */
const PLAN_DETAIL_FOV_DEGREES = 0.5;

/** Straight down over the middle of `plan`: three's `lookAt` puts +X right and +Z down the image. */
function planPose(plan: PlanView): { eye: Vec3; target: Vec3 } {
  const x = (plan.minX + plan.maxX) / 2;
  const z = (plan.minZ + plan.maxZ) / 2;
  return { eye: [x, PLAN_EYE_HEIGHT_METRES, z], target: [x, 0, z] };
}

/** Replaces the camera's projection with an orthographic one exactly covering `plan`. */
function applyPlanProjection(camera: THREE.PerspectiveCamera, plan: PlanView): void {
  const halfWidth = (plan.maxX - plan.minX) / 2;
  const halfHeight = (plan.maxZ - plan.minZ) / 2;
  camera.fov = PLAN_DETAIL_FOV_DEGREES;
  camera.projectionMatrix.makeOrthographic(-halfWidth, halfWidth, halfHeight, -halfHeight, 1, PLAN_DEPTH_METRES);
  camera.projectionMatrixInverse.copy(camera.projectionMatrix).invert();
}

export function webGpuProjection(source: THREE.PerspectiveCamera): Matrix4Values {
  const gl = source.projectionMatrix.elements;
  const gpu = [...gl];
  for (const index of [2, 6, 10, 14]) {
    const z = gl[index];
    const w = gl[index + 1];
    if (z === undefined || w === undefined) {
      throw new Error("Incomplete projection matrix");
    }
    gpu[index] = (z + w) / 2;
  }
  return gpu as Matrix4Values;
}

type Drag = { pointerId: number; x: number; y: number; mode: Exclude<DragMode, "none"> };
type Box = { min: Vec3; max: Vec3 };

/** Structure collider boxes in metres, for choosing an unobstructed first view. */
function occluders(world: ZoneWorld): Box[] {
  const { props, unitsPerMetre } = world.scenery.scenery;
  const margin = 0.4;
  return props.flatMap((prop) => {
    const collider = prop.collider;
    if (!collider) {
      return [];
    }
    const [cx, cy, cz] = collider.center.map((value) => value / unitsPerMetre) as Vec3;
    const [hx, hy, hz] = collider.halfExtents.map((value) => value / unitsPerMetre) as Vec3;
    return [{ min: [cx - hx - margin, cy - hy, cz - hz - margin], max: [cx + hx + margin, cy + hy + margin, cz + hz + margin] }];
  });
}

/** Whether the sight line from `target` to `eye` passes through a box (sampled). */
function obstructed(boxes: readonly Box[], target: Vec3, eye: Vec3): boolean {
  for (const t of [0.35, 0.6, 0.8, 1]) {
    const point = [0, 1, 2].map((axis) => target[axis]! + (eye[axis]! - target[axis]!) * t) as Vec3;
    if (boxes.some((box) => point.every((value, axis) => value >= box.min[axis]! && value <= box.max[axis]!))) {
      return true;
    }
  }
  return false;
}

/** `padGuard` is the lane a pad holding the guard trigger aims, or `null` (#156); `rolls` counts dodge-roll presses (#516) like `jumps`. */
export type WorldInput = { held: HeldIntent; jumps: number; rolls?: number; padGuard?: StrikeHeight | null };

const NO_DRESSED = new Uint32Array();

/** Areas within this distance of the player (60 m) have their art fetched ahead of arrival. */
const STREAM_RADIUS_UNITS = 6_000;

/** The viewer's feet XZ in a projection. */
function entitiesSelf(projection: ZoneSnapshot): [number, number] | null {
  const self = projection.entities.find((entity) => entity.kind === "player" && entity.entityId === projection.viewerId);
  return self ? [self.position[0], self.position[2]] : null;
}

type Bounds = { minXz: readonly [number, number]; maxXz: readonly [number, number] };
type AreaBounds = Bounds & { id: number };

/** Squared XZ gap between two boxes (0 when they touch or overlap). */
function boxGapSquared(a: Bounds, b: Bounds): number {
  const dx = Math.max(a.minXz[0] - b.maxXz[0], 0, b.minXz[0] - a.maxXz[0]);
  const dz = Math.max(a.minXz[1] - b.maxXz[1], 0, b.minXz[1] - a.maxXz[1]);
  return dx * dx + dz * dz;
}

/**
 * Greyhaven's art in pieces (#240): the Ranger, each creature model and the inn load once on
 * first need. A region needs the Ranger, its creatures' models and, for the area the inn stands
 * in, the inn. A failed piece settles too; its units keep their procedural stand-ins. Selection
 * orders the loads by intent (#412, `ArtPrefetch`).
 */
class RegionArt {
  readonly #world: ZoneWorld;
  readonly #innArea: number | null;
  /** The areas the Outpost kit buildings (#420) stand in. */
  readonly #outpostAreas: Set<number>;
  /** The areas the market square's settlement props (#421) stand in. */
  readonly #settlementAreas: Set<number>;
  /** The areas the ambient animals (#419) pace in. */
  readonly #ambientAreas: Set<number>;
  readonly #prefetch: ArtPrefetch;

  constructor(world: ZoneWorld, models: QuaterniusUnits, inn: WalkInBuildings, outpost: KitModules, settlement: KitModules, ambient: AmbientAnimals, onProgress: () => void) {
    this.#world = world;
    const innProp = world.scenery.scenery.props.find((prop) => prop.kind === "inn");
    this.#innArea = innProp ? world.scenery.areaAt(innProp.position[0], innProp.position[2])?.id ?? null : null;
    const unit = world.scenery.scenery.unitsPerMetre;
    const areas = (placements: readonly KitPlacement[]) => new Set(placements.flatMap(({ matrix }) => {
      const area = world.scenery.areaAt(matrix[12]! * unit, matrix[14]! * unit);
      return area ? [area.id] : [];
    }));
    this.#outpostAreas = areas(world.scenery.outpost.placements);
    this.#settlementAreas = areas(world.scenery.settlement.placements);
    this.#ambientAreas = new Set(world.scenery.ambient.anchors.flatMap(([x, z]) => {
      const area = world.scenery.areaAt(x, z);
      return area ? [area.id] : [];
    }));
    // Bundle glTF files share textures; three's file cache serves a repeat from memory instead
    // of downloading it again, and is emptied whenever no piece is loading.
    THREE.Cache.enabled = true;
    this.#prefetch = new ArtPrefetch(
      (piece) => (piece === "inn" ? inn.load() : piece === "outpost" ? outpost.load() : piece === "settlement" ? settlement.load()
        : piece === "ambient" ? ambient.load() : piece === "residents" ? models.preloadResidents() : models.preloadWorld(piece as ImportedModelKind)),
      (idle) => {
        if (idle) { THREE.Cache.clear(); }
        onProgress();
      },
    );
  }

  /** The pieces area `id` needs (every piece for `null`). */
  #pieces(id: number | null): string[] {
    if (id === null) {
      return ["Ranger", "Wolf", "Rat", "Boar", "Bear", "Spider", "Lurker", "Lynx", "Adder", "Bat", "Crayfish", "Stag", "residents", "inn", "outpost", "settlement", "ambient"];
    }
    return [
      "Ranger",
      ...this.#world.scenery.regionModels(id),
      ...(id === this.#innArea ? ["inn"] : []),
      ...(this.#outpostAreas.has(id) ? ["outpost"] : []),
      ...(this.#settlementAreas.has(id) ? ["settlement"] : []),
      ...(this.#ambientAreas.has(id) ? ["ambient"] : []),
    ];
  }

  /** Starts area `id`'s pieces now (every area's for `null`). */
  fetch(id: number | null): void {
    this.#prefetch.request(this.#pieces(id));
  }

  /**
   * Selection's plan (#412): area `entry`'s pieces now; once they settled, `others` (other
   * characters' saved areas, most likely first), then the areas within streaming reach of `entry`.
   * A later plan replaces the queued remainder.
   */
  plan(entry: number, others: readonly number[]): void {
    const areas = this.#world.scenery.scenery.areas;
    const home = areas.find((area) => area.id === entry);
    // The Outpost's rim screens the other regions from the village floor. Fetch only the
    // currently selected location during selection; the exit approach warms its neighbours.
    const near = home && !isOutpostArea(home) ? areas.filter((area) => area.id !== entry && boxGapSquared(area, home) <= STREAM_RADIUS_UNITS ** 2) : [];
    // First paint the real character on the selection stage. Greyhaven art may be much larger;
    // starting it in parallel used to compete with the avatar on a cold browser visit.
    if (!this.#prefetch.settled("Ranger")) {
      this.#prefetch.plan({ foreground: ["Ranger"], background: [] });
      return;
    }
    // Then the inn, which alone holds Enter World (#855): the area's other pieces would share its downloads and decodes.
    if (entry === this.#innArea && !this.#prefetch.settled("inn")) {
      this.#prefetch.plan({ foreground: ["inn"], background: [] });
      return;
    }
    this.#prefetch.plan({
      foreground: this.#pieces(entry),
      background: (isOutpostArea(home) ? [] : [...others, ...near.map((area) => area.id)]).flatMap((id) => this.#pieces(id)),
    });
  }

  progress(id: number | null): { loaded: number; total: number } {
    const pieces = this.#pieces(id);
    return { loaded: pieces.filter((piece) => this.#prefetch.settled(piece)).length, total: pieces.length };
  }

  /**
   * Whether area `id` can be entered: only the inn blocks entry, and only in its own area (#395).
   * Units enter as procedural stand-ins and swap to their models once those settle.
   */
  entryReady(id: number): boolean {
    return id !== this.#innArea || this.#prefetch.settled("inn");
  }

  /** Fetches the area at `at` and every area within the streaming radius of it. */
  approach(at: [number, number] | null, areas: readonly AreaBounds[]): void {
    if (!at) { return; }
    const sheltered = shelteredOutpost(at, this.#world.scenery.scenery.areas);
    if (sheltered) {
      this.fetch(sheltered.id);
      return;
    }
    for (const area of areas) {
      if (boxGapSquared(area, { minXz: at, maxXz: at }) <= STREAM_RADIUS_UNITS ** 2) {
        this.fetch(area.id);
      }
    }
  }
}

/** The viewer's presented unit this frame: the camera's focus and its feet, metres. */
type FrameSelf = { focus: Vec3; x: number; feetY: number; z: number; facing: number };

/** One world frame's shared state (#1060): one object reused every frame; each stage fills what later stages read. */
class WorldFrame {
  stopped = false;
  world!: ZoneWorld;
  input!: WorldInput;
  look!: ViewerLook;
  deltaSeconds = 0;
  now = 0;
  scene!: SceneryScene;
  sceneryFrame!: SceneryFrame;
  received: readonly ZoneSnapshot[] = [];
  connected = false;
  projection!: ZoneSnapshot;
  /** The zone tick whose hour lights the frame. */
  tick = 0n;
  animate = false;
  units!: SharedUnits;
  presented!: ReturnType<SharedUnits["frame"]>;
  /** Each presented unit's feet by entity key; cleared and refilled every frame. */
  readonly anchors = new Map<string, { x: number; feetY: number; z: number }>();
  others: MinimapUnit[] = [];
  self!: FrameSelf;
  unitNodes: readonly RendererSceneNode[] = [];
  effectNodes: readonly RendererSceneNode[] = [];
  view!: CameraView;
  sceneFrame!: ReturnType<SceneryFrame["nodes"]>;
  nodes: RendererSceneNode[] = [];
  batchedNodes = 0;
  camera!: RendererCamera;
  environment!: RendererEnvironment;
  dayFraction = 0;
}

/** Panels each received projection reaches, and panels the latest projection redraws when it changes. */
const RECEIVED_PANELS = 9;
const LATEST_PANELS = 3;
const NO_NODES: readonly RendererSceneNode[] = [];
const NO_KICK = [0, 0, 0] as const;
/** The static scene's ground part (`#buildStaticPart`), which waits for the terrain's far levels (#855). */
const GROUND_PART = STATIC_PARTS - 2;
/** The static scene's last part, the grass, which waits for the grass's seed (#855). */
const GRASS_PART = STATIC_PARTS - 1;

function appendAll<T>(into: T[], from: readonly T[]): void {
  for (let index = 0; index < from.length; index += 1) { into.push(from[index]!); }
}

function sameRoots(roots: readonly THREE.Object3D[], submitted: ReadonlySet<THREE.Object3D>): boolean {
  if (roots.length !== submitted.size) { return false; }
  for (const root of roots) {
    if (!submitted.has(root)) { return false; }
  }
  return true;
}

export class WorldView {
  readonly #renderer: ThreeSceneRenderer;
  readonly #camera: THREE.PerspectiveCamera;
  readonly #elements: WorldViewElements;
  readonly #reducedMotion: MediaQueryList;
  /** The player's motion preference; `system` follows `prefers-reduced-motion`. */
  #motion: "system" | "reduce" | "full" = "system";
  /** Look-drag multiplier from Settings; 1 is the default sensitivity. */
  #lookSensitivity = 1;
  readonly #overlay: DebugOverlay;
  readonly #frameRate = new FrameRate();
  readonly #sky: SkyLayer;
  /** Imported unit models (Atlas 6.3.6/6.3.7), skinned once loaded; the shared presenter picks their clips. */
  readonly #models = new QuaterniusUnits((kind) => this.#restClip(kind));
  /** The walk-in inn and Elden's house (#158), drawn from their asset-tooling bundles once loaded. */
  #inn: WalkInBuildings | null = null;
  /** The Outpost kit buildings (#420), drawn from asset-tooling's Outpost kit once loaded; their blockouts until then. */
  #outpost: KitModules | null = null;
  #fortifications: KitModules | null = null;
  /** The market square's settlement props (#421), drawn from asset-tooling's bundle once loaded; their boxes until then. */
  #settlement: KitModules | null = null;
  /** The Battle Royale fortress (#591), drawn from asset-tooling's fortress kit once loaded; its graybox until then. */
  #fortress: CompositionKit | null = null;
  /** Greyhaven's cosmetic cat, dog and rodent (#419), drawn once their bundle loads. */
  #ambient: AmbientAnimals | null = null;
  /** Greyhaven's trees, rocks and bushes (#49), drawn from asset-tooling's bundle once loaded; the procedural ones until then. */
  #nature: PlacedKit | null = null;
  #climbing: PlacedKit | null = null;
  #crag: PlacedKit | null = null;
  #gather: PlacedKit | null = null;
  /** The ore veins (#917): centre in units and their full and spent `gather` placements. */
  #veins: readonly { x: number; z: number; full: number; spent: number }[] = [];
  /** Every ordinary placed kit of the shared inventory (#683), drawn alike: no field or branch per kit. */
  #kits: readonly PlacedKit[] = [];
  /** Bundles are fetched only once some authored placement is near the selected area/player. */
  #placedKits: { kit: PlacedKit; source: PlacedKitSource; requested: boolean }[] = [];
  #pendingKitArea: Area | null = null;
  #avatarPrepared = false;
  #ground: Ground | null = null;
  #grass: Grass | null = null;
  #worldDetail: WorldDetail = "medium";
  /** The roots last submitted to the renderer. */
  #submittedRoots = new Set<THREE.Object3D>();
  #regions: RegionArt | null = null;
  #onArtProgress: (() => void) | null = null;
  /** Authored hit/block bursts (#52) on zone-confirmed events; one permanent imported root. */
  readonly #impacts = new ImpactEffects();
  /** The strike and spell effect bundles (#894), drawn where the shared presenter places them. */
  readonly #kitEffects = new KitEffects();
  /** Smoke over a raided barn (#530); one permanent imported root. */
  readonly #raidSmoke = new RaidSmoke();
  readonly #raidSigns = new RaidSigns();
  /** The raided barn's roof centre and smoke span in metres, by objective (#530). */
  #raidRoof: { key: string; top: [number, number, number]; span: [number, number] } | null = null;
  #reachableDoor: (x: number, z: number) => number = () => 0;
  #reachableSeat: (x: number, z: number) => number = () => 0;
  #reachablePieces: (x: number, z: number) => readonly number[] = () => [];
  /** The night raid target's disturbance circle in units (#530), from the scenery. */
  #raidArea: { x: number; z: number; radius: number } | null = null;
  /** The night raid target's aftermath aid task spots (#531), from the scenery. */
  #raidAid: readonly AidTask[] = [];
  /** The night raid target's forewarning spots (#532), from the scenery. */
  #raidPrep: RaidPrepSpots | null = null;
  #restClip: (kind: ImportedModelKind) => string = () => { throw new Error("No world is loaded."); };
  /** Models loaded so far with their clip lengths, replayed into each new presenter. */
  readonly #loadedModels: [ImportedModelKind, [string, number][], string | null][] = [];
  /** The shared unit presenter (#184) of the current viewer. */
  #units: { viewerId: number; units: SharedUnits; pushed: ZoneSnapshot | null; residents: boolean; kitPlatforms: boolean; effectKits: boolean } | null = null;
  /** Combat sounds (#447), once the bundle loaded; silent until then. */
  #sound: WorldSound | null = null;
  #audioPreferences: AudioPreferences | null = null;
  /** Camera heading of the last frame; health bars face it. */
  #barYaw = 0;
  #lastCamera: RendererCamera | null = null;
  readonly #combatHud: CombatHud;
  readonly #progressionHud: ProgressionHud;
  readonly #classHud: ClassHud;
  readonly #weaponMode: HTMLButtonElement;
  readonly #bags: BagsPanel;
  readonly #character: CharacterPane;
  readonly #loot: LootPanel;
  readonly #waystone: WaystonePanel;
  readonly #vendor: VendorPanel;
  readonly #quests: QuestPanel;
  readonly #chat: ChatFrame;
  readonly #depthsLoot: DepthsLootPanel;
  readonly #party: PartyFrame;
  #partySource: WorldSource | null = null;
  readonly #intents: { intent: Intent; onSent: ((sequence: number | null) => void) | undefined }[] = [];
  /** Mouse-look travel since a captured left press; past a few pixels the press is no click. */
  #lockedTravel = 0;
  readonly #primaryClick = new PointerClick(0);
  #lastLinkConnected = true;
  #presentationTime: number | null = null;
  /** Debug only (#753): the world source is not advanced, so reference captures see one zone state. */
  #holdWorld = false;
  #lastEnvironment: RendererEnvironment | null = null;
  #lastInstanceBatches: RendererInstanceBatch[] = [];
  readonly #picking = new WorldPicking();
  #lastSceneryNodes: readonly RendererSceneNode[] = [];
  #pendingNpc: { target: EntityRef; sequence: number } | null = null;
  /** The presented zone's resource owner (#1005), retired when the next zone loads. */
  #zone = new ZoneLifecycle();
  #scene: SceneryScene | null = null;
  readonly #lighting: GreyhavenLighting;
  #catalog: ContentCatalog | null = null;
  #zoneName = ZONE_NAME;
  #arena = false;
  /** The unit nodes of the last frame, for debug readouts. */
  #lastUnitNodes: readonly RendererSceneNode[] = [];
  #lastEffectNodes: readonly RendererSceneNode[] = [];
  /** The projected units of the last frame, for debug readouts. */
  #lastProjectedUnits: readonly ProjectedUnit[] = [];
  #sceneryFrame: SceneryFrame | null = null;
  /** The static scene's draw groups, batched per material (#615); the frame's nodes only show and hide them. */
  #sceneryBatches: BatchedScenery | null = null;
  #presentationFingerprint: string | null = null;
  #reliefAt: ((x: number, z: number) => number) | null = null;
  readonly #minimap: MinimapPresenter;
  /** The exploration map (#983) both maps draw, from the viewer's discovered areas and quest sheet. */
  readonly #exploration = new Exploration();
  readonly #worldMapHost: HTMLElement;
  /** The world map, loaded with the Maps library on the first M so the game starts without it. */
  #worldMap: WorldMap | null = null;
  #worldMapLoading = false;
  /** The world `load` last readied, so re-selecting it (`refreshArenaWorld`) rebuilds nothing. */
  #loadedScenery: ZoneWorld["scenery"] | null = null;
  #loadedArena = false;
  /** The world `load` last readied, whose static scene and entry view `prebuild` prepares. */
  #loadedWorld: ZoneWorld | null = null;
  /** How many of the static scene's parts (`STATIC_PARTS`) are built for the loaded world. */
  #staticParts = 0;
  /** The loaded scene's rest transforms, read by the static scene's first part for its second (#855). */
  #restModels: Float32Array | null = null;
  /**
   * Which of the worker's later answers have reached the page's module (#855): the prebuild stops before the ground
   * until the terrain's far levels did, and before the grass until its seed did.
   */
  #derived: DerivedParts = { far: false, grass: false };
  /** Static scene builds a world frame had to run itself: none once the selection screen prebuilt it (#855). */
  #frameStaticBuilds = 0;
  /** The selection screen's prebuild of the entry view (#855). */
  readonly #prebuild: EntryPrebuild<EntryPreview>;
  /** The prebuild's placeholder frame: the entry view without units. */
  readonly #prebuildFrame = new WorldFrame();
  /** Hidden copies of the selection stage's nodes, kept through a prebuild's prepare so it evicts none of them. */
  #selectionStage: RendererSceneNode[] = [];
  /** Moves whenever the render-root list was rebuilt outside a world frame, so a prebuilt view prepares again. */
  #rootsVersion = 0;
  /** The prebuilt view's renderer nodes and instance batches, hidden, which the turntable's frames keep (#855). */
  #selectionKeep: { nodes: RendererSceneNode[]; instanceBatches: RendererInstanceBatch[] } | null = null;
  #buildMs = 0;
  #orbit = new OrbitCamera();
  #cameraMode: CameraMode = { kind: "free-look" };
  #pointerOwner: PointerOwner = "interface";
  #locked = false;
  /** A held Aimed or Piercing Shot draw (#607): shown locally, sent on the key's release. */
  readonly #powerDraw = new PowerDrawInput({
    prepare: (ability) => this.#units?.units.prepare(ability, this.#seconds),
    abandon: () => this.#units?.units.abandonPreparation(),
    send: (ability) => this.queueIntent(
      () => ({ kind: "use-ability", ability, target: null, facing: this.#aimFacing() }),
      () => this.#units?.units.preparedSent(this.#seconds),
    ),
  });
  #drag: Drag | null = null;
  /** Fingers on the canvas, and the spread of a two-finger pinch zoom in progress (#434). */
  readonly #touches = new Map<number, { x: number; y: number }>();
  #pinch: number | null = null;
  /** A held shield guard whose lane follows the pointer (#156). */
  readonly #guardHold = new ShieldGuardHold();
  /** Who owns the mouse (#636): free look, focus, or a held strike or guard aimed with it (#154). */
  readonly #mouse = new MouseOwnerInput();
  /** The manual aim's expected landing marker (#649), made on the first aim. */
  #aimLanding: HTMLElement | null = null;
  /** Pointer Lock this view released itself, so its `pointerlockchange` opens no menu. */
  #releasingLock = false;
  /** A left press in released free look: its click selects under the cursor, or recaptures. */
  #recaptureClick = false;
  /** Called when the browser takes Pointer Lock away (its Escape): the game menu opens (#636). */
  onPointerLockLost: (() => void) | null = null;
  /** Runs a radial wheel's chosen action (#653) by its semantic id, as its own key would. */
  onWheelAction: ((id: string) => void) | null = null;
  /** Radial wheels (#653): Settings → Wheels. */
  #wheels: WheelPreferences = { enabled: false, countUsage: false, sectors: [] };
  /** When the open wheel's key went down: a quick tap keeps it open for a click. */
  #wheelPressedAt = 0;
  /** The open wheel's overlay, made on the first wheel, and the sectors it shows. */
  #wheelElement: HTMLElement | null = null;
  #wheelShown: string | null = null;
  /** When the opt-in usage counters were last stored. */
  #usageSavedAt = 0;
  #movementFacing = 0;
  #outbox: MovementOutbox;
  #shownArea: string | null = null;
  #seconds = 0;
  #viewerClip: string | null = null;
  #viewerAnimation: ViewerAnimation | null = null;
  /** The Action Lab's blend, or null for the presenter's default. */
  #blend: AnimationBlend | null = null;
  /** The development body overlay (#39). */
  #bodyOverlay = false;
  #flyTo: { eye: Vec3; target: Vec3 } | null = null;
  /** The debug map view, or null for the ordinary perspective camera. */
  #plan: PlanView | null = null;
  /** The world roots the map view hid, shown again as soon as it stops hiding them. */
  readonly #planHidden = new Set<THREE.Object3D>();
  /** The perspective camera's field of view, restored on leaving the map view. */
  readonly #perspectiveFov: number;
  #areas: ZoneWorld["scenery"]["scenery"]["areas"] = [];
  #lastWork: RendererWorkObservations | null = null;
  /** While `prepare` runs its frame: the renderer's pending preparations. */
  #preparing: Promise<unknown>[] | null = null;
  /** The selection turntable's first frame (#855): `pending` while the renderer prepares its programs and textures. */
  #previewPrepared: "no" | "pending" | "ready" = "no";
  /** The turntable's roots changed while its first frame was prepared, so the next frame submits them again. */
  #previewStale = false;
  #occluders: Box[] = [];
  /** The viewer's projected unit as last drawn, for debug readouts. */
  #lastSelf: { x: number; z: number; facing: number } | null = null;
  /** The projection the last frame drew, for debug readouts. */
  #lastProjection: ZoneSnapshot | null = null;
  /** The first frame of a session picks a heading whose view is not inside a building. */
  #framePending = false;
  /** The staged frame (#1060): its work counters, render-root registry, panel gate and reused context. */
  readonly #frameWork = emptyFrameWork();
  readonly #rootRegistry = new RootRegistry(this.#frameWork);
  /** The registry rebuilt since its roots were last compared with the submitted set. */
  #rootsStale = true;
  readonly #panelGate = new PanelGate<ZoneSnapshot>();
  readonly #frame = new WorldFrame();
  readonly #stages = new FrameStages<WorldFrame>([
    { stage: "input", update: (frame) => this.#inputStage(frame) },
    { stage: "replication", update: (frame) => this.#replicationStage(frame) },
    { stage: "units", update: (frame) => this.#unitStage(frame) },
    { stage: "camera", update: (frame) => this.#cameraStage(frame) },
    { stage: "scenery", update: (frame) => this.#sceneryStage(frame) },
    { stage: "render", update: (frame) => this.#renderStage(frame) },
    { stage: "hud", update: (frame) => this.#hudStage(frame) },
    { stage: "panels", update: (frame) => this.#panelStage(frame) },
  ], this.#frameWork);

  constructor(renderer: ThreeSceneRenderer, camera: THREE.PerspectiveCamera, lighting: GreyhavenLighting, elements: WorldViewElements) {
    this.#renderer = renderer;
    this.#lighting = lighting;
    this.#models.configureRenderer(renderer);
    // Every asset-tooling bundle may ship KTX2 textures (asset-tooling#511); they share this transcoder.
    useBundleKtx2Loader(this.#models.ktx2Loader);
    this.#camera = camera;
    this.#perspectiveFov = camera.fov;
    this.#elements = elements;
    this.#minimap = new MinimapPresenter(new Minimap(elements.minimap), ENVIRONMENT);
    this.#worldMapHost = elements.worldMap;
    this.#reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    this.#overlay = new DebugOverlay(elements.overlay);
    this.#sky = new SkyLayer(elements.sky, ENVIRONMENT);
    this.#combatHud = new CombatHud(elements.unitStatus, elements.combatFeedback);
    this.#classHud = new ClassHud(elements.classHud, {
      // A click on the bar has no release to wait for: it fires at once, power shots too (#607).
      onUse: (slot) => this.useSlot(slot),
    });
    this.#weaponMode = document.createElement("button");
    this.#weaponMode.type = "button";
    this.#weaponMode.className = "weapon-mode";
    this.#weaponMode.textContent = "Exploration · Draw weapon [H]";
    this.#weaponMode.addEventListener("click", () => this.toggleWeapon());
    // Above the action bar, in the dock's flow: the toolbar's corner holds Loot and Bags (#1079).
    (elements.classHud.querySelector(".action-dock") ?? elements.classHud).prepend(this.#weaponMode);
    this.#progressionHud = new ProgressionHud(elements.experienceBar, elements.experienceStatus, elements.progressionFeedback);
    this.#bags = new BagsPanel(elements.bags, (command, onSent) => this.queueIntent(() => command, onSent));
    // The pane and the vendor share the bags' received sheet: one revision covers bag and equipment.
    this.#character = new CharacterPane(elements.character, this.#bags.state, (command, onSent) => this.queueIntent(() => command, onSent));
    this.#waystone = new WaystonePanel(elements.waystone, (command) => this.queueIntent(() => command));
    this.#loot = new LootPanel(elements.loot, (intent) => this.queueIntent(intent), () => {
      this.#bags.close();
      this.#character.close(false);
      this.#vendor.close(false);
      this.#closeQuests();
    });
    this.#vendor = new VendorPanel(elements.vendor, this.#bags.state, (command, onSent) => this.queueIntent(() => command, onSent), () => {
      this.#loot.close(false);
      this.#character.close(false);
      this.#bags.close();
      this.#closeQuests();
    });
    // The quest dialog and log share the bag panes' places on screen, so they replace them.
    this.#quests = new QuestPanel(elements.quests, (command) => this.queueIntent(() => command), () => {
      this.#loot.close(false);
      this.#vendor.close(false);
      this.#bags.close();
      this.#character.close(false);
    });
    // Leaving the chat field hands the keyboard back to the world.
    this.#depthsLoot = new DepthsLootPanel(elements.depthsLoot, (command) => this.queueIntent(() => command));
    this.#party = new PartyFrame(elements.party, (request) => this.#sendParty(request));
    this.#chat = new ChatFrame(elements.chat, (command) => this.queueIntent(() => command), () => elements.canvas.focus(), (parsed) => {
      const request = this.#party.request(parsed);
      if (typeof request === "string") {
        this.#chat.note(request);
      } else {
        this.#sendParty(request);
      }
    });
    elements.bags.toggle.addEventListener("click", () => {
      this.#loot.close(false);
      this.#vendor.close(false);
      this.#closeQuests();
    });
    elements.character.toggle.addEventListener("click", () => {
      this.#loot.close(false);
      this.#vendor.close(false);
      this.#closeQuests();
    });
    this.#outbox = new MovementOutbox(this.#input({ held: IDLE_INTENT, jumps: 0 }));
    // The render roots in drawing order; `load`, `#buildStatic`, the detail level and a map view mark the registry dirty
    // when an owner is replaced or a condition below changes.
    const outpostReady = () => (this.#outpost?.drawn ?? false) && (this.#fortifications?.drawn ?? false);
    this.#rootRegistry.register([
      fixedRootEntry("world", () => this.#sceneryBatches),
      rootEntry("inhabitant", () => this.#models),
      rootEntry("world", () => this.#inn),
      // This combined Outpost kit replaces one shared fallback only when both bundles draw.
      rootEntry("world", () => this.#outpost, outpostReady),
      rootEntry("world", () => this.#fortifications, outpostReady),
      rootEntry("world", () => this.#settlement),
      rootEntry("world", () => this.#fortress),
      rootEntry("inhabitant", () => this.#ambient, () => this.#worldDetail !== "low"),
      rootEntry("world", () => this.#nature),
      rootEntry("world", () => this.#climbing),
      rootEntry("world", () => this.#crag),
      rootEntry("world", () => this.#gather),
      {
        role: "world",
        revision: () => {
          let revision = 0;
          for (const kit of this.#kits) { revision += kit.rootsRevision + 1; }
          return revision;
        },
        collect: (into) => { for (const kit of this.#kits) { kit.collectRoots(into); } },
      },
      fixedRootEntry("ground", () => this.#ground),
      fixedRootEntry("world", () => this.#grass, () => !this.#plan),
      fixedRootEntry("world", () => this.#impacts),
      fixedRootEntry("world", () => this.#kitEffects),
      fixedRootEntry("world", () => this.#raidSmoke),
      fixedRootEntry("world", () => this.#raidSigns),
    ]);
    this.#prebuild = new EntryPrebuild<EntryPreview>({
      staticBuilt: () => this.#staticParts >= STATIC_PARTS,
      staticPartReady: () => this.#staticParts === GROUND_PART ? this.#derived.far
        : this.#staticParts === GRASS_PART ? this.#derived.grass : true,
      buildStaticPart: () => { if (this.#loadedWorld) { this.#buildStaticPart(this.#loadedWorld); } },
      compose: (entry) => this.#composeEntry(entry),
      prepareKey: () => this.#entryPrepareKey(),
      prepare: (entry) => this.#prepareEntry(entry),
    });
  }

  get #animate(): boolean {
    return this.#motion === "system" ? !this.#reducedMotion.matches : this.#motion === "full";
  }

  /** Applies the Settings motion preference (presentation only). */
  setMotionPreference(motion: "system" | "reduce" | "full"): void {
    this.#motion = motion;
  }

  /** Plays the shared presenter's sound cues through `sound` from the next projection on. */
  attachSound(sound: WorldSound): void {
    this.#sound = sound;
    if (this.#audioPreferences) { sound.setPreferences(this.#audioPreferences); }
  }

  /** Applies the Settings volume and mute preferences. */
  /** The world detail setting (`graphics.world_detail`): how far and how thick the grass grows, if at all. */
  setWorldDetail(detail: WorldDetail): void {
    this.#worldDetail = detail;
    this.#grass?.setDetail(detail);
    this.#rootRegistry.markDirty();
  }

  setAudioPreferences(preferences: AudioPreferences): void {
    this.#audioPreferences = preferences;
    this.#sound?.setPreferences(preferences);
  }

  setLookInversion(invert: boolean): void {
    this.#orbit.setLookInversion(invert);
  }

  /** Applies the Settings look sensitivity to later camera drags. */
  setLookSensitivity(multiplier: number): void {
    if (Number.isFinite(multiplier) && multiplier > 0) {
      this.#lookSensitivity = multiplier;
    }
  }

  /**
   * Starts loading the player figure before any world is loaded (#855): the page calls this as soon as the module
   * can name the figure's rest clip, so the download runs beside the world build.
   */
  prefetchPlayer(restClip: (kind: ImportedModelKind) => string, look?: ViewerLook): void {
    if (!this.#loadedScenery) { this.#restClip = restClip; }
    // The selected figure and its look lead the download (#979).
    if (look) { this.#selectPlayer(look); }
    void this.#models.preload("Ranger");
  }

  /** Names `look` the player look the Ranger loads first (#979), starting its figure once the Ranger has started. */
  selectPlayer(look: ViewerLook): void {
    this.#models.selectPlayer(look.sex, look.classId ? CLASS_LOADOUTS[look.classId] : null, viewerHeadwear(look), viewerHair(look));
  }

  /** Names `look` the player look the Ranger loads first (#979); true once it can be drawn. */
  #selectPlayer(look: ViewerLook): boolean {
    return this.#models.playerLookReady(look.sex, look.classId ? CLASS_LOADOUTS[look.classId] : null, viewerHeadwear(look), viewerHair(look));
  }

  /**
   * Readies the loaded world's panels and region art. Its static scene (#526) is built part by part in idle time while
   * the selection screen shows (`prebuild`), or by the first frame if entry comes first.
   */
  load(world: ZoneWorld): void {
    const arena = world.arena ?? false;
    if (this.#loadedScenery === world.scenery && this.#loadedArena === arena) {
      return; // Re-selecting the same world (`refreshArenaWorld`) must not rebuild it.
    }
    // The previous zone's presentation releases everything it owns, once (#1005); its loads still in flight attach
    // nothing. Retained bundle resources stay, so a revisit reuses them.
    this.#zone.retire();
    const zone = this.#zone = new ZoneLifecycle();
    this.#units?.units.dispose();
    this.#loadedScenery = world.scenery;
    this.#loadedArena = arena;
    this.#loadedWorld = world;
    this.#rootRegistry.markDirty();
    this.#staticParts = 0;
    this.#restModels = null;
    this.#prebuild.reset();
    this.#avatarPrepared = false;
    this.#pendingKitArea = null;
    this.#picking.reset();
    const started = performance.now();
    this.#catalog = world.catalog;
    this.#zoneName = world.name ?? ZONE_NAME;
    this.#arena = world.arena ?? false;
    this.#bags.load(world.catalog);
    this.#character.load(world.catalog);
    this.#loot.load(world.catalog);
    this.#vendor.load(world.catalog);
    this.#quests.load(world.catalog, world.scenery.raidPrep?.lookout ?? null);
    const scenery = world.scenery.scenery;
    this.#areas = scenery.areas;
    this.#presentationFingerprint = scenery.presentationFingerprint;
    this.#reliefAt = world.scenery.reliefAt;
    this.#scene = null;
    this.#sceneryFrame = null;
    this.#ground = null;
    this.#grass = null;
    this.#sceneryBatches = null;
    this.#minimap.reset();
    this.#nature = zone.own(new PlacedKit(world.scenery.nature));
    this.#climbing = zone.own(new PlacedKit(world.scenery.climbing));
    this.#crag = zone.own(new PlacedKit(world.scenery.crag));
    this.#gather = zone.own(new PlacedKit(world.scenery.gather));
    this.#veins = world.scenery.veins;
    this.#showVeins([]);
    this.#kits = world.scenery.kits.map((source) => zone.own(new PlacedKit(source)));
    this.#placedKits = [
      { kit: this.#nature, source: world.scenery.nature, requested: false },
      { kit: this.#climbing, source: world.scenery.climbing, requested: false },
      { kit: this.#crag, source: world.scenery.crag, requested: false },
      { kit: this.#gather, source: world.scenery.gather, requested: false },
      ...this.#kits.map((kit, index) => ({ kit, source: world.scenery.kits[index]!, requested: false })),
    ];
    this.#restClip = (kind) => world.scenery.importedRestClip(kind);
    this.#inn = zone.own(new WalkInBuildings(world.scenery.buildings.presenters()));
    const fortEntry = (key: string) => key.startsWith("greyhaven.fort.");
    this.#outpost = zone.own(new KitModules(OUTPOST_BUNDLE, "Outpost kit", world.scenery.outpost.entries.filter((key) => !fortEntry(key)), world.scenery.outpost.placements.filter(({ key }) => !fortEntry(key)), { wear: true }));
    this.#fortifications = zone.own(new KitModules(`${import.meta.env.BASE_URL}authored/greyhaven-fort/`, "Greyhaven fortifications", world.scenery.outpost.entries.filter(fortEntry), world.scenery.outpost.placements.filter(({ key }) => fortEntry(key))));
    this.#settlement = zone.own(new KitModules(SETTLEMENT_BUNDLE, "settlement props", world.scenery.settlement.entries, world.scenery.settlement.placements));
    // The arena's fortress kit (#591) loads at once: the whole fortress is in view from the courtyard.
    this.#fortress = world.scenery.fortress ? zone.own(new CompositionKit(world.scenery.fortress)) : null;
    // Greyhaven's art streams in region by region (#240): the selection screen fetches the
    // starting area's pieces, and play fetches each area the player approaches.
    this.#ambient = zone.own(new AmbientAnimals(world.scenery.ambient));
    this.#regions = new RegionArt(world, this.#models, this.#inn, this.#outpost, this.#settlement, this.#ambient, () => this.#onArtProgress?.());
    this.#reachableDoor = world.scenery.buildings.reachableDoor;
    this.#reachableSeat = world.scenery.buildings.reachableSeat;
    this.#reachablePieces = world.scenery.reachablePieces;
    this.#raidArea = world.scenery.raidArea;
    this.#raidAid = world.scenery.raidAid;
    this.#raidPrep = world.scenery.raidPrep;
    this.#raidRoof = null;
    this.#waystone.load(world.scenery.waystoneAt, world.scenery.waystones);
    this.#units = null;
    this.#occluders = occluders(world);
    this.#buildMs = performance.now() - started;
    // The selected character's imported model owns the first network/decoder work, then the entry area's inn, which
    // Enter World waits for (#855). Once both settled (including a failed load), background kits can prepare while the
    // player selects a character or moves through creation. No world entry waits for them.
    const prewarmed = world.scenery;
    const fortress = this.#fortress;
    // The selected look leads (#979); the kits wait for the world's Rangers as they waited for the whole Ranger before.
    void this.#models.preloadWorld("Ranger").then(() => {
      if (this.#loadedScenery !== prewarmed || this.#loadedArena !== arena) { return; }
      this.#avatarPrepared = true;
      this.#loadSelectionKits();
      void fortress?.load();
    });
  }

  /**
   * The selected area's placed kits and the fortifications, once the player figure and the selected area's entry art
   * have settled (#855): started earlier, their downloads and decodes delay the inn Enter World waits for.
   */
  #loadSelectionKits(): void {
    const area = this.#pendingKitArea;
    if (!this.#avatarPrepared || !area || !(this.#regions?.entryReady(area.id) ?? false)) { return; }
    this.#loadKitsForArea(area);
    void this.#fortifications?.load();
  }

  /**
   * Prebuilds `world`'s entry while the selection screen shows (#855, owner decision 2026-11-23), one idle slice at a
   * time so the turntable and keys stay smooth: the static scene part by part, then the view at `entry` (the selected
   * character's expected entry; null builds only the static scene) composed as its first frame would compose it without
   * units, and once `prepare` allows it (the entry art is in) that view's programs and textures through the lighting's
   * passes. Enter World then joins into the built scene, and its `prepare` adds what the joined projection changes: the
   * units. A different character or slot composes and prepares again; the static scene stays. Cheap to call again with
   * the same arguments; nothing joins. The static scene starts once the worker's scene reached the page, stops before
   * its ground until `derived.far` says the terrain's far levels did too, and before its grass until `derived.grass` says
   * the grass's seed did; ask again then.
   */
  prebuild(world: ZoneWorld, entry: EntryPreview | null, prepare: boolean, derived: DerivedParts): void {
    if (world.scenery !== this.#loadedScenery || !this.#loadedWorld) { return; }
    this.#derived = derived;
    const key = entry && `${entry.feet.join(",")}|${entry.facing}|${wasm.zoneSkyHour(entry.tick, this.#animate)}`;
    // The turntable's first frame prepares its own programs first; a prebuild prepare meanwhile would swap the scene.
    this.#prebuild.request(entry, key, prepare && this.#previewPrepared !== "pending");
  }

  /**
   * What the turntable's frames draw besides the selection stage, hidden (#855): the prebuilt entry view's renderer
   * nodes and instance batches, so a selection frame evicts nothing Enter World would build and upload again. Null
   * before a prebuild prepared a view.
   */
  selectionKept(): { nodes: readonly RendererSceneNode[]; instanceBatches: readonly RendererInstanceBatch[] } | null {
    return this.#selectionKeep;
  }

  /** The prebuild's deterministic work, whether it settled, and static builds a world frame still ran (#855). */
  prebuildReport(): PrebuildWork & { settled: boolean; prepared: boolean; frameStaticBuilds: number; keptNodes: number; keptBatches: number } {
    return {
      ...this.#prebuild.work, settled: this.#prebuild.settled, prepared: this.#prebuild.prepared, frameStaticBuilds: this.#frameStaticBuilds,
      keptNodes: this.#selectionKeep?.nodes.length ?? 0, keptBatches: this.#selectionKeep?.instanceBatches.length ?? 0,
    };
  }

  /**
   * The placeholder frame of the view at `entry` (#855): the camera a session's first frame picks behind the character
   * standing there, and no units.
   */
  #entryFrame(world: ZoneWorld, scene: SceneryScene, sceneryFrame: SceneryFrame, entry: EntryPreview): WorldFrame {
    const { unitsPerMetre } = world.scenery.scenery;
    const [x, feetY, z] = entry.feet.map((value) => value / unitsPerMetre) as [number, number, number];
    const self: FrameSelf = { focus: [x, entry.centreY / unitsPerMetre, z], x, feetY, z, facing: entry.facing };
    const orbit = new OrbitCamera();
    let view: CameraView;
    try {
      this.#lookFirst(orbit, self.focus, self.facing, scene.surfaceY);
      view = orbit.view(self.focus, scene.surfaceY);
    } finally {
      orbit.dispose();
    }
    this.#camera.position.set(...view.eye);
    this.#camera.lookAt(...view.target);
    this.#camera.updateMatrixWorld(true);
    const frame = this.#prebuildFrame;
    frame.world = world;
    frame.scene = scene;
    frame.sceneryFrame = sceneryFrame;
    frame.deltaSeconds = 0;
    frame.animate = this.#animate;
    frame.tick = entry.tick;
    frame.self = self;
    frame.view = view;
    frame.unitNodes = NO_NODES;
    frame.effectNodes = NO_NODES;
    return frame;
  }

  /** Runs the scenery stage over the placeholder frame at `entry`, leaving the page's last drawn camera as it was. */
  #composeEntryFrame(entry: EntryPreview): WorldFrame | null {
    const world = this.#loadedWorld;
    const scene = this.#scene;
    const sceneryFrame = this.#sceneryFrame;
    if (!world || !scene || !sceneryFrame) { return null; }
    const frame = this.#entryFrame(world, scene, sceneryFrame, entry);
    const lastCamera = this.#lastCamera;
    this.#loadKitsNearPoint([entry.feet[0], entry.feet[2]]);
    this.#sceneryStage(frame);
    // `enter` redraws the roots with the last drawn camera; the placeholder's must not count as drawn.
    this.#lastCamera = lastCamera;
    return frame;
  }

  /** One prebuild compose round at `entry`: static groups, ground tiles and grass stream in; true once none did. */
  #composeEntry(entry: EntryPreview): boolean {
    const before = this.#streamed();
    if (!this.#composeEntryFrame(entry)) { return true; }
    return this.#streamed() === before;
  }

  /** How much of the static scene has streamed in around the eye: resident groups, near ground tiles and grass cells. */
  #streamed(): string {
    return `${this.#sceneryBatches?.observe().resident ?? 0}|${this.#ground?.observe().resident ?? 0}|${this.#grass?.observe().cells ?? 0}`;
  }

  /** What a prebuild prepare covers besides its entry: whether the lighting has loaded, and the render roots. */
  #entryPrepareKey(): string {
    if (this.#rootRegistry.refresh()) {
      this.#rootsStale = true;
      this.#rootsVersion += 1;
    }
    return `${this.#lighting.ready}|${this.#rootsVersion}`;
  }

  /**
   * Prepares the placeholder frame at `entry` like Enter World's `prepare` (#547): its programs through the lighting's
   * passes, with the lighting the first frame will have, and its textures, drawing nothing.
   */
  async #prepareEntry(entry: EntryPreview): Promise<void> {
    const frame = this.#composeEntryFrame(entry);
    if (!frame) { return; }
    // The renderer evicts whatever a frame leaves out. The prepare keeps the selection stage's nodes, hidden, and the
    // turntable's frames keep the prepared view's nodes and instance batches, hidden, until Enter World draws them.
    this.#selectionKeep = {
      nodes: frame.nodes.map((node) => ({ ...node, visible: false })),
      instanceBatches: frame.sceneFrame.instanceBatches.map((batch) => ({ ...batch, visible: false })),
    };
    appendAll(frame.nodes, this.#selectionStage);
    const pending: Promise<unknown>[] = [];
    this.#preparing = pending;
    try {
      this.#renderStage(frame);
    } finally {
      this.#preparing = null;
    }
    // The prepare put the world's roots in the renderer's scene; the turntable submits its own again before it draws.
    this.#previewStale = true;
    await Promise.all(pending);
    markStartup("prebuild-prepared");
  }

  /** Prepare only the authored kits around the selected area, including the Outpost cliff rim. */
  #loadKitsForArea(area: Area): void {
    const units = this.#loadedScenery?.scenery.unitsPerMetre;
    if (!units) { return; }
    for (const entry of this.#placedKits) {
      if (!entry.requested && kitNearArea(entry.source, area, units, 5_000)) {
        entry.requested = true;
        void entry.kit.load();
      }
    }
  }

  /** The ordinary placed kit of the shared inventory (#683) called `name`, if the zone has it. */
  #inventoryKit(name: string): PlacedKit | null {
    return this.#placedKits.find(({ source }) => source.bundle && source.kit === name)?.kit ?? null;
  }

  /** Whether an inventory kit that replaces stand-in `name` draws (#1011): the static scene then hides that fallback. */
  #standInDrawn(name: keyof DrawnKits): boolean {
    return this.#placedKits.some(({ kit, source }) => source.standIn === name && kit.drawn);
  }

  /** The terrain already screens distant placed kits; stream new categories on approach. */
  #loadKitsNearPoint(at: readonly [number, number]): void {
    if (!this.#avatarPrepared || shelteredOutpost(at, this.#areas)) { return; }
    const units = this.#loadedScenery?.scenery.unitsPerMetre;
    if (!units) { return; }
    for (const entry of this.#placedKits) {
      if (!entry.requested && kitNearPoint(entry.source, at, units, STREAM_RADIUS_UNITS)) {
        entry.requested = true;
        void entry.kit.load();
      }
    }
  }

  /**
   * Decodes the static scene and places its always-resident parts (#526): far terrain, animated parts and the batches
   * the groups stream into. Nothing far is decoded or uploaded here; [`BatchedScenery`] streams groups from the first frame.
   */
  #buildStatic(world: ZoneWorld): void {
    while (this.#staticParts < STATIC_PARTS) { this.#buildStaticPart(world); }
  }

  /**
   * Builds the loaded world's static scene up to its ground (#855): Enter World clicked before the prebuild finished
   * builds these while the worker still packs the terrain's far levels.
   */
  buildStaticBeforeGround(world: ZoneWorld): void {
    this.#buildStaticBefore(world, GROUND_PART);
  }

  /**
   * Builds the loaded world's static scene up to its grass (#855): Enter World clicked before the prebuild finished
   * builds these while the worker still derives the grass, which the world's prepare then adds.
   */
  buildStaticBeforeGrass(world: ZoneWorld): void {
    this.#buildStaticBefore(world, GRASS_PART);
  }

  #buildStaticBefore(world: ZoneWorld, part: number): void {
    if (world.scenery !== this.#loadedScenery || !this.#loadedWorld) { return; }
    while (this.#staticParts < part) { this.#buildStaticPart(world); }
  }

  /**
   * Builds the static scene's next part (`STATIC_PARTS` in all): the page's module realizing its seeded scene (its
   * terrain tiles, on the first read of the rest transforms), the decoded scene, the batches its groups stream into, the
   * ground (its far levels as the worker packed them, once seeded), the grass. The selection screen's prebuild builds one
   * per idle slice (#855).
   */
  #buildStaticPart(world: ZoneWorld): void {
    const part = this.#staticParts;
    this.#staticParts += 1;
    const content = world.scenery.staticScene;
    if (part === 0) {
      this.#restModels = content.staticSceneMotion(0, false);
    } else if (part === 1) {
      const scene = buildSceneryScene(world.scenery.scenery, content, this.#restModels ?? undefined);
      this.#restModels = null;
      this.#scene = scene;
      this.#sceneryFrame = new SceneryFrame(scene);
    } else if (part === 2) {
      this.#sceneryBatches = this.#zone.own(new BatchedScenery(this.#scene!, content.groupStream()));
      this.#rootRegistry.markDirty();
    } else if (part === GROUND_PART) {
      this.#ground = this.#zone.own(new Ground(content.terrainStream(), this.#scene!.surfaces, this.#scene!.cutaways));
      void this.#ground.load(this.#models.ktx2Loader);
    } else if (part === GRASS_PART) {
      this.#grass = content.grassCell && content.grassCellMetres
        ? new Grass({
          cell: (column, row) => content.grassCell!(column, row), cellMetres: content.grassCellMetres(),
          ...(content.grassFarCell && content.grassFarCellMetres
            ? { farCell: (column: number, row: number) => content.grassFarCell!(column, row), farCellMetres: content.grassFarCellMetres() }
            : {}),
        }, this.#worldDetail)
        : null;
      if (this.#grass) { this.#zone.own(this.#grass); }
      markStartup("static-built");
    }
  }

  /** The directed-strike action's working binding labels, for the Heroic Strike slot hint. */
  setDirectedBinding(labels: readonly string[]): void {
    this.#classHud.setDirectedBinding(labels);
  }

  /** The directed-strike action was pressed: start aiming when the viewer is a living Warden. */
  beginDirectedStrike(): void {
    const projection = this.#lastProjection;
    if (!projection || !this.#catalog || !canStrikeDirected(projection, this.#catalog)) {
      return;
    }
    // The most recent press owns the mouse (Atlas 5.2.37): the sword lowers a held guard. An aim or
    // wheel keeps the mouse, and the press does nothing.
    const begin = this.#mouse.beginHold("strike");
    if (!begin.started) { return; }
    this.#gestureEnded(begin.preempted);
    this.#powerDraw.cancel();
    this.#updatePointerOwner();
    this.#classHud.showStrikeAim(this.#mouse.aim().strike);
    // The sword winds up while the player aims (Atlas 6.6.10), toward the aimed slash (#582);
    // only the zone's strike swings it.
    this.#units?.units.prepare(HEROIC_STRIKE_ABILITY, this.#seconds);
    this.#aimWindUp();
  }

  /** The held directed strike's wind-up shows the slash toward its current aim (#582). */
  #aimWindUp(): void {
    const aim = this.#mouse.aim().strike;
    if (aim) {
      this.#units?.units.aimPreparation(aim);
    }
  }

  /** A deliberate release sends one directed strike toward the view's heading; the zone resolves it. */
  releaseDirectedStrike(): void {
    const aim = this.#mouse.aim().strike;
    const fires = this.#mouse.releaseHold("strike");
    this.#classHud.showStrikeAim(null);
    this.#updatePointerOwner();
    const catalog = this.#catalog;
    if (!fires || !aim || !catalog) {
      // A cancelled, preempted or interrupted hold's key: nothing fires (5.2.37).
      if (!this.#mouse.gesture) { this.#units?.units.abandonPreparation(); }
      return;
    }
    const facing = this.#orbit.facing();
    // The Warden turns to strike; later movement keeps that heading until steered.
    this.#movementFacing = facing;
    this.#queuePrepared((projection) => directedStrikeCommand(projection, catalog, facing, aim));
  }

  /** Blur, menus, retired input: no strike, and no wind-up. */
  cancelDirectedStrike(): void {
    if (this.#mouse.owner() === "strike") {
      this.#gestureEnded(this.#mouse.interrupt());
    }
  }

  /** A gesture ended without firing (#636): its aim display, wind-up or raised guard goes. */
  #gestureEnded(ended: EndedGesture): void {
    if (ended === "strike") {
      this.#classHud.showStrikeAim(null);
      this.#units?.units.abandonPreparation();
    } else if (ended === "aim") {
      this.#units?.units.abandonPreparation();
      this.#aimEnded();
    } else if (ended === "guard") {
      this.#lowerGuard();
    }
    this.#updatePointerOwner();
  }

  /** Ends whatever gesture owns the mouse without firing: a menu, a panel, focus or lock loss. */
  interruptGesture(): void {
    this.#gestureEnded(this.#mouse.interrupt());
  }

  /** Follows the Settings choice of the camera during a strike or guard hold (#636). */
  setHoldCamera(value: "locked" | "free"): void {
    this.#mouse.setHoldCamera(value);
  }

  /** Follows the Settings choice of how a manual aim throws (#649). */
  setAimStyle(value: "hold" | "armed"): void {
    this.#mouse.setAimStyle(value);
  }

  /**
   * Follows Settings → Wheels (#653): wheels on, the fixed wheel's sectors, and the opt-in usage
   * counters, kept only in this browser's local storage under `WHEEL_USAGE_KEY`.
   */
  setWheels(wheels: WheelPreferences): void {
    if (wheels.countUsage !== this.#wheels.countUsage) {
      if (wheels.countUsage) {
        this.#mouse.setUsageCounting(readWheelUsage() ?? "");
      } else {
        this.#saveWheelUsage(this.#mouse.countUsage(0));
        this.#mouse.setUsageCounting(null);
      }
    }
    this.#wheels = wheels;
    if (!wheels.enabled && this.#mouse.owner() === "wheel") { this.interruptGesture(); }
  }

  /** A wheel key went down (#653): with wheels on, the fixed or context wheel takes the mouse. */
  pressWheel(kind: WheelKindName): void {
    const projection = this.#lastProjection;
    if (!this.#wheels.enabled || !projection) { return; }
    if (this.#mouse.openWheel(kind, this.#wheels.sectors, projection)) {
      this.#powerDraw.cancel();
      this.#wheelPressedAt = performance.now();
    }
    this.#updatePointerOwner();
  }

  /** The wheel key went up (or was `retired`): a flick runs its action; a tap keeps it open. */
  releaseWheel(retired: boolean): void {
    const release = this.#mouse.releaseWheel(performance.now() - this.#wheelPressedAt, retired);
    if (release.outcome === "pick" && release.action !== null) { this.onWheelAction?.(release.action); }
    this.#updatePointerOwner();
  }

  /** The open wheel (#653) over the canvas centre, its pointed-at sector lit; hidden otherwise. */
  #showWheel(): void {
    const projection = this.#lastProjection;
    const view = projection ? this.#mouse.wheel(projection) : null;
    if (!view) {
      this.#wheelElement?.setAttribute("hidden", "");
      this.#wheelShown = null;
      return;
    }
    if (!this.#wheelElement) {
      this.#wheelElement = document.createElement("div");
      this.#wheelElement.className = "pie-wheel";
      this.#wheelElement.setAttribute("aria-hidden", "true");
      (this.#elements.canvas.parentElement ?? document.body).append(this.#wheelElement);
    }
    const wheel = this.#wheelElement;
    const shown = JSON.stringify(view.sectors);
    if (shown !== this.#wheelShown) {
      this.#wheelShown = shown;
      const radius = 118;
      wheel.replaceChildren(...view.sectors.map((sector, index) => {
        const turn = (index / view.sectors.length) * Math.PI * 2;
        const label = document.createElement("span");
        label.className = "pie-wheel-sector";
        label.textContent = sector?.label ?? "—";
        label.toggleAttribute("data-empty", !sector);
        label.style.transform = `translate(-50%, -50%) translate(${Math.sin(turn) * radius}px, ${-Math.cos(turn) * radius}px)`;
        return label;
      }));
    }
    const rect = this.#elements.canvas.getBoundingClientRect();
    wheel.style.left = `${rect.left + rect.width / 2}px`;
    wheel.style.top = `${rect.top + rect.height / 2}px`;
    wheel.dataset.kind = view.kind;
    [...wheel.children].forEach((child, index) => child.classList.toggle("lit", index === view.highlight));
    wheel.removeAttribute("hidden");
  }

  /** Stores the counters' JSON at most every 10 s, or at once with `now` 0; local only. */
  #saveWheelUsage(json: string | undefined, now = 0): void {
    if (json === undefined || (now !== 0 && now - this.#usageSavedAt < 10_000)) { return; }
    this.#usageSavedAt = now;
    try {
      localStorage.setItem(WHEEL_USAGE_KEY, json);
    } catch {
      // Storage refused (private mode, quota): the counters keep counting in memory.
    }
  }

  /**
   * One weapon swing where the character faces (#517): Q, gamepad X or the touch Attack pill (the
   * left mouse button selects since #636). The zone resolves what it hits; the dead swing nothing.
   */
  swing(): void {
    if (!this.#lastProjection || this.#lastProjection.viewer.dead) {
      return;
    }
    this.#powerDraw.cancel();
    this.queueIntent(() => ({ kind: "swing", facing: this.#movementFacing }));
  }

  /**
   * Uses action-bar slot `slot` (1–20); the viewer winds up at once while the zone decides. The
   * command carries the camera's heading (#884), so a cast after orbiting the camera fires where
   * the player looks rather than along the facing the last move left.
   */
  useSlot(slot: number): void {
    this.#powerDraw.cancel();
    this.#queuePrepared((projection) => (this.#catalog ? useAbilitySlot(slot, projection, this.#catalog, this.#aimFacing()) : null));
  }

  /** The heading an ability fires along (#884): the camera's; the character keeps it afterwards. */
  #aimFacing(): number {
    this.#movementFacing = this.#orbit.facing();
    return this.#movementFacing;
  }

  /**
   * A held input (a key, pad button or touch face) pressed `slot`: a power shot draws and waits for
   * the release (#607, whichever slot holds it); anything else is used at once. An empty slot does
   * nothing.
   */
  pressSlot(slot: number): void {
    const projection = this.#lastProjection;
    const ability = projection && this.#catalog ? abilitySlots(projection, this.#catalog)[slot - 1] : undefined;
    if (!projection || !ability) {
      return;
    }
    // The bomb takes a temporary shoulder-camera aim (#649); a press while another gesture owns the
    // mouse does nothing.
    const aim = this.#mouse.pressAim(slot, ability.id, projection, this.panelOpen);
    if (aim !== "use") {
      if (aim === "aim") { this.#beginAim(ability.id); }
      return;
    }
    if (!this.#powerDraw.press(slot, ability.id, projection, this.panelOpen)) {
      this.useSlot(slot);
    }
  }

  /** `slot`'s held input went up; a deliberate release sends the drawn shot or the aimed throw once. */
  releaseSlot(slot: number, retired: boolean): void {
    const aiming = this.#mouse.owner() === "aim";
    const thrown = this.#mouse.releaseAim(slot, retired);
    if (thrown !== undefined) {
      this.#throwAimed(thrown);
    } else if (aiming && this.#mouse.owner() !== "aim") {
      this.#gestureEnded("aim");
    }
    this.#powerDraw.release(slot, retired);
  }

  /**
   * A manual aim began (#649): the mouse steers the same shoulder camera, which leaves the focused
   * target (still selected) until the aim ends. Captured motion keeps the cursor from the edges.
   */
  #beginAim(ability: number): void {
    this.#powerDraw.cancel();
    this.#units?.units.prepare(ability, this.#seconds);
    if (document.pointerLockElement !== this.#elements.canvas) {
      void this.#elements.canvas.requestPointerLock().catch(() => {});
    }
    this.#updatePointerOwner();
  }

  /** Throws the aimed `ability` along the camera's heading; the zone decides its flight and hits. */
  #throwAimed(ability: number): void {
    this.#movementFacing = this.#orbit.facing();
    const projection = this.#lastProjection;
    const index = projection && this.#catalog
      ? abilitySlots(projection, this.#catalog).findIndex((held) => held?.id === ability) : -1;
    if (index >= 0) { this.useSlot(index + 1); }
    this.#aimEnded();
  }

  /** The aim's landing marker goes, and a focus gets its free cursor back. */
  #aimEnded(): void {
    this.#aimLanding?.setAttribute("hidden", "");
    if (this.#cameraMode.kind === "focused") { this.#releasePointerLock(); }
    this.#updatePointerOwner();
  }

  /** The aim's expected landing (#649), drawn over the ground ahead: a non-authoritative aid. */
  #showAimLanding(projection: ZoneSnapshot, unitsPerMetre: number): void {
    if (this.#mouse.owner() !== "aim") { return; }
    const landing = this.#mouse.aimLanding(projection, this.#orbit.facing());
    if (!landing) { return; }
    if (!this.#aimLanding) {
      this.#aimLanding = document.createElement("span");
      this.#aimLanding.className = "aim-landing";
      this.#aimLanding.setAttribute("aria-hidden", "true");
      (this.#elements.canvas.parentElement ?? document.body).append(this.#aimLanding);
    }
    const [x, y, z] = landing;
    const relief = this.#reliefAt?.(x, z) ?? 0;
    const point = new THREE.Vector3(x / unitsPerMetre, (y + relief) / unitsPerMetre, z / unitsPerMetre).project(this.#camera);
    const rect = this.#elements.canvas.getBoundingClientRect();
    this.#aimLanding.style.left = `${rect.left + ((point.x + 1) / 2) * rect.width}px`;
    this.#aimLanding.style.top = `${rect.top + ((1 - point.y) / 2) * rect.height}px`;
    this.#aimLanding.toggleAttribute("hidden", point.z > 1);
  }

  /** Escape drops a held draw before any other meaning; `true` if it did. */
  cancelPowerDraw(): boolean {
    return this.#powerDraw.cancel();
  }

  /**
   * Queues an attack whose local wind-up (Atlas 6.6.10) starts as it is built and waits for the
   * zone's answer once sent. Nothing to send lets the wind-up go. Presentation only.
   */
  #queuePrepared(intent: Intent): void {
    this.queueIntent((projection) => {
      const command = intent(projection);
      const units = this.#units?.units;
      if (command?.kind === "use-ability" || command?.kind === "directed-strike") {
        units?.prepare(command.ability, this.#seconds);
        if (command.kind === "directed-strike") {
          units?.aimPreparation(command.aim);
        }
      } else {
        units?.abandonPreparation();
      }
      return command;
    }, () => this.#units?.units.preparedSent(this.#seconds));
  }

  /**
   * The guard action was pressed (#156): a living Warden raises the shield in the middle lane, and
   * mouse motion moves it. The press supersedes an unreleased strike preparation, which never fires.
   */
  beginGuard(): void {
    const projection = this.#lastProjection;
    if (!projection || !canGuard(projection)) {
      return;
    }
    const begin = this.#mouse.beginHold("guard");
    if (!begin.started) { return; }
    this.#gestureEnded(begin.preempted);
    this.#powerDraw.cancel();
    this.#guardHold.begin();
    this.#updatePointerOwner();
    this.#sendGuardLane();
  }

  /** A release or any retirement lowers the guard the zone holds. */
  endGuard(): void {
    if (this.#mouse.owner() === "guard") {
      this.#mouse.releaseHold("guard");
    }
    this.#lowerGuard();
  }

  #lowerGuard(): void {
    if (this.#guardHold.end()) {
      this.queueIntent(() => guardCommand(null));
    }
    this.#updatePointerOwner();
  }

  #sendGuardLane(): void {
    const lane = this.#guardHold.pending();
    if (lane !== undefined) {
      this.queueIntent(() => guardCommand(lane));
    }
  }

  /** Starts a session: fresh camera, intent and animation state. */
  enter(input: WorldInput, projection: ZoneSnapshot | null): void {
    // Whatever the selection screen prebuilt stays (#855); nothing more is built for it in the world.
    this.#prebuild.pause();
    this.#selectionKeep = null;
    this.interruptGesture();
    this.#guardHold.end();
    this.#orbit.dispose();
    this.#orbit = new OrbitCamera();
    this.#cameraMode = { kind: "free-look" };
    this.#updatePointerOwner();
    // Keep the zone's facing: the first intent must not turn a character that entered facing elsewhere.
    const own = projection?.entities.find((entity) => entity.kind === "player" && entity.entityId === projection.viewerId);
    this.#movementFacing = own?.facing ?? this.#orbit.facing();
    this.#endDrag();
    this.#outbox.reset(this.#input(input));
    this.#resetUnits();
    this.#clearModels();
    this.#intents.length = 0;
    this.#combatHud.reset();
    this.#progressionHud.reset();
    this.#classHud.reset();
    this.#bags.reset(projection);
    this.#character.reset(projection);
    this.#loot.reset(projection);
    this.#vendor.reset(projection);
    this.#quests.reset(projection);
    this.#exploration.reset();
    this.#chat.reset();
    this.#party.reset();
    this.#depthsLoot.reset();
    this.#shownArea = null;
    this.#flyTo = null;
    this.#framePending = true;
    this.#lastSelf = null;
    this.#worldMap?.close();
    this.#lastProjection = null;
    this.#lastSceneryNodes = [];
    this.#lastInstanceBatches = [];
    this.#lastEnvironment = null;
    this.#lastWork = null;
    this.#presentationTime = null;
    this.#lastUnitNodes = [];
    this.#lastEffectNodes = [];
    this.#pendingNpc = null;
    this.#frameRate.reset();
    // The CSS sky stays hidden until the first world frame's HUD stage says the atmosphere cannot draw the sky (#855):
    // shown here, its drifting clouds were repainted behind the selection screen for the whole entry.
  }

  /**
   * Runs the entered world's first frame without drawing it and resolves once the renderer has
   * compiled its programs and uploaded its textures (#547), so the first drawn world frame does
   * not stall for most of a second. Call it after `enter` and draw nothing else until it resolves.
   */
  async prepare(world: ZoneWorld, input: WorldInput, look: ViewerLook, now: number): Promise<void> {
    const pending: Promise<unknown>[] = [];
    this.#preparing = pending;
    try {
      this.frame(world, input, look, 0, now);
    } finally {
      this.#preparing = null;
    }
    await Promise.all(pending);
  }

  /**
   * Starts behind the character, or turned in eighth turns until the sight
   * line clears every structure. The camera itself never collides; this only
   * frames the first view of a session, deterministically.
   */
  #lookFirst(orbit: OrbitCamera, focus: Vec3, facing: number, groundAt: (x: number, z: number) => number): void {
    for (const step of [0, 2, -2, 1, -1, 3, -3, 4]) {
      orbit.lookAlong(facing + (step * Math.PI) / 4);
      const view = orbit.view(focus, groundAt);
      if (!obstructed(this.#occluders, view.target, view.eye)) {
        return;
      }
    }
    orbit.lookAlong(facing);
  }

  /** Drops every imported unit model and takes them out of the shared renderer scene. */
  #clearModels(): void {
    this.#models.clear();
    if (this.#lastCamera && this.#rootRegistry.refresh()) {
      const roots = this.#rootRegistry.roots;
      this.#renderer.renderImportedRoots({ camera: this.#lastCamera, roots: roots.slice() });
      this.#submittedRoots = new Set(roots);
      this.#rootsStale = false;
    }
  }

  leave(): void {
    this.cancelPointer();
    this.#cameraMode = { kind: "free-look" };
    this.interruptGesture();
    this.#impacts.clear();
    this.#guardHold.end();
    this.#raidSmoke.clear();
    this.#raidSigns.clear();
    this.#pendingNpc = null;
    this.#clearModels();
    this.#lastSceneryNodes = [];
    this.#lastInstanceBatches = [];
    this.#lastEnvironment = null;
    this.#lastWork = null;
    this.#presentationTime = null;
    this.#lastUnitNodes = [];
    this.#lastEffectNodes = [];
    this.#classHud.reset();
    this.#resetUnits();
    this.#bags.reset();
    this.#character.reset();
    this.#loot.reset();
    this.#vendor.reset();
    this.#quests.reset();
    this.#chat.reset();
    this.#party.reset();
    this.#depthsLoot.reset();
    this.#panelGate.reset();
    this.#endDrag();
    this.#sky.show(false);
    this.#overlay.hide();
  }

  /**
   * The selection turntable (#362): the real Ranger in its idle clip standing at `feet`, dressed as
   * the viewer would enter (class loadout, headwear, hair), through the same imported-model path as
   * the world. Absent before a world loaded, while the model loads or after it failed; the page then
   * keeps its stage veiled (see `selectionPreviewStatus`). Entering the world drops it like any unit
   * no longer projected. Its first frame is prepared with the `stage` nodes (#855: the programs compile in parallel
   * instead of stalling the page for seconds on its first draw). False while that runs: the caller then draws nothing,
   * which would evict what is being prepared.
   */
  selectionPreview(look: ViewerLook, feet: [number, number, number], yaw: number, height: number, seconds: number,
    camera: RendererCamera, stage: RendererSceneNode[]): boolean {
    if (this.#selectionStage.length !== stage.length) { this.#selectionStage = stage.map((node) => ({ ...node, visible: false })); }
    if (!this.#regions || !this.#selectPlayer(look)) { return true; }
    const clip = this.#restClip("Ranger");
    const length = this.#models.clipLength("Ranger", clip) ?? 1;
    // Reduced motion holds the idle pose, as it does for the procedural figure.
    const time = this.#animate ? seconds % length : 0;
    this.#models.update([{
      id: "selection-preview", kind: "Ranger", entity: { kind: "player", id: 0 }, feet, yaw, pitch: 0, heightMetres: height,
      clip, time, lower: [clip, time], layers: [{ clip, time, weight: 1, mask: "all" }], tint: null,
      loadout: look.classId ? CLASS_LOADOUTS[look.classId] : null, resident: null, figure: null, variety: null,
      outfit: look.classId ? { preset: `class.${look.classId}`, replaced: [] } : null, headwear: true, swing: null, chains: null, bow: null,
      sheathed: [false, false], throwable: null, belted: null, crossbow: null, channel: null,
      weaponDrawn: false,
    }], { headwear: viewerHeadwear(look), hair: viewerHair(look), face: look.face ?? {}, physique: look.physique ?? null, sex: look.sex });
    const { roots, changed } = this.#models.roots();
    if (this.#previewPrepared === "no") {
      this.#previewPrepared = "pending";
      void this.#renderer.prepare({ camera, nodes: stage }, { roots }).catch((error: unknown) => {
        console.warn("Preparing the selection preview failed; it draws unprepared.", error);
      }).finally(() => { this.#previewPrepared = "ready"; });
      return false;
    }
    if (this.#previewPrepared === "pending") {
      this.#previewStale ||= changed;
      return false;
    }
    if (changed || this.#previewStale) {
      this.#previewStale = false;
      this.#renderer.renderImportedRoots({ camera, roots });
    }
    return true;
  }

  /**
   * How far the selection turntable's real character is: the Ranger `loading`, `failed` for good, or
   * `ready`, with whether the class outfit `look` wears can be drawn yet (it pops in otherwise).
   */
  selectionPreviewStatus(look: ViewerLook): { model: "loading" | "ready" | "failed"; outfit: boolean } {
    if (!this.#regions) { return { model: "loading", outfit: false }; }
    const status = this.#models.playerStatus(look.sex);
    // Ready once drawn: not while its first frame is still being prepared.
    const model = status === "ready" && this.#previewPrepared !== "ready" ? "loading" : status;
    return { model, outfit: model === "ready" && (!look.classId || this.#models.playerOutfitReady(`class.${look.classId}`, look.sex)) };
  }

  /** The face features the loaded Ranger offers (#411): undefined while loading, null if it has none. */
  appearanceOptions(sex: "male" | "female") {
    return this.#models.appearanceOptions(sex);
  }

  physiqueControls(sex: "male" | "female"): readonly string[] {
    return this.#models.physiqueControls(sex);
  }

  faceFeatures(sex: "male" | "female" = "male"): readonly FaceFeature[] | null | undefined {
    return this.#models.faceFeatures(sex);
  }

  /** Sends a party request; a refusal or a world without a party service becomes a chat note. */
  #sendParty(request: PartyRequestInput): void {
    const send = this.#partySource?.sendParty?.bind(this.#partySource);
    if (!send) {
      this.#chat.note("Parties need other players online.");
      return;
    }
    send(request).then((refusal: string | null) => {
      if (refusal) { this.#chat.note(refusal); }
    }, (error: unknown) => {
      this.#chat.note(`Party request failed: ${error instanceof Error ? error.message : String(error)}`);
    });
  }

  /** Queues an intent for the next frame; the zone decides what happens. */
  queueIntent(intent: Intent, onSent?: (sequence: number | null) => void): void {
    this.#intents.push({ intent, onSent });
  }

  get weaponDrawn(): boolean {
    return this.#lastProjection?.viewer.weaponDrawn ?? false;
  }

  /** Explicitly enter/leave combat mode; the zone owns the resulting state (#802). */
  toggleWeapon(): void {
    const projection = this.#lastProjection;
    if (!projection || projection.viewer.dead) { return; }
    this.interruptGesture();
    this.#powerDraw.cancel();
    this.#lowerGuard();
    const drawn = !projection.viewer.weaponDrawn;
    this.queueIntent(() => ({ kind: "set-weapon-drawn", drawn }));
  }

  #requireFreeHands(): boolean {
    if (!this.weaponDrawn) { return true; }
    this.#chat.note("Sheathe your weapon to interact (H).");
    return false;
  }

  toggleBags(): void {
    this.#loot.close(false);
    this.#vendor.close(false);
    this.#closeQuests();
    this.#bags.toggle();
  }

  toggleCharacter(): void {
    this.#loot.close(false);
    this.#vendor.close(false);
    this.#closeQuests();
    this.#character.toggle();
  }

  #closeQuests(): void {
    this.#quests.closeDialog(false);
    this.#quests.closeLog(false);
  }

  toggleVendor(): void {
    if (!this.#requireFreeHands()) { return; }
    this.#vendor.toggle();
  }

  closeVendor(): boolean { return this.#vendor.close(); }

  /** Talks to the selected or nearest quest giver: the NPC dialog. */
  toggleQuestDialog(): void {
    if (!this.#requireFreeHands()) { return; }
    this.#quests.toggleDialog();
  }

  /**
   * Interact (E / gamepad Start): inspect quest evidence in reach (#1019); talk to a selected NPC;
   * without one, swing the door in reach
   * (#158), repair a damaged structure piece in reach (#305, free; moving stops it), or help a
   * lost night raid's aftermath in reach (#531);
   * otherwise open the quest dialog, which says whom to approach.
   */
  interact(): void {
    if (!this.#requireFreeHands()) { return; }
    const projection = this.#lastProjection;
    // The viewer leads every projection; positions are in units.
    const at = projection?.entities[0]?.position;
    const talking = projection?.viewer.target?.kind === "npc";
    // On a forewarned evening: read the tracks, report them to the lookout (even while talking
    // to him) or brace the barn in reach (#532).
    const step = at ? this.#prepStep(projection, at) : null;
    if (step !== null) {
      this.queueIntent(() => ({ kind: "raid-prepare", step }));
      return;
    }
    // Inspect the quest evidence in reach an active quest needs (#1019), even beside its giver.
    const spot = at ? evidenceInReach(this.#quests.state.sheet, at) : null;
    if (spot !== null) {
      this.queueIntent(() => ({ kind: "inspect", spot }));
      return;
    }
    const door = at && !talking ? this.#reachableDoor(at[0], at[2]) : 0;
    if (door !== 0) {
      this.queueIntent((latest) => doorToggle(latest, door));
      return;
    }
    // Or take (or leave) the seat in reach (#619).
    const seat = at && !talking ? this.#reachableSeat(at[0], at[2]) : 0;
    if (seat !== 0) {
      this.queueIntent(() => ({ kind: "sit", seat }));
      return;
    }
    const damaged = new Set(projection?.pieces.map((view) => view.piece) ?? []);
    const piece = at && !talking ? this.#reachablePieces(at[0], at[2]).find((id) => damaged.has(id)) : undefined;
    if (piece !== undefined) {
      this.queueIntent(() => ({ kind: "repair", piece }));
      return;
    }
    // Or tend a guard or mend a breach a lost night raid left in reach (#531).
    const task = at && !talking ? reachableAidTask(projection?.raid, this.#raidAid, at[0], at[2]) : null;
    if (task !== null) {
      this.queueIntent(() => ({ kind: "raid-aid", task }));
      return;
    }
    // Or skin the selected boar corpse, or mine the full ore vein in reach (#917).
    const corpse = at ? this.#skinnableCorpse(projection, at) : null;
    if (corpse !== null) {
      this.queueIntent(() => ({ kind: "skin", creatureId: corpse }));
      return;
    }
    const node = at && !talking ? this.#reachableVein(projection, at) : null;
    if (node !== null) {
      this.queueIntent(() => ({ kind: "gather", node }));
      return;
    }
    this.toggleQuestDialog();
  }

  /** Draws each ore vein full, or spent when the projection lists it (#917). */
  #showVeins(spent: readonly number[]): void {
    this.#veins.forEach((vein, index) => {
      const isSpent = spent.includes(index);
      this.#gather?.setHidden(vein.full, isSpent);
      this.#gather?.setHidden(vein.spent, !isSpent);
    });
  }

  /** The full ore vein (#917) feet at `at` units can mine, or null. */
  #reachableVein(projection: ZoneSnapshot | null, at: readonly number[]): number | null {
    const index = this.#veins.findIndex((vein, node) => !projection?.depletedVeins.includes(node) && (vein.x - at[0]!) ** 2 + (vein.z - at[2]!) ** 2 <= zoneConstants().gatherReachUnits ** 2);
    return index < 0 ? null : index;
  }

  /** The selected boar corpse (#917) feet at `at` units can skin, or null; the zone checks the rest. */
  #skinnableCorpse(projection: ZoneSnapshot | null, at: readonly number[]): number | null {
    const target = projection?.viewer.target;
    if (target?.kind !== "creature") { return null; }
    const corpse = projection?.entities.find((entity) => entity.kind === "creature" && entity.entityId === target.id);
    if (!corpse?.flags.dead || this.#catalog?.creatureTemplates.get(corpse.appearance)?.family !== "boar") { return null; }
    const distance = corpse.position.reduce((sum, axis, index) => sum + (axis - at[index]!) ** 2, 0);
    return distance <= zoneConstants().lootReachUnits ** 2 ? target.id : null;
  }

  /** The night raid preparation step (#532) feet at `at` units can take now, or null. */
  #prepStep(projection: ZoneSnapshot | null, at: readonly number[]): number | null {
    const lookout = this.#raidPrep && projection?.entities.find((entity) => entity.kind === "npc" && entity.entityId === this.#raidPrep!.lookout);
    return reachablePrepStep(projection?.raid, this.#raidPrep, at[0]!, at[2]!, lookout ? { x: lookout.position[0], z: lookout.position[2] } : null);
  }

  toggleQuestLog(): void {
    this.#quests.toggleLog();
  }

  /** Which touch context buttons apply now (#434), from the last received projection. */
  touchContext(): TouchContext {
    const projection = this.#lastProjection;
    const at = projection?.entities[0]?.position;
    const damaged = new Set(projection?.pieces.map((view) => view.piece) ?? []);
    return touchContext(projection, {
      isVendor: (npc) => this.#catalog?.vendors.has(npc) ?? false,
      doorInReach: at ? this.#reachableDoor(at[0], at[2]) !== 0 : false,
      repairInReach: at ? this.#reachablePieces(at[0], at[2]).some((id) => damaged.has(id)) || reachableAidTask(projection?.raid, this.#raidAid, at[0], at[2]) !== null || this.#prepStep(projection, at) !== null : false,
      arena: this.#arena,
    });
  }

  /**
   * The touch interact button: open the loot of a corpse in reach, select the nearest NPC (its
   * vendor or quest dialog opens once the zone accepts the selection, as after a click), or
   * otherwise the keyboard Interact (doors and repairs).
   */
  touchInteract(): void {
    const context = this.touchContext();
    if (context.interact !== "loot" && !this.#requireFreeHands()) { return; }
    if (context.interact === "loot") {
      if (!this.#loot.open) {
        this.#loot.toggle();
      }
      return;
    }
    const npc = context.npc;
    if (npc && (context.interact === "vendor" || context.interact === "talk")) {
      this.queueIntent((projection) => projection.entities.some((entity) => entity.kind === "npc" && entity.entityId === npc.id)
        ? { kind: "select-target", target: npc } : null, (sequence) => {
        if (sequence !== null) {
          this.#pendingNpc = { target: npc, sequence };
        }
      });
      return;
    }
    this.interact();
  }

  /** The imported clip the shared presenter chose for the viewer last frame, or null when procedural (#381). */
  get viewerClip(): string | null {
    return this.#viewerClip;
  }

  /** The viewer's layered animation last frame (#384): each region's target clip and the posed layers, or null when procedural. */
  get viewerAnimation(): ViewerAnimation | null {
    return this.#viewerAnimation;
  }

  /** Tunes the imported models' layered blend (the Action Lab, #384); kept for every later presenter. Presentation only. */
  setAnimationBlend(blend: AnimationBlend): void {
    this.#blend = blend;
    this.#units?.units.setBlend(blend);
  }

  /** Whether the development body overlay (#39) is shown. */
  get bodyOverlay(): boolean {
    return this.#bodyOverlay;
  }

  /** Shows or hides each unit's authoritative body over its model (#39); kept for every later presenter. */
  setBodyOverlay(shown: boolean): void {
    this.#bodyOverlay = shown;
    this.#units?.units.setBodyOverlay(shown);
  }

  /** The hotbar as the touch face buttons mirror it. */
  get hotbarKey(): string {
    return this.#classHud.slotKey;
  }

  hotbarFaces(): { name: string; source: HTMLButtonElement }[] {
    return this.#classHud.slotFaces();
  }

  closeQuestDialog(): boolean { return this.#quests.closeDialog(); }

  closeQuestLog(): boolean { return this.#quests.closeLog(); }

  /** Focuses the chat field; typing there releases held movement. */
  openChat(): void {
    this.#releasePointerLock();
    this.#chat.open();
  }

  closeLoot(): boolean { return this.#loot.close(); }

  closeBags(): boolean { return this.#bags.close(); }

  closeCharacter(): boolean { return this.#character.close(); }

  /** Opens or closes the world map (M). */
  toggleWorldMap(): void {
    if (this.#worldMap) {
      this.#worldMap.toggle();
      return;
    }
    if (this.#worldMapLoading) return;
    this.#worldMapLoading = true;
    const self = (): MapSelf | null => this.#lastSelf && { x: this.#lastSelf.x, z: this.#lastSelf.z, yaw: this.#lastSelf.facing };
    void import("./world-map").then(({ WorldMap }) => {
      this.#worldMap = new WorldMap(this.#worldMapHost, self, () => (this.#arena ? null : this.#exploration.map));
      this.#worldMap.open();
    }).finally(() => { this.#worldMapLoading = false; });
  }

  closeWorldMap(): boolean {
    const open = this.#worldMap?.isOpen ?? false;
    this.#worldMap?.close();
    return open;
  }

  /** Whether bags, the character pane, loot, the vendor or the world map are open: a non-blocking overlay over gameplay controls. */
  get panelOpen(): boolean {
    return this.#bags.open || this.#character.open || this.#loot.open || this.#vendor.open
      || this.#quests.dialogOpen || this.#quests.logOpen || (this.#worldMap?.isOpen ?? false);
  }

  toggleOverlay(): void {
    this.#overlay.toggle();
  }

  #input({ held, jumps, rolls }: WorldInput): MovementInput {
    const input = movementInput(held, this.#orbit.facing(), this.#movementFacing, jumps, this.#drag?.mode ?? "none",
      this.#cameraMode.kind === "focused" || this.#pointerOwner === "camera" || (this.#pointerOwner === "gesture" && this.#locked));
    this.#movementFacing = input.facing;
    return rolls === undefined ? input : { ...input, rolls };
  }

  #showArea(name: string): void {
    if (name === this.#shownArea) {
      return;
    }
    this.#shownArea = name;
    this.#elements.areaName.textContent = name;
  }

  /**
   * One frame: intent out, ticks, projection in, scene drawn. Throws when the projection lacks the viewer. The frame runs
   * as ordered stages over one reused context (#1060, `WORLD_FRAME_STAGES`).
   */
  frame(world: ZoneWorld, input: WorldInput, look: ViewerLook, deltaSeconds: number, now: number): void {
    const frame = this.#frame;
    frame.world = world;
    frame.input = input;
    frame.look = look;
    frame.deltaSeconds = deltaSeconds;
    frame.now = now;
    this.#stages.run(frame);
  }

  /** Deterministic work since the last call (the `world-frame` budget, #1060); the counters restart from zero. */
  takeFrameWork(): FrameWork {
    const work = { ...this.#frameWork };
    Object.assign(this.#frameWork, emptyFrameWork());
    return work;
  }

  /** Input: the static scene once, then queued intents, the guard lane and movement out to the source. */
  #inputStage(frame: WorldFrame): void {
    const { world, input, now } = frame;
    if (this.#staticParts < STATIC_PARTS) {
      // Built here only when entry came before the selection screen's prebuild finished (#855).
      this.#frameStaticBuilds += 1;
      this.#buildStatic(world);
    }
    const scene = this.#scene;
    const sceneryFrame = this.#sceneryFrame;
    if (!scene || !sceneryFrame) {
      throw new Error("The world view has no scenery loaded.");
    }
    frame.scene = scene;
    frame.sceneryFrame = sceneryFrame;
    const { source } = world;
    this.#partySource = source;
    const latest = source.latestProjection();
    for (const { intent, onSent } of this.#intents.splice(0)) {
      const command = latest ? intent(latest) : null;
      if (command) {
        // Send first: an optional call would skip evaluating its argument when there is no callback.
        const sequence = source.sendCommand(command);
        onSent?.(sequence);
      }
    }
    this.#guardHold.aimWithPad(input.padGuard ?? null);
    if (this.#guardHold.active) {
      this.#sendGuardLane();
    }
    for (const command of this.#outbox.update(this.#input(input), now, latest?.viewer.dead ?? false)) {
      source.sendCommand(command);
    }
    profilePhase("input");
  }

  /** Replication: the source advances, received projections fan out to the panels, and the latest one steers input. */
  #replicationStage(frame: WorldFrame): void {
    const { source, catalog } = frame.world;
    const receivedProjections = source.advance(this.#holdWorld ? 0 : frame.deltaSeconds);
    frame.received = receivedProjections;
    profilePhase("simulation");
    const connected = source.linkState() === "connected";
    frame.connected = connected;
    if (connected !== this.#lastLinkConnected) {
      this.#resetUnits();
      this.#lastLinkConnected = connected;
    }
    for (const received of receivedProjections) {
      this.#bags.update(received);
      this.#character.update(received);
      this.#loot.update(received);
      this.#waystone.update(received);
      this.#vendor.update(received);
      this.#quests.update(received);
      this.#chat.update(received);
      this.#party.update(received);
      this.#depthsLoot.update(received);
      this.#frameWork.panelRenders += RECEIVED_PANELS;
    }
    const projection = source.latestProjection();
    if (!projection) {
      throw new Error("The world source has no projection for the joined player.");
    }
    frame.projection = projection;
    frame.tick = projection.tick;
    this.#lastProjection = projection;
    this.#showVeins(projection.depletedVeins);
    this.#powerDraw.observe(projection, this.panelOpen);
    this.#syncCameraMode(projection);
    this.#mouse.setFocus(this.#cameraMode.kind === "focused");
    // Death or a stun ends a held gesture without firing (#636).
    this.#gestureEnded(this.#mouse.observe(projection));
    // Death already lowered the guard in the zone; a later life needs a new press.
    if (projection.viewer.dead) {
      this.#guardHold.end();
    }
    const pendingNpc = this.#pendingNpc;
    if (pendingNpc && projection.acknowledgedSequence >= pendingNpc.sequence) {
      this.#pendingNpc = null;
      if (sameEntity(projection.viewer.target, pendingNpc.target) && !projection.viewer.weaponDrawn) {
        if (catalog.vendors.has(pendingNpc.target.id)) {
          if (!this.#vendor.open) {
            this.#vendor.toggle();
          }
        } else if ([...catalog.quests.values()].some((quest) => quest.giver === pendingNpc.target.id || quest.ender === pendingNpc.target.id)) {
          if (!this.#quests.dialogOpen) {
            this.#quests.toggleDialog();
          }
        }
      }
    }
    // An open panel ends a held gesture without firing (#636).
    if (this.panelOpen) { this.#releasePointerLock(); if (this.#mouse.gesture) { this.interruptGesture(); } }
  }

  /** Units: the shared unit presenter samples the projections, and imported models, impacts and sound follow it. */
  #unitStage(frame: WorldFrame): void {
    const { world, projection, deltaSeconds, now, look } = frame;
    const { source, scenery, catalog } = world;
    const animate = this.#animate;
    frame.animate = animate;
    this.#seconds += deltaSeconds;
    if (this.#units?.viewerId !== projection.viewerId) {
      this.#units?.units.dispose();
      const units = new SharedUnits(world.scenery.unitPresenter(projection.viewerId));
      for (const [kind, clips, descriptor] of this.#loadedModels) { units.modelReady(kind, clips, descriptor); }
      if (this.#blend) { units.setBlend(this.#blend); }
      if (this.#bodyOverlay) { units.setBodyOverlay(true); }
      this.#units = { viewerId: projection.viewerId, units, pushed: null, residents: false, kitPlatforms: false, effectKits: false };
    }
    const units = this.#units.units;
    frame.units = units;
    if (!this.#units.kitPlatforms && this.#fortress?.drawn) {
      // The fortress kit's lifts (#591) replace the presenter's pillars once drawn.
      this.#units.kitPlatforms = true;
      units.setKitPlatforms(this.#fortress.platforms);
    }
    // The strike and spell effect bundles (#894) replace the presenter's primitive shots, beams and flashes once loaded.
    void this.#kitEffects.load();
    if (!this.#units.effectKits && this.#kitEffects.loaded) {
      this.#units.effectKits = true;
      units.setEffectKits(true);
    }
    const at = entitiesSelf(projection);
    this.#regions?.approach(at, scenery.scenery.areas);
    if (at) { this.#loadKitsNearPoint(at); }
    for (const [kind, clips, descriptor] of this.#models.drainLoaded()) {
      this.#loadedModels.push([kind, clips, descriptor]);
      units.modelReady(kind, clips, descriptor);
    }
    if (!this.#units.residents && this.#models.residentsLoaded()) {
      this.#units.residents = true;
      units.residentsReady();
    }
    for (const received of frame.received) {
      this.#classHud.receive(received, now);
      if (frame.connected) { this.#pushProjection(units, received); }
    }
    // The join projection (and a paused source's) reaches the presenter without an advance.
    if (this.#units.pushed !== projection) { this.#pushProjection(units, projection); }
    // The pushed projections' sound cues (#447); taken even while silent so none play late.
    const sounds = units.takeAudio();
    this.#sound?.apply(sounds);
    this.#orbit.update(deltaSeconds, !animate);
    profilePhase("projection");
    const { unitsPerMetre } = scenery.scenery;
    const shown = source.shown();
    units.setInteraction(this.#loot.open, this.#quests.dialogNpc ?? this.#vendor.npc);
    const presented = units.frame(shown, projection, { seconds: this.#seconds, dt: deltaSeconds, reducedMotion: !animate, barYaw: this.#barYaw, look });
    frame.presented = presented;
    frame.effectNodes = presented.effectNodes;
    this.#fortress?.place(presented.platforms);
    this.#kitEffects.update(presented.effects);
    const viewerModel = presented.imported.find((unit) => unit.entity.kind === "player" && unit.entity.id === projection.viewerId);
    this.#viewerClip = viewerModel?.clip ?? null;
    this.#viewerAnimation = viewerModel ? { upper: [viewerModel.clip, viewerModel.time], lower: viewerModel.lower, layers: viewerModel.layers } : null;
    const projected: ProjectedUnit[] = [];
    const others: MinimapUnit[] = [];
    const anchors = frame.anchors;
    anchors.clear();
    let self: FrameSelf | null = null;
    for (const unit of presented.units) {
      const entity = unit.state;
      if (!entity) { continue; }
      const placement = { x: unit.feet[0], feetY: unit.feet[1], z: unit.feet[2], yawRadians: unit.yaw };
      anchors.set(entityKey(unit.entity), placement);
      projected.push(projectedUnit(entity, placement, catalog));
      if (entity.kind === "player" && entity.entityId === projection.viewerId) {
        self = { focus: [unit.feet[0], unit.centreY, unit.feet[2]], x: unit.feet[0], feetY: unit.feet[1], z: unit.feet[2], facing: unit.yaw };
        this.#showArea(scenery.areaAt(entity.position[0], entity.position[2])?.name ?? this.#zoneName);
      } else {
        others.push(this.#minimapUnit(entity, unitsPerMetre));
      }
    }
    frame.others = others;
    this.#lastProjectedUnits = projected;
    // The posed casters' spell origins: the next frames' beams leave the staff's head from there (#612).
    units.setChannelOrigins(this.#models.update(presented.imported, { headwear: viewerHeadwear(look), hair: viewerHair(look), face: look.face ?? {}, physique: look.physique ?? null, sex: look.sex }));
    const intensity = animate ? "strong" : "subtle";
    this.#impacts.preload(intensity);
    for (const cue of units.takeImpacts()) {
      const unit = units.unitOf(cue.target);
      if (unit) { this.#impacts.spawn(cue.family, intensity, [unit.feet[0], unit.centreY, unit.feet[2]], now); }
    }
    this.#impacts.update(now);
    const roof = this.#roofOf(projection.raid ? this.#raidArea : null, scenery.scenery);
    this.#raidSmoke.update(projection.raid, roof?.top ?? null, roof?.span ?? [0, 0], now);
    frame.unitNodes = this.#plan ? NO_NODES : presented.nodes;
    profilePhase("units");
    if (!self) {
      throw new Error("The projection is missing the viewer's own unit.");
    }
    frame.self = self;
  }

  /** Camera: the orbit follows the viewer (or a focused opponent), rooms adapt the boom, and the listener follows the eye. */
  #cameraStage(frame: WorldFrame): void {
    const { self, projection, deltaSeconds, now, animate, scene, presented } = frame;
    const { unitsPerMetre } = frame.world.scenery.scenery;
    // A manual aim (#649) hands the camera to the mouse; it turns back to the kept focus after.
    const opponent = this.#cameraMode.kind === "focused" && this.#mouse.owner() !== "aim"
      ? frame.anchors.get(entityKey(this.#cameraMode.target)) : undefined;
    this.#orbit.track(self.focus, opponent ? [opponent.x, opponent.feetY + 1, opponent.z] : null, deltaSeconds);
    this.#lastSelf = { x: self.x, z: self.z, facing: self.facing };
    this.#inn?.update(projection.doors, [self.x, self.feetY, self.z], deltaSeconds, !animate);
    // A walk-in cottage's roof lifts away while the viewer is inside it (#843).
    this.#outpost?.hideCottages(this.#inn?.cottagesInside([self.x, self.feetY, self.z]) ?? new Set());
    this.#raidSigns.update(projection.raid, this.#raidPrep, scene.surfaceY, unitsPerMetre);
    if (this.#framePending) {
      this.#framePending = false;
      this.#lookFirst(this.#orbit, self.focus, self.facing, scene.surfaceY);
    }
    const orbit = this.#orbit.view(self.focus, scene.surfaceY);
    // The arena and authored indoor rooms use an adaptive boom, retaining normal orbit controls.
    const indoor = this.#inn?.camera(orbit, [self.x, self.feetY, self.z], deltaSeconds, !animate) ?? orbit;
    const view = this.#plan ? planPose(this.#plan) : this.#flyTo ?? (this.#arena ? clearSightLine(indoor, this.#occluders) : indoor);
    frame.view = view;
    // A blow the viewer struck or took kicks the camera a moment (#900); zero under reduced motion.
    const kick = this.#plan || this.#flyTo ? NO_KICK : presented.kick;
    this.#camera.position.set(view.eye[0] + kick[0], view.eye[1] + kick[1], view.eye[2] + kick[2]);
    this.#camera.lookAt(view.target[0] + kick[0], view.target[1] + kick[1], view.target[2] + kick[2]);
    this.#camera.updateMatrixWorld(true);
    if (this.#plan) { applyPlanProjection(this.#camera, this.#plan); }
    // Ambient creatures and the settlement props' node clips (#891: mill wheels, sails, signs, vanes and scarecrows)
    // sample by the posed camera's view and distance (#1095). Low world detail draws no ambient creatures.
    if (this.#worldDetail !== "low") { this.#ambient?.update(this.#camera, this.#seconds, animate); }
    this.#settlement?.update(this.#camera, this.#seconds, animate);
    this.#showAimLanding(projection, unitsPerMetre);
    // The opt-in wheel usage counters (#653): time under each mouse owner, stored every 10 s.
    if (this.#wheels.countUsage) {
      this.#saveWheelUsage(this.#mouse.countUsage(Math.min(deltaSeconds, 1) * 1000), Math.max(now, 1));
    }
    if (this.#sound) {
      const right = new THREE.Vector3().setFromMatrixColumn(this.#camera.matrixWorld, 0);
      this.#sound.listen(view.eye, [right.x, right.y, right.z]);
      this.#sound.pump();
    }
  }

  /** Scenery: static groups, ground, grass and kits stream around the eye, and the hour lights the scene. */
  #sceneryStage(frame: WorldFrame): void {
    const { world, view, self, deltaSeconds, animate, tick } = frame;
    const outpostReady = (this.#outpost?.drawn ?? false) && (this.#fortifications?.drawn ?? false);
    const kits = { outpost: outpostReady, settlementProps: this.#settlement?.drawn ?? false, nature: this.#nature?.drawn ?? false, limestone: this.#standInDrawn("limestone"), climbing: this.#climbing?.drawn ?? false, crag: this.#crag?.drawn ?? false, fortress: this.#fortress?.drawn ?? false };
    const batches = this.#sceneryBatches;
    if (batches) { batches.stream(view.eye, { ...kits, dressed: this.#fortress?.drawn ? world.scenery.dressedProps?.() ?? NO_DRESSED : NO_DRESSED }); }
    const sceneFrame = frame.sceneryFrame.nodes(view.eye, { seconds: this.#seconds, animate, viewer: [self.x, self.feetY, self.z], kits, resident: batches ? (group) => batches.resident(group) : () => true });
    frame.sceneFrame = sceneFrame;
    // A map view's eye is high only to clear the terrain: its ground streams as if standing on it.
    this.#ground?.update(this.#plan ? [view.eye[0], 0, view.eye[2]] : view.eye, sceneFrame.shown);
    if (!this.#plan) { this.#grass?.update(view.eye, [self.x, self.feetY, self.z], this.#seconds, animate); }
    // The sun cascades cast only what the kits draw: they keep off-screen casters whose shadow reaches the view (#1038).
    const sun = this.#lighting.sunShadowDirection;
    this.#nature?.update(this.#camera, deltaSeconds, animate, sun);
    this.#climbing?.update(this.#camera, deltaSeconds, animate, sun);
    this.#crag?.update(this.#camera, deltaSeconds, animate, sun);
    this.#gather?.update(this.#camera, deltaSeconds, animate, sun);
    for (const kit of this.#kits) { kit.update(this.#camera, deltaSeconds, animate, sun); }
    this.#fortress?.update(this.#camera, deltaSeconds, animate);
    this.#lastSceneryNodes = sceneFrame.nodes;
    this.#barYaw = Math.atan2(view.target[0] - view.eye[0], view.target[2] - view.eye[2]);
    // The batches draw the static groups and animated parts; the renderer only the water.
    this.#sceneryBatches?.show(sceneFrame.nodes);
    const rendered = this.#sceneryBatches?.undrawn(sceneFrame.nodes) ?? sceneFrame.nodes;
    frame.batchedNodes = sceneFrame.nodes.length - rendered.length;
    const nodes: RendererSceneNode[] = [];
    if (!this.#plan?.groundOnly) { appendAll(nodes, rendered); }
    appendAll(nodes, frame.unitNodes);
    this.#lastEffectNodes = frame.effectNodes;
    appendAll(nodes, this.#lastEffectNodes);
    frame.nodes = nodes;
    this.#lastUnitNodes = frame.unitNodes;
    const camera: RendererCamera = {
      viewMatrix: [...this.#camera.matrixWorldInverse.elements] as Matrix4Values,
      projectionMatrix: webGpuProjection(this.#camera),
    };
    frame.camera = camera;
    // The sky follows the zone's clock (#158); a debug presentation time overrides it.
    // Reduced motion steps the sky once per zone hour instead of drifting continuously (the shared `clock`).
    const hour = this.#presentationTime === null ? wasm.zoneSkyHour(tick, animate) : wasm.pagesHour(this.#presentationTime, true);
    const dayFraction = wasm.dayFraction(hour);
    frame.dayFraction = dayFraction;
    const environment = environmentAt(ENVIRONMENT, dayFraction);
    frame.environment = environment;
    this.#lastEnvironment = environment;
    // The native client's lights at the same hour (#600): sun or moon, sky light, fog and the scenery's lamps.
    this.#lighting.apply({
      hour,
      sky: true,
      interior: Math.max(this.#scene?.interior([self.x, self.feetY, self.z]) ?? 0, this.#inn?.interior([self.x, self.feetY, self.z]) ?? 0),
      fog: environment.fog && !this.#plan ? String(environment.fog.color) : null,
      // Low world detail lights no lamps or fires: each local light is shaded on every lit pixel near it.
      pointLights: this.#worldDetail === "low" ? null : this.#inn?.pointLights(this.#scene?.pointLights(this.#seconds, animate) ?? new Float32Array()) ?? this.#scene?.pointLights(this.#seconds, animate) ?? null,
    });
    this.#lastInstanceBatches = sceneFrame.instanceBatches;
    this.#lastCamera = camera;
    profilePhase("scene");
  }

  /** Render: the root registry (rebuilt only on a change) and the frame go to the renderer. */
  #renderStage(frame: WorldFrame): void {
    const { camera, nodes, sceneFrame, environment } = frame;
    if (this.#rootRegistry.refresh()) {
      this.#rootsStale = true;
      // A map shows the world without its inhabitants; a ground map only the terrain. The registry rebuilds when a map
      // view starts, changes or ends, and whenever a root joins.
      if (this.#plan) { this.#hideForPlan(this.#plan); }
    }
    const roots = this.#rootRegistry.roots;
    // No scene background: `Lighting` clears its own frame, and three would clear to the background again in each of its
    // passes, wiping the sky (#600). The environment's fog still fogs the unlit sprites.
    const { background: _background, ...withoutBackground } = environment;
    const rendererFrame = { camera, nodes, instanceBatches: this.#plan?.groundOnly ? [] : sceneFrame.instanceBatches, environment: withoutBackground };
    if (this.#preparing) {
      // `prepare` (#547): the first world frame's programs and textures, without drawing it.
      this.#preparing.push(this.#renderer.prepare(rendererFrame, { roots: roots.slice() }));
      // The prepare submitted these roots: the first drawn frame need not submit them again, which draws a whole extra
      // frame (#855). A selection frame drawn since detaches them, which the next world frame notices.
      this.#submittedRoots = new Set(roots);
      this.#rootsStale = false;
      frame.stopped = true;
      return;
    }
    // Submitting roots adds them to the renderer's scene, and anything later added under a submitted root is drawn without
    // it; `renderImportedRoots` also draws a whole frame, so it runs only when the set of roots changes, not each time a
    // streamed cell, tile or group joins one (a doubled frame every few metres of walking).
    const detached = !this.#impacts.root.parent || !this.#kitEffects.root.parent || !this.#raidSmoke.root.parent || !this.#raidSigns.root.parent;
    if (detached || (this.#rootsStale && !sameRoots(roots, this.#submittedRoots))) {
      this.#renderer.renderImportedRoots({ camera, roots: roots.slice() });
      this.#submittedRoots = new Set(roots);
    }
    this.#rootsStale = false;
    const work = this.#renderer.render(rendererFrame);
    this.#lastWork = work;
    profilePhase("render");
    if (profiling()) {
      profileFacts({
        ticks: frame.received.length, viewer: [frame.self.x, frame.self.z], heading: this.#orbit.heading, nodes: nodes.length, instanceBatches: sceneFrame.instanceBatches.length,
        created: { objects: work.objectCreateCount, geometries: work.geometryCreateCount, materials: work.materialCreateCount, instanceUploads: work.instanceUploadCount },
      });
    }
  }

  /** HUD: combat, class and progression bars, quest markers, the sky fallback, the minimap and the overlay. */
  #hudStage(frame: WorldFrame): void {
    const { projection, now, self, view } = frame;
    const { scenery, catalog } = frame.world;
    const { unitsPerMetre } = scenery.scenery;
    this.#combatHud.update(projection, catalog, now);
    this.#classHud.update(projection, catalog, now);
    this.#weaponMode.textContent = projection.viewer.weaponDrawn
      ? "Combat · Sheathe weapon [H]"
      : "Exploration · Draw weapon [H]";
    this.#weaponMode.dataset.mode = projection.viewer.weaponDrawn ? "combat" : "exploration";
    this.#weaponMode.disabled = projection.viewer.dead;
    this.#progressionHud.update(projection, now);
    this.#minimap.update(scenery.scenery, { x: self.x, z: self.z });
    this.#quests.placeMarkers(this.#markerPoints(frame.anchors, projection));
    // The atmosphere draws the sky (#600); the CSS sky remains only where the browser cannot draw it.
    const cssSky = !this.#lighting.drawsSky;
    if (this.#elements.sky.hidden === cssSky) { this.#sky.show(cssSky); }
    if (cssSky) { this.#sky.update(this.#horizon(view.eye, view.target), frame.dayFraction); }
    // A night raid's approximate disturbance area while its assault runs (#530).
    const area = this.#raidArea;
    const disturbance = area && (projection.raid?.phase ?? 0) > 0
      ? { x: area.x / unitsPerMetre, z: area.z / unitsPerMetre, radius: area.radius / unitsPerMetre }
      : null;
    // Discovered landmarks and approximate quest areas (#983); the arena has neither.
    const exploration = this.#arena ? null : minimapExploration(this.#exploration.update(projection), self);
    this.#minimap.draw({ x: self.x, z: self.z }, this.#orbit.heading, self.facing, frame.others, disturbance, exploration);
    this.#frameRate.sample(now);
    this.#overlay.update({
      fps: this.#frameRate.fps,
      frameMs: this.#frameRate.frameMs,
      nodes: frame.nodes.length + frame.batchedNodes + frame.sceneFrame.instanceBatches.length,
      staticNodes: frame.scene.stats.staticNodes,
      visibleStaticNodes: frame.sceneFrame.visibleBatches,
      staticVertices: frame.scene.stats.staticVertices,
      units: frame.presented.units.length,
      work: this.#lastWork,
    }, now);
  }

  /**
   * Panels: bags, the character pane and loot redraw from the latest projection only when it or the shared bag state
   * changed (#1060); every received projection already reached all panels in the replication stage.
   */
  #panelStage(frame: WorldFrame): void {
    const { projection } = frame;
    if (!this.#panelGate.changed(projection, this.#bags.state.changes)) { return; }
    this.#bags.update(projection);
    this.#character.update(projection);
    this.#loot.update(projection);
    this.#frameWork.panelRenders += LATEST_PANELS;
  }

  /** Hides a map view's inhabitants (and on a ground map everything but the ground), and shows what it no longer hides. */
  #hideForPlan(plan: PlanView): void {
    const roots = this.#rootRegistry.roots;
    for (let index = 0; index < roots.length; index += 1) {
      const root = roots[index]!;
      const role = this.#rootRegistry.role(index);
      if (role === "inhabitant" || (plan.groundOnly && role !== "ground")) {
        root.visible = false;
        this.#planHidden.add(root);
      } else if (this.#planHidden.delete(root)) {
        root.visible = true;
      }
    }
  }

  /** Drops every per-unit and visual state; the next frame pushes the latest projection again. */
  #resetUnits(): void {
    if (this.#units) {
      this.#units.units.reset();
      this.#units.units.takeAudio();
      this.#units.pushed = null;
    }
    this.#sound?.reset();
  }

  #pushProjection(units: SharedUnits, projection: ZoneSnapshot): void {
    const ingested = ingestedProjection(projection);
    if (ingested && this.#units) {
      units.push(ingested, this.#seconds);
      this.#units.pushed = projection;
    }
  }

  /** Screen anchors above every projected NPC, from the camera this frame drew with. */
  #markerPoints(anchors: ReadonlyMap<string, { x: number; feetY: number; z: number }>, projection: ZoneSnapshot): MarkerPoint[] {
    const canvas = this.#elements.canvas;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    const point = new THREE.Vector3();
    return projection.entities.flatMap((entity) => {
      if (entity.kind !== "npc") {
        return [];
      }
      const anchor = anchors.get(entityKey({ kind: "npc", id: entity.entityId }));
      if (!anchor) {
        return [];
      }
      point.set(anchor.x, anchor.feetY + MARKER_HEIGHT_METRES, anchor.z).project(this.#camera);
      const onScreen = point.z > -1 && point.z < 1 && Math.abs(point.x) <= 1.05 && Math.abs(point.y) <= 1.05;
      return [{ npc: entity.entityId, x: ((point.x + 1) / 2) * width, y: ((1 - point.y) / 2) * height, onScreen }];
    });
  }

  /** The roof of the barn nearest the raid target's area (units), from the scenery's props. */
  #roofOf(objective: { x: number; z: number } | null, scenery: ZoneWorld["scenery"]["scenery"]): { top: [number, number, number]; span: [number, number] } | null {
    if (!objective) {
      return null;
    }
    const key = `${objective.x},${objective.z}`;
    if (this.#raidRoof?.key !== key) {
      const barn = scenery.props
        .filter((prop) => prop.kind === "barn")
        .sort((a, b) => Math.hypot(a.position[0] - objective.x, a.position[2] - objective.z) - Math.hypot(b.position[0] - objective.x, b.position[2] - objective.z))[0];
      if (!barn) {
        return null;
      }
      const unit = scenery.unitsPerMetre;
      this.#raidRoof = {
        key,
        top: [barn.position[0] / unit, (barn.position[1] + 2 * barn.halfExtents[1]) / unit, barn.position[2] / unit],
        span: [(1.4 * barn.halfExtents[0]) / unit, (1.4 * barn.halfExtents[2]) / unit],
      };
    }
    return this.#raidRoof;
  }

  #minimapUnit(entity: EntityState, unitsPerMetre: number): MinimapUnit {
    return { x: entity.position[0] / unitsPerMetre, z: entity.position[2] / unitsPerMetre, disposition: dispositionOf(entity) };
  }

  /** The horizon's height on screen as a fraction from the top, for the CSS sky. */
  #horizon(eye: Vec3, target: Vec3): number {
    const dx = target[0] - eye[0];
    const dz = target[2] - eye[2];
    const length = Math.hypot(dx, dz) || 1;
    const far = new THREE.Vector3(eye[0] + (dx / length) * 5_000, eye[1], eye[2] + (dz / length) * 5_000);
    far.project(this.#camera);
    return (1 - far.y) / 2;
  }

  /**
   * One mouse owner at a time (#636): the left button selects (or confirms an aim or wheel, and does
   * nothing during a strike or guard hold); the right button cancels a held gesture without firing,
   * otherwise the viewer's cast. Touch keeps its drag, pinch and tap selection.
   */
  pointerDown(event: PointerEvent): void {
    if (event.pointerType !== "touch") {
      this.#mousePress(event);
      return;
    }
    this.#powerDraw.cancel();
    // A finger supersedes mouse capture; existing touch drag/pinch stays authoritative.
    this.#releasePointerLock();
    this.#updatePointerOwner();
    this.#touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
    // A second finger turns the look drag into a pinch zoom; neither finger selects.
    if (this.#touches.size === 2 && this.#drag) {
      try {
        this.#elements.canvas.setPointerCapture(event.pointerId);
      } catch {
        // The pinch still tracks the finger while it stays over the canvas.
      }
      this.#primaryClick.reset();
      this.#pinch = this.#touchSpread();
      return;
    }
    this.#beginDrag(event, "turn");
  }

  #mousePress(event: PointerEvent): void {
    if (event.button === 2) {
      // Right click never fires: it drops a held draw (#607) and cancels the gesture or the cast.
      this.#powerDraw.cancel();
      const click = this.#mouse.rightClick();
      if (click.outcome === "cancel") {
        this.#gestureEnded(click.ended);
      } else {
        this.queueIntent(cancelCast);
        // Without capture, a right drag still turns the character.
        if (this.#cameraMode.kind === "free-look" && this.#pointerOwner === "interface") { this.#beginDrag(event, "turn"); }
      }
      return;
    }
    if (event.button !== 0) {
      return;
    }
    // A click drops a held draw (#607) and is nothing else.
    if (this.#powerDraw.cancel()) { return; }
    if (this.#cameraMode.kind === "free-look" && this.#pointerOwner === "interface" && event.pointerType === "mouse"
      && !this.#mouse.gesture) {
      // A click on a unit or door under the free cursor selects it on release; any other click
      // recaptures mouse-look then, within the press's user activation (#636).
      this.#beginDrag(event, "orbit");
      this.#recaptureClick = true;
      return;
    }
    const click = this.#mouse.leftClick();
    // An armed aim throws (#649); a hold or held aim ignores the left button; a tapped-open wheel
    // runs the pointed-at sector, or nothing from the centre (#653).
    if (click.outcome === "throw" && click.ability !== null) {
      this.#throwAimed(click.ability);
      return;
    }
    if (click.outcome === "confirm") {
      if (click.action !== null) { this.onWheelAction?.(click.action); }
      this.#updatePointerOwner();
      return;
    }
    if (click.outcome !== "select") { return; }
    if (this.#pointerOwner === "camera" || this.#cameraMode.kind === "focused") {
      // Selects on the release, unless the press became mouse-look travel.
      this.#primaryClick.down(event);
      this.#lockedTravel = 0;
      return;
    }
    this.#beginDrag(event, "orbit");
  }

  #beginDrag(event: PointerEvent, mode: "orbit" | "turn"): void {
    if (this.#drag) {
      return;
    }
    try {
      this.#elements.canvas.setPointerCapture(event.pointerId);
    } catch {
      return;
    }
    // A finger drag looks and turns the character with the view (#434); a tap still selects.
    this.#drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, mode };
    this.#primaryClick.down(event);
    this.#elements.canvas.dataset.drag = mode;
  }

  /** Requests Pointer Lock for free look; the browser may refuse, and the next world click retries. */
  recapture(): void {
    if (this.#cameraMode.kind === "free-look" && document.pointerLockElement !== this.#elements.canvas) {
      void this.#elements.canvas.requestPointerLock().catch(() => {});
    }
  }

  /** Losing focus retires the gesture without firing or selecting. */
  cancelPointer(): void {
    this.interruptGesture();
    this.#powerDraw.cancel();
    this.#endDrag();
    this.#releasePointerLock();
    this.#updatePointerOwner();
  }

  /**
   * Pointer Lock is browser-owned: the presentation only requests it during user input. Losing it
   * without asking (the browser's Escape) ends a gesture without firing and opens the game menu.
   */
  pointerLockChanged(): void {
    const locked = document.pointerLockElement === this.#elements.canvas;
    if (this.#locked && !locked) {
      this.interruptGesture();
      this.#powerDraw.cancel();
      if (!this.#releasingLock) { this.onPointerLockLost?.(); }
    }
    this.#locked = locked;
    this.#releasingLock = false;
    // A capture an aim asked for that arrived after it ended gives a focus its cursor back (#649).
    if (locked && this.#cameraMode.kind === "focused" && !this.#mouse.gesture) { this.#releasePointerLock(); }
    this.#updatePointerOwner();
  }

  #releasePointerLock(): void {
    if (document.pointerLockElement === this.#elements.canvas) {
      this.#releasingLock = true;
      document.exitPointerLock();
    }
  }

  #updatePointerOwner(): void {
    this.#pointerOwner = pointerOwner(this.#cameraMode, document.pointerLockElement === this.#elements.canvas,
      this.#mouse.gesture);
    this.#elements.canvas.dataset.pointerOwner = this.#pointerOwner;
    this.#elements.canvas.dataset.mouseOwner = this.#mouse.owner();
    this.#showWheel();
  }

  #syncCameraMode(projection: ZoneSnapshot): void {
    const target = combatFocus(projection);
    if (this.#cameraMode.kind === "focused" && !target && sameEntity(projection.viewer.target, this.#cameraMode.target)) {
      // A dead/despawned target must not become a phantom focus on a later projection.
      this.queueIntent(() => ({ kind: "select-target", target: null }));
    }
    if (!target && this.#cameraMode.kind === "free-look") { return; }
    if (target && this.#cameraMode.kind === "focused" && sameEntity(target, this.#cameraMode.target)) { return; }
    this.#cameraMode = target ? { kind: "focused", target } : { kind: "free-look" };
    this.#mouse.setFocus(target !== null);
    if (target) { this.#releasePointerLock(); }
    this.#updatePointerOwner();
  }

  pointerMove(event: PointerEvent): void {
    if (event.pointerType !== "touch" && this.#mouse.gesture) {
      // The gesture owns the motion (#636); a locked camera holds still, a free one keeps turning.
      const turn = this.#mouse.motion(event.movementX, event.movementY);
      this.#followGestureAim();
      // An open wheel (#653) lights the sector the motion points at; the camera holds still.
      if (this.#mouse.owner() === "wheel") { this.#showWheel(); }
      if (turn && (document.pointerLockElement === this.#elements.canvas || this.#mouse.owner() === "aim")) {
        this.#orbit.orbit(event.movementX * this.#lookSensitivity, event.movementY * this.#lookSensitivity);
      }
      return;
    }
    if (this.#pointerOwner === "camera" && event.pointerType !== "touch") {
      this.#lockedTravel += Math.hypot(event.movementX, event.movementY);
      if (this.#lockedTravel > 6) { this.#primaryClick.reset(); }
      this.#orbit.orbit(event.movementX * this.#lookSensitivity, event.movementY * this.#lookSensitivity);
      return;
    }
    this.#primaryClick.move(event);
    if (this.#touches.has(event.pointerId)) {
      this.#touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (this.#pinch !== null) {
        const spread = this.#touchSpread();
        if (spread > 0 && this.#pinch > 0) {
          this.#orbit.zoomBy(spread / this.#pinch);
        }
        this.#pinch = spread;
        return;
      }
    }
    const drag = this.#drag;
    if (!drag || drag.pointerId !== event.pointerId) {
      return;
    }
    const sensitivity = this.#lookSensitivity;
    this.#orbit.orbit((event.clientX - drag.x) * sensitivity, (event.clientY - drag.y) * sensitivity);
    drag.x = event.clientX;
    drag.y = event.clientY;
  }

  /** The held strike's aim display and wind-up, or the held guard's lane, follow the mouse. */
  #followGestureAim(): void {
    const aim = this.#mouse.aim();
    if (aim.strike) {
      this.#classHud.showStrikeAim(aim.strike);
      this.#aimWindUp();
    }
    if (aim.guard) {
      this.#guardHold.aimWithPointer(aim.guard);
      this.#sendGuardLane();
    }
  }

  pointerEnd(event: PointerEvent): void {
    if (event.pointerType !== "touch" && this.#drag?.pointerId !== event.pointerId) {
      // A click that started selecting selects only if no gesture took the mouse meanwhile.
      if (this.#primaryClick.end(event) && !this.#mouse.gesture) {
        this.#selectAt(event, document.pointerLockElement === this.#elements.canvas);
      }
      this.#lockedTravel = 0;
      return;
    }
    if (this.#touches.delete(event.pointerId) && this.#pinch !== null) {
      // Lifting either pinch finger ends the gesture; the other finger must lift before a new drag.
      this.#pinch = null;
      if (this.#elements.canvas.hasPointerCapture(event.pointerId)) {
        this.#elements.canvas.releasePointerCapture(event.pointerId);
      }
      this.#endDrag();
      return;
    }
    if (this.#drag?.pointerId !== event.pointerId) {
      return;
    }
    // Explicit release can precede the browser's queued lostpointercapture event.
    // Inspect ownership now, before a later pointerup can turn it into a click.
    if (event.type === "pointerup" && !this.#elements.canvas.hasPointerCapture(event.pointerId)) {
      this.#endDrag();
      return;
    }
    // A tap, or a click that never became a drag, selects; a released free look's click on
    // nothing recaptures mouse-look.
    const recapture = this.#recaptureClick;
    this.#recaptureClick = false;
    if (this.#primaryClick.end(event) && !this.#mouse.gesture && !this.#selectAt(event, false) && recapture) {
      this.recapture();
    }
    if (this.#drag?.pointerId === event.pointerId) {
      this.#endDrag();
    }
  }

  /** Selects (or swings the door) under the pointer or crosshair; `true` if anything was picked. */
  #selectAt(event: PointerEvent, centred: boolean): boolean {
    const rect = this.#elements.canvas.getBoundingClientRect();
    const point = new THREE.Vector2(
      centred ? 0 : ((event.clientX - rect.left) / rect.width) * 2 - 1,
      centred ? 0 : 1 - ((event.clientY - rect.top) / rect.height) * 2,
    );
    // Every other living player is selectable: a foe to attack or a friend to heal and shield (#303).
    const viewerId = this.#lastProjection?.viewerId;
    const selectablePlayers = new Set(this.#lastProjection?.entities
      .filter((entity) => entity.kind === "player" && entity.entityId !== viewerId && !entity.flags.dead)
      .map((entity) => entity.entityId) ?? []);
    const picked = this.#picking.pick(point, this.#camera, this.#lastSceneryNodes, this.#lastUnitNodes, this.#lastInstanceBatches, this.#models.pickables(), [...(this.#inn?.pickables() ?? []), ...[this.#outpost, this.#fortifications, this.#settlement].flatMap((kit) => kit?.pickables() ?? [])], selectablePlayers, { width: rect.width, height: rect.height, pointerType: event.pointerType });
    this.#pendingNpc = null;
    // A click on the door leaf asks the zone to swing it; the zone checks reach (#158).
    if (picked?.kind === "door") {
      if (this.weaponDrawn) { this.#requireFreeHands(); }
      this.queueIntent((projection) => doorToggle(projection, picked.id));
    }
    const target = picked?.kind === "door" ? null : picked;
    if (!picked && this.#cameraMode.kind === "focused" && event.pointerType === "mouse") {
      // With Escape now the game menu (#636), a focused click on empty world clears the focus.
      this.queueIntent(() => ({ kind: "select-target", target: null }));
    }
    if (target) {
      this.queueIntent((projection) => projection.entities.some((entity) => entity.kind === target.kind && entity.entityId === target.id)
        ? { kind: "select-target", target } : null, (sequence) => {
        if (target.kind === "npc" && sequence !== null) {
          if (this.weaponDrawn) { this.#requireFreeHands(); }
          else { this.#pendingNpc = { target, sequence }; }
        }
      });
    }
    return Boolean(picked);
  }

  /** The distance between the first two fingers on the canvas, in CSS pixels. */
  #touchSpread(): number {
    const [first, second] = [...this.#touches.values()];
    return first && second ? Math.hypot(first.x - second.x, first.y - second.y) : 0;
  }

  #endDrag(): void {
    this.#touches.clear();
    this.#pinch = null;
    this.#primaryClick.reset();
    this.#recaptureClick = false;
    this.#lockedTravel = 0;
    const drag = this.#drag;
    if (drag && this.#elements.canvas.hasPointerCapture(drag.pointerId)) {
      this.#elements.canvas.releasePointerCapture(drag.pointerId);
    }
    this.#drag = null;
    delete this.#elements.canvas.dataset.drag;
  }

  wheel(event: WheelEvent): void {
    const lines = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL
      ? event.deltaY / PIXELS_PER_WHEEL_LINE
      : event.deltaMode === WheelEvent.DOM_DELTA_LINE ? event.deltaY : event.deltaY * 3;
    // Scrolling up (negative delta) zooms in.
    this.#orbit.zoom(-lines);
  }

  /**
   * Prefetches by selection intent (#412): area `entry`'s art now; then, one piece at a time,
   * `others` (other characters' saved areas) and `entry`'s neighbours.
   */
  prefetchSelection(entry: number, others: readonly number[]): void {
    this.#pendingKitArea = this.#areas.find((area) => area.id === entry) ?? null;
    this.#regions?.plan(entry, others);
    this.#loadSelectionKits();
  }

  /** How many of area `id`'s art pieces have settled (loaded or failed) out of how many. */
  artProgress(id: number | null): { loaded: number; total: number } {
    return this.#regions?.progress(id) ?? { loaded: 0, total: 0 };
  }

  /** Whether area `id`'s entry-blocking art (its inn, if any) has settled; false before the world loads. */
  entryArtReady(id: number): boolean {
    return this.#regions?.entryReady(id) ?? false;
  }

  /** Called whenever another art piece settles. */
  set onArtProgress(callback: (() => void) | null) {
    this.#onArtProgress = callback;
  }

  /** Debug-only hooks for screenshots and measurements; the simulation never sees them. */
  debugApi() {
    return {
      /** The staged frame's work since the last call (#1060): stage calls, panel redraws and root-list rebuilds. */
      takeFrameWork: () => this.takeFrameWork(),
      setPresentationTime: (seconds: number | null) => {
        if (seconds !== null && !Number.isFinite(seconds)) {
          throw new Error("Presentation time must be finite");
        }
        this.#presentationTime = seconds;
      },
      presentationFrame: () => ({
        environment: this.#lastEnvironment,
        effectNodes: structuredClone(this.#lastEffectNodes),
        work: this.#lastWork,
        vertexColoredNodes: this.#lastSceneryNodes.filter((node) => node.geometry.kind === "mesh" && node.geometry.colors?.length).length,
        emissiveNodes: this.#lastSceneryNodes.filter((node) => node.emissive !== undefined).length,
      }),
      viewpoints: Object.keys(DEBUG_VIEWPOINTS),
      flyTo: (name: string) => {
        const viewpoint = DEBUG_VIEWPOINTS[name];
        if (!viewpoint) {
          throw new Error(`Unknown viewpoint ${name}.`);
        }
        this.#flyTo = viewpoint;
      },
      flyToPose: (eye: Vec3, target: Vec3) => {
        this.#flyTo = { eye, target };
      },
      /** Stops (or resumes) advancing the world source, so captures at several poses see one zone state (#753). */
      holdWorld: (hold: boolean) => {
        this.#holdWorld = hold;
      },
      follow: () => {
        this.#flyTo = null;
      },
      controls: () => ({ mode: this.#cameraMode, pointer: this.#pointerOwner, heading: this.#orbit.heading }),
      /** Named areas with their bounds in simulation units, for tools that walk the regions. */
      areas: () => structuredClone(this.#areas),
      /** Starts every area's art, which otherwise streams in around the character. */
      fetchAllArt: () => this.#regions?.fetch(null),
      /** An orthographic map of the world alone (no units or fog) over an XZ rectangle in metres; `null` leaves it. */
      planView: (plan: PlanView | null) => {
        this.#plan = plan;
        this.#rootRegistry.markDirty();
        if (!plan) {
          this.#camera.fov = this.#perspectiveFov;
          this.#camera.updateProjectionMatrix();
          for (const root of this.#planHidden) { root.visible = true; }
          this.#planHidden.clear();
        }
      },
      reliefAt: (x: number, z: number) => {
        if (!this.#reliefAt) {
          throw new Error("Scenery is not loaded");
        }
        return this.#reliefAt(x, z);
      },
      /** Node IDs the last frame drew for one unit, e.g. `unit-creature-108`. */
      /** Includes `<identity>-model` while the unit is drawn by its imported Quaternius model. */
      unitNodeIds: (identity: string) => [
        ...this.#lastUnitNodes.filter((node) => node.id.startsWith(`${identity}-`)).map((node) => node.id),
        ...(this.#models.has(identity) ? [`${identity}-model`] : []),
      ],
      /** The imported model's actual clip, clip time, root and bone world positions, or null. */
      unitModelPose: (identity: string) => this.#models.pose(identity),
      /** The viewer's layered clips last frame (#384), including a local wind-up (Atlas 6.6.10), or null. */
      viewerAnimation: () => structuredClone(this.#viewerAnimation),
      /** Copies of actual submitted geometry transforms, never a simulation control. */
      unitNodes: (identity: string) => structuredClone(this.#lastUnitNodes.filter((node) => node.id.startsWith(`${identity}-`))),
      /** The walk-in inn (#158): whether its bundle loaded and which entries the last frame showed. */
      inn: () => this.#inn?.observe("greyhaven.inn") ?? null,
      /** Elden's private home (#158), likewise. */
      marshalHouse: () => this.#inn?.observe("greyhaven.marshal-house") ?? null,
      /** The Outpost kit buildings (#420): whether their modules loaded and which are placed. */
      outpost: () => this.#outpost?.observe() ?? null,
      fortifications: () => this.#fortifications?.observe() ?? null,
      /** The Battle Royale fortress kit (#591): whether it is drawn, its entries, pieces and drawn levels. */
      fortress: () => this.#fortress?.observe() ?? null,
      /** The market square's settlement props (#421): whether they loaded and which are placed. */
      settlement: () => this.#settlement?.observe() ?? null,
      /** Greyhaven's trees, rocks and bushes (#49): whether the bundle loaded, and how many instances each entry level draws. */
      nature: () => this.#nature?.observe() ?? null,
      /** Greyhaven's ladders, ropes and ivy (#621): whether the climbing kit loaded, and how many instances each part level draws. */
      climbing: () => this.#climbing?.observe() ?? null,
      /** Greyhaven's highland crags (#668): whether the crag kit loaded, and how many instances each module level draws. */
      crag: () => this.#crag?.observe() ?? null,
      /** The limestone blocks Greyhaven's rocks are drawn as (asset-tooling#536), likewise. */
      limestone: () => this.#inventoryKit("limestone")?.observe() ?? null,
      /** The stones along Greyhaven's watercourses (asset-tooling#538), likewise. */
      river: () => this.#inventoryKit("river")?.observe() ?? null,
      /** Any ordinary placed kit of the shared inventory (#683) by name, likewise. */
      placedKit: (name: string) => this.#inventoryKit(name)?.observe() ?? null,
      ground: () => this.#ground?.observe() ?? null,
      /** The grass field (#832): resident cells, clumps and triangles drawn last frame, or null without one. */
      grass: () => this.#grass?.observe() ?? null,
      /** The walk-in cottages (#843) the viewer stands in, and how many Outpost kit parts are cottage roofs. */
      cottages: (feet: [number, number, number]) => ({ inside: [...(this.#inn?.cottagesInside(feet) ?? [])], roofs: this.#outpost?.cottageRoofCount() ?? 0 }),
      /** Applies a world detail level as the setting would. */
      setWorldDetail: (detail: WorldDetail) => this.setWorldDetail(detail),
      /** The static groups (#526): how many exist, are resident, and of the merged ones are in their batches. */
      staticStream: () => this.#sceneryBatches?.observe() ?? null,
      /** The ambient animals (#419): whether they loaded, and each one's entry, clip and feet. */
      ambient: () => this.#ambient?.observe() ?? null,
      /** The strike and spell effect bundles (#894): whether they loaded, their entries and the effects last shown. */
      kitEffects: () => this.#kitEffects.observe(),
      /** The units of the last projection (kind, id, creature family, position in metres). */
      projectedUnits: () => this.#lastProjectedUnits,
      /** The projected animal (wolf, boar, vermin) nearest to a point in metres, or null. */
      nearestAnimal: (x: number, z: number) => nearestAnimal(this.#lastProjectedUnits, x, z),
      stats: () => ({
        presentationFingerprint: this.#presentationFingerprint,
        buildMs: Number(this.#buildMs.toFixed(1)),
        scene: this.#scene?.stats ?? null,
        frame: this.#overlay.latest,
        self: this.#lastSelf,
      }),
      /** The decoded projection the last frame drew: its tick, viewer and entities in units. */
      projection: () => {
        const projection = this.#lastProjection;
        return projection && {
          tick: Number(projection.tick),
          viewerId: projection.viewerId,
          entities: projection.entities.map(({ kind, entityId, position, facing }) => ({ kind, id: entityId, position: [...position], facing })),
        };
      },
    };
  }
}

/** The `use-door` that flips `door` from its latest projected state. */
function doorToggle(projection: ZoneSnapshot, door: number): WorldCommand {
  return { kind: "use-door", door, open: !projection.doors.some((view) => view.door === door && view.open) };
}

/** The viewer's chosen headwear for the imported Ranger (#374); no choice is no hat. */
function viewerHeadwear(look: ViewerLook): Headwear {
  const style = look.headwear;
  return style === "class" || style === "wayfarer-hood" || style === "ranger-cap" || style === "ironcrest-helm" ? style : "none";
}

/** The viewer's hair (#376); a look without a choice keeps the starter hair. */
function viewerHair(look: ViewerLook): Hair {
  return { style: look.hair ?? "parted", color: look.hairColor ?? "brown" };
}
