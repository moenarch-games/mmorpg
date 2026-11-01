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
import { projectionBytes, type EntityState, type StrikeHeight, type ZoneSnapshot } from "../replication";
import { DebugOverlay, FrameRate } from "./debug-overlay";
import { ENVIRONMENT, DAY_SECONDS, ZONE_DAY_TICKS, ZONE_START_TIME, environmentAt, zoneSkySeconds } from "./environment";
import { Minimap, buildMinimapLayers, dispositionOf, type MinimapElements, type MinimapUnit } from "./minimap";
import { MovementOutbox, type MovementInput } from "./movement-outbox";
import {
  IDLE_INTENT,
  OrbitCamera,
  PIXELS_PER_WHEEL_LINE,
  clearSightLine,
  movementInput,
  type DragMode,
  type HeldIntent,
  type Vec3,
} from "./orbit-camera";
import { SceneryFrame, buildSceneryScene, type SceneryScene } from "./scenery-nodes";
import { ImpactEffects, impactCues } from "./impact-effects";
import { RaidSmoke } from "./raid-smoke";
import { type AidTask, reachableAidTask } from "./raid-aid";
import { type RaidPrepSpots, reachablePrepStep } from "./raid-prep";
import { RaidSigns } from "./raid-signs";
import { WalkInBuildings } from "./walk-in-buildings";
import { KitModules, OUTPOST_BUNDLE, SETTLEMENT_BUNDLE, SHRINE_BUNDLE, type KitPlacement } from "./kit-modules";
import { CompositionKit } from "./composition-kit";
import { Nature } from "./nature";
import { AmbientAnimals } from "./ambient-animals";
import { ArtPrefetch } from "./art-prefetch";
import { SkyLayer } from "./sky";
import { SharedUnits, type AnimationBlend, type ImportedModelKind, type Loadout, type ViewerAnimation, type ViewerLook } from "./unit-presenter";

import { BagsPanel, type BagsElements } from "./units/bags-panel";
import { CharacterPane, type CharacterElements } from "./units/character-pane";
import { useAbilitySlot } from "./units/abilities";
import { DirectedStrikeAim, HEROIC_STRIKE_ABILITY, canStrikeDirected, directedStrikeCommand } from "./units/directed-strike";
import { ShieldGuardHold, canGuard, guardCommand } from "./units/shield-guard";
import { ClassHud } from "./units/class-hud";
import { CombatHud } from "./units/combat-hud";
import { LootPanel, type LootElements } from "./units/loot-panel";
import { WaystonePanel, type WaystoneElements } from "./units/waystone-panel";
import { VendorPanel, type VendorElements } from "./units/vendor-panel";
import { QuestPanel, type MarkerPoint, type QuestElements } from "./units/quest-panel";
import { ChatFrame, type ChatElements } from "./units/chat-frame";
import { nearestAnimal, projectedUnit, type ProjectedUnit } from "./units/projected-units";
import { ProgressionHud } from "./units/progression-hud";
import { PointerClick, SecondaryClick } from "./units/targeting";
import { WorldPicking } from "./units/world-picking";
import { QuaterniusUnits, type FaceFeature, type Hair, type Headwear } from "./units/quaternius-units";
import type { ContentCatalog } from "./catalog";
import type { ZoneWorld } from "./world-source";
import type { AudioPreferences, WorldSound } from "./world-sound";
import { touchContext, type TouchContext } from "../input/touch-layout";
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
  chat: ChatElements;
  /** Action bar, unit frames, cast bars and combat text. */
  classHud: HTMLElement;
};

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
  lake: { eye: [30, 20, -30], target: [56, 11, -56] },
  vale: { eye: [0, 70, 95], target: [0, 0, -10] },
};

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
  /** The areas the Sunken Shrine's kit modules (#431) stand in. */
  readonly #shrineAreas: Set<number>;
  /** The areas the Outpost kit buildings (#420) stand in. */
  readonly #outpostAreas: Set<number>;
  /** The areas the market square's settlement props (#421) stand in. */
  readonly #settlementAreas: Set<number>;
  /** The areas the ambient animals (#419) pace in. */
  readonly #ambientAreas: Set<number>;
  readonly #prefetch: ArtPrefetch;

  constructor(world: ZoneWorld, models: QuaterniusUnits, inn: WalkInBuildings, shrine: KitModules, outpost: KitModules, settlement: KitModules, ambient: AmbientAnimals, onProgress: () => void) {
    this.#world = world;
    const innProp = world.scenery.scenery.props.find((prop) => prop.kind === "inn");
    this.#innArea = innProp ? world.scenery.areaAt(innProp.position[0], innProp.position[2])?.id ?? null : null;
    const unit = world.scenery.scenery.unitsPerMetre;
    const areas = (placements: readonly KitPlacement[]) => new Set(placements.flatMap(({ matrix }) => {
      const area = world.scenery.areaAt(matrix[12]! * unit, matrix[14]! * unit);
      return area ? [area.id] : [];
    }));
    this.#shrineAreas = areas(world.scenery.shrine.placements);
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
      (piece) => (piece === "inn" ? inn.load() : piece === "shrine" ? shrine.load() : piece === "outpost" ? outpost.load() : piece === "settlement" ? settlement.load()
        : piece === "ambient" ? ambient.load() : piece === "residents" ? models.preloadResidents() : models.preload(piece as ImportedModelKind)),
      (idle) => {
        if (idle) { THREE.Cache.clear(); }
        onProgress();
      },
    );
  }

  /** The pieces area `id` needs (every piece for `null`). */
  #pieces(id: number | null): string[] {
    if (id === null) {
      return ["Ranger", "Wolf", "Rat", "Pig", "residents", "inn", "shrine", "outpost", "settlement", "ambient"];
    }
    return [
      "Ranger",
      ...this.#world.scenery.regionModels(id),
      ...(id === this.#innArea ? ["inn"] : []),
      ...(this.#shrineAreas.has(id) ? ["shrine"] : []),
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
    const near = home ? areas.filter((area) => area.id !== entry && boxGapSquared(area, home) <= STREAM_RADIUS_UNITS ** 2) : [];
    this.#prefetch.plan({
      foreground: this.#pieces(entry),
      background: [...others, ...near.map((area) => area.id)].flatMap((id) => this.#pieces(id)),
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
    for (const area of areas) {
      if (boxGapSquared(area, { minXz: at, maxXz: at }) <= STREAM_RADIUS_UNITS ** 2) {
        this.fetch(area.id);
      }
    }
  }
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
  /** The Sunken Shrine (#431), drawn from asset-tooling's shrine kit once loaded. */
  #shrine: KitModules | null = null;
  /** The Outpost kit buildings (#420), drawn from asset-tooling's Outpost kit once loaded; their blockouts until then. */
  #outpost: KitModules | null = null;
  /** The market square's settlement props (#421), drawn from asset-tooling's bundle once loaded; their boxes until then. */
  #settlement: KitModules | null = null;
  /** The Battle Royale fortress (#591), drawn from asset-tooling's fortress kit once loaded; its graybox until then. */
  #fortress: CompositionKit | null = null;
  /** Greyhaven's cosmetic cat, dog and rodent (#419), drawn once their bundle loads. */
  #ambient: AmbientAnimals | null = null;
  /** Greyhaven's trees, rocks and bushes (#49), drawn from asset-tooling's bundle once loaded; the procedural ones until then. */
  #nature: Nature | null = null;
  #regions: RegionArt | null = null;
  #onArtProgress: (() => void) | null = null;
  /** Authored hit/block bursts (#52) on zone-confirmed events; one permanent imported root. */
  readonly #impacts = new ImpactEffects();
  /** Smoke over a raided barn (#530); one permanent imported root. */
  readonly #raidSmoke = new RaidSmoke();
  readonly #raidSigns = new RaidSigns();
  /** The raided barn's roof centre and smoke span in metres, by objective (#530). */
  #raidRoof: { key: string; top: [number, number, number]; span: [number, number] } | null = null;
  #reachableDoor: (x: number, z: number) => number = () => 0;
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
  #units: { viewerId: number; units: SharedUnits; pushed: ZoneSnapshot | null; residents: boolean; kitPlatforms: boolean } | null = null;
  /** Combat sounds (#447), once the bundle loaded; silent until then. */
  #sound: WorldSound | null = null;
  #audioPreferences: AudioPreferences | null = null;
  /** Camera heading of the last frame; health bars face it. */
  #barYaw = 0;
  #lastCamera: RendererCamera | null = null;
  readonly #combatHud: CombatHud;
  readonly #progressionHud: ProgressionHud;
  readonly #classHud: ClassHud;
  readonly #bags: BagsPanel;
  readonly #character: CharacterPane;
  readonly #loot: LootPanel;
  readonly #waystone: WaystonePanel;
  readonly #vendor: VendorPanel;
  readonly #quests: QuestPanel;
  readonly #chat: ChatFrame;
  readonly #intents: { intent: Intent; onSent: ((sequence: number | null) => void) | undefined }[] = [];
  readonly #secondaryClick = new SecondaryClick();
  readonly #primaryClick = new PointerClick(0);
  #lastLinkConnected = true;
  #presentationTime: number | null = null;
  #lastEnvironment: RendererEnvironment | null = null;
  #lastInstanceBatches: RendererInstanceBatch[] = [];
  readonly #picking = new WorldPicking();
  #lastSceneryNodes: readonly RendererSceneNode[] = [];
  #pendingNpc: { target: EntityRef; sequence: number } | null = null;
  #scene: SceneryScene | null = null;
  #catalog: ContentCatalog | null = null;
  #zoneName = ZONE_NAME;
  #arena = false;
  /** The unit nodes of the last frame, for debug readouts. */
  #lastUnitNodes: readonly RendererSceneNode[] = [];
  #lastEffectNodes: readonly RendererSceneNode[] = [];
  /** The projected units of the last frame, for debug readouts. */
  #lastProjectedUnits: readonly ProjectedUnit[] = [];
  #sceneryFrame: SceneryFrame | null = null;
  #presentationFingerprint: string | null = null;
  #reliefAt: ((x: number, z: number) => number) | null = null;
  #minimap: Minimap | null = null;
  #buildMs = 0;
  #orbit = new OrbitCamera();
  #drag: Drag | null = null;
  /** Fingers on the canvas, and the spread of a two-finger pinch zoom in progress (#434). */
  readonly #touches = new Map<number, { x: number; y: number }>();
  #pinch: number | null = null;
  /** A held shield guard whose lane follows the pointer (#156). */
  readonly #guardHold = new ShieldGuardHold();
  /** A held directed Heroic Strike being aimed with the pointer (#154). */
  readonly #strikeAim = new DirectedStrikeAim();
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
  #lastWork: RendererWorkObservations | null = null;
  /** While `prepare` runs its frame: the renderer's pending preparations. */
  #preparing: Promise<unknown>[] | null = null;
  #occluders: Box[] = [];
  /** The viewer's projected unit as last drawn, for debug readouts. */
  #lastSelf: { x: number; z: number; facing: number } | null = null;
  /** The projection the last frame drew, for debug readouts. */
  #lastProjection: ZoneSnapshot | null = null;
  /** The first frame of a session picks a heading whose view is not inside a building. */
  #framePending = false;

  constructor(renderer: ThreeSceneRenderer, camera: THREE.PerspectiveCamera, elements: WorldViewElements) {
    this.#renderer = renderer;
    this.#models.configureRenderer(renderer);
    this.#camera = camera;
    this.#elements = elements;
    this.#reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    this.#overlay = new DebugOverlay(elements.overlay);
    this.#sky = new SkyLayer(elements.sky, ENVIRONMENT);
    this.#combatHud = new CombatHud(elements.unitStatus, elements.combatFeedback);
    this.#classHud = new ClassHud(elements.classHud, {
      onUse: (slot) => this.useSlot(slot),
    });
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
    this.#chat = new ChatFrame(elements.chat, (command) => this.queueIntent(() => command), () => elements.canvas.focus());
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

  /** Builds the static scene and the minimap image once per loaded world. */
  load(world: ZoneWorld): void {
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
    this.#presentationFingerprint = scenery.presentationFingerprint;
    this.#reliefAt = world.scenery.reliefAt;
    this.#scene = buildSceneryScene(scenery, world.scenery.staticScene);
    this.#nature = new Nature(world.scenery.nature);
    void this.#nature.load();
    this.#restClip = (kind) => world.scenery.importedRestClip(kind);
    this.#inn = new WalkInBuildings(world.scenery.buildings.presenters());
    this.#shrine = new KitModules(SHRINE_BUNDLE, "shrine kit", world.scenery.shrine.entries, world.scenery.shrine.placements);
    this.#outpost = new KitModules(OUTPOST_BUNDLE, "Outpost kit", world.scenery.outpost.entries, world.scenery.outpost.placements);
    this.#settlement = new KitModules(SETTLEMENT_BUNDLE, "settlement props", world.scenery.settlement.entries, world.scenery.settlement.placements);
    // The arena's fortress kit (#591) loads at once: the whole fortress is in view from the courtyard.
    this.#fortress = world.scenery.fortress ? new CompositionKit(world.scenery.fortress) : null;
    void this.#fortress?.load(THREE.MathUtils.degToRad(this.#camera.fov));
    // Greyhaven's art streams in region by region (#240): the selection screen fetches the
    // starting area's pieces, and play fetches each area the player approaches.
    this.#ambient = new AmbientAnimals(world.scenery.ambient);
    this.#regions = new RegionArt(world, this.#models, this.#inn, this.#shrine, this.#outpost, this.#settlement, this.#ambient, () => this.#onArtProgress?.());
    this.#reachableDoor = world.scenery.buildings.reachableDoor;
    this.#reachablePieces = world.scenery.reachablePieces;
    this.#raidArea = world.scenery.raidArea;
    this.#raidAid = world.scenery.raidAid;
    this.#raidPrep = world.scenery.raidPrep;
    this.#raidRoof = null;
    this.#waystone.load(world.scenery.waystoneAt, world.scenery.waystones);
    this.#units = null;
    this.#sceneryFrame = new SceneryFrame(this.#scene);
    this.#minimap = new Minimap(this.#elements.minimap, buildMinimapLayers(scenery, ENVIRONMENT));
    this.#occluders = occluders(world);
    this.#buildMs = performance.now() - started;
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
    // The most recent press owns the pointer gesture (Atlas 5.2.37): the sword lowers a held guard.
    this.endGuard();
    this.#strikeAim.begin();
    this.#classHud.showStrikeAim(this.#strikeAim.aim());
    // The sword winds up while the player aims (Atlas 6.6.10), toward the aimed slash (#582);
    // only the zone's strike swings it.
    this.#units?.units.prepare(HEROIC_STRIKE_ABILITY, this.#seconds);
    this.#aimWindUp();
  }

  /** The held directed strike's wind-up shows the slash toward its current aim (#582). */
  #aimWindUp(): void {
    const aim = this.#strikeAim.aim();
    if (aim) {
      this.#units?.units.aimPreparation(aim);
    }
  }

  /** A deliberate release sends one directed strike toward the view's heading; the zone resolves it. */
  releaseDirectedStrike(): void {
    const aim = this.#strikeAim.release();
    this.#classHud.showStrikeAim(null);
    const catalog = this.#catalog;
    if (!aim || !catalog) {
      this.#units?.units.abandonPreparation();
      return;
    }
    const facing = this.#orbit.facing();
    // The Warden turns to strike; later movement keeps that heading until steered.
    this.#movementFacing = facing;
    this.#queuePrepared((projection) => directedStrikeCommand(projection, catalog, facing, aim));
  }

  /** Blur, menus, retired input or a superseding gesture: no strike, and no wind-up. */
  cancelDirectedStrike(): void {
    this.#strikeAim.cancel();
    this.#classHud.showStrikeAim(null);
    this.#units?.units.abandonPreparation();
  }

  /**
   * One weapon swing where the character faces (#517): the left mouse button, gamepad X or the
   * touch Attack pill. The zone resolves what it hits; the dead swing nothing.
   */
  swing(): void {
    if (!this.#lastProjection || this.#lastProjection.viewer.dead) {
      return;
    }
    this.queueIntent(() => ({ kind: "swing", facing: this.#movementFacing }));
  }

  /** Uses action-bar slot `slot` (1–8); the viewer winds up at once while the zone decides. */
  useSlot(slot: number): void {
    this.#queuePrepared((projection) => (this.#catalog ? useAbilitySlot(slot, projection, this.#catalog) : null));
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
   * pointer motion moves it. The press supersedes an unreleased strike preparation, which never fires.
   */
  beginGuard(): void {
    const projection = this.#lastProjection;
    if (!projection || !canGuard(projection)) {
      return;
    }
    this.cancelDirectedStrike();
    this.#guardHold.begin();
    this.#sendGuardLane();
  }

  /** A deliberate release, any retirement or a superseding gesture lowers the guard the zone holds. */
  endGuard(): void {
    if (this.#guardHold.end()) {
      this.queueIntent(() => guardCommand(null));
    }
  }

  #sendGuardLane(): void {
    const lane = this.#guardHold.pending();
    if (lane !== undefined) {
      this.queueIntent(() => guardCommand(lane));
    }
  }

  /** Starts a session: fresh camera, intent and animation state. */
  enter(input: WorldInput, projection: ZoneSnapshot | null): void {
    this.cancelDirectedStrike();
    this.#guardHold.end();
    this.#orbit = new OrbitCamera();
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
    this.#chat.reset();
    this.#shownArea = null;
    this.#flyTo = null;
    this.#framePending = true;
    this.#lastSelf = null;
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
    this.#sky.show(true);
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
  #frameFirstView(focus: Vec3, facing: number, groundAt: (x: number, z: number) => number): void {
    for (const step of [0, 2, -2, 1, -1, 3, -3, 4]) {
      this.#orbit.lookAlong(facing + (step * Math.PI) / 4);
      const view = this.#orbit.view(focus, groundAt);
      if (!obstructed(this.#occluders, view.target, view.eye)) {
        return;
      }
    }
    this.#orbit.lookAlong(facing);
  }

  /** Drops every imported unit model and takes them out of the shared renderer scene. */
  #clearModels(): void {
    this.#models.clear();
    const { roots, changed } = this.#models.roots();
    if (changed && this.#lastCamera) {
      this.#renderer.renderImportedRoots({ camera: this.#lastCamera, roots: [...roots, ...(this.#inn?.roots().roots ?? []), ...(this.#shrine?.roots().roots ?? []), ...(this.#outpost?.roots().roots ?? []), ...(this.#settlement?.roots().roots ?? []), ...(this.#fortress?.roots().roots ?? []), ...(this.#ambient?.roots().roots ?? []), ...(this.#nature?.roots().roots ?? []), this.#impacts.root, this.#raidSmoke.root, this.#raidSigns.root] });
    }
  }

  leave(): void {
    this.cancelDirectedStrike();
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
    this.#endDrag();
    this.#sky.show(false);
    this.#overlay.hide();
  }

  /**
   * The selection turntable (#362): the real Ranger in its idle clip standing at `feet`, dressed as
   * the viewer would enter (class loadout, headwear, hair), through the same imported-model path as
   * the world. False before a world loaded, while the model loads or after it failed; the page then
   * draws its procedural preview. Entering the world drops it like any unit no longer projected.
   */
  selectionPreview(look: ViewerLook, feet: [number, number, number], yaw: number, height: number, seconds: number,
    camera: RendererCamera): boolean {
    if (!this.#regions || !this.#models.ready("Ranger")) { return false; }
    const clip = this.#restClip("Ranger");
    const length = this.#models.clipLength("Ranger", clip) ?? 1;
    this.#models.update([{
      id: "selection-preview", kind: "Ranger", entity: { kind: "player", id: 0 }, feet, yaw, pitch: 0, heightMetres: height,
      clip, time: seconds % length, lower: [clip, seconds % length], layers: [{ clip, time: seconds % length, weight: 1, mask: "all" }], tint: null,
      loadout: look.classId ? CLASS_LOADOUTS[look.classId] : null, resident: null, figure: null, variety: null,
      outfit: look.classId ? { preset: `class.${look.classId}`, replaced: [] } : null, headwear: true, swing: null, bow: null,
      sheathed: [false, false], throwable: null,
    }], { headwear: viewerHeadwear(look), hair: viewerHair(look), face: look.face ?? {} });
    const { roots, changed } = this.#models.roots();
    if (changed) { this.#renderer.renderImportedRoots({ camera, roots }); }
    return true;
  }

  /** The face features the loaded Ranger offers (#411): undefined while loading, null if it has none. */
  faceFeatures(): readonly FaceFeature[] | null | undefined {
    return this.#models.faceFeatures();
  }

  /** Queues an intent for the next frame; the zone decides what happens. */
  queueIntent(intent: Intent, onSent?: (sequence: number | null) => void): void {
    this.#intents.push({ intent, onSent });
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
    this.#vendor.toggle();
  }

  closeVendor(): boolean { return this.#vendor.close(); }

  /** Talks to the selected or nearest quest giver: the NPC dialog. */
  toggleQuestDialog(): void {
    this.#quests.toggleDialog();
  }

  /**
   * Interact (T / gamepad Start): talk to a selected NPC; without one, swing the door in reach
   * (#158), repair a damaged structure piece in reach (#305, free; moving stops it), or help a
   * lost night raid's aftermath in reach (#531);
   * otherwise open the quest dialog, which says whom to approach.
   */
  interact(): void {
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
    const door = at && !talking ? this.#reachableDoor(at[0], at[2]) : 0;
    if (door !== 0) {
      this.queueIntent((latest) => doorToggle(latest, door));
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
    this.toggleQuestDialog();
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
    this.#chat.open();
  }

  closeLoot(): boolean { return this.#loot.close(); }

  closeBags(): boolean { return this.#bags.close(); }

  closeCharacter(): boolean { return this.#character.close(); }

  /** Whether bags, the character pane, loot or the vendor are open: a non-blocking overlay over gameplay controls. */
  get panelOpen(): boolean {
    return this.#bags.open || this.#character.open || this.#loot.open || this.#vendor.open
      || this.#quests.dialogOpen || this.#quests.logOpen;
  }

  toggleOverlay(): void {
    this.#overlay.toggle();
  }

  #input({ held, jumps, rolls }: WorldInput): MovementInput {
    const input = movementInput(held, this.#orbit.facing(), this.#movementFacing, jumps, this.#drag?.mode ?? "none");
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

  /** One frame: intent out, ticks, projection in, scene drawn. Throws when the projection lacks the viewer. */
  frame(world: ZoneWorld, input: WorldInput, look: ViewerLook, deltaSeconds: number, now: number): void {
    const scene = this.#scene;
    const sceneryFrame = this.#sceneryFrame;
    if (!scene || !sceneryFrame) {
      throw new Error("The world view has no scenery loaded.");
    }
    const { source, scenery, catalog } = world;
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
    const receivedProjections = source.advance(deltaSeconds);
    profilePhase("simulation");
    const connected = source.linkState() === "connected";
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
    }
    const projection = source.latestProjection();
    if (!projection) {
      throw new Error("The world source has no projection for the joined player.");
    }
    this.#lastProjection = projection;
    // Death already lowered the guard in the zone; a later life needs a new press.
    if (projection.viewer.dead) {
      this.#guardHold.end();
    }
    const pendingNpc = this.#pendingNpc;
    if (pendingNpc && projection.acknowledgedSequence >= pendingNpc.sequence) {
      this.#pendingNpc = null;
      if (sameEntity(projection.viewer.target, pendingNpc.target)) {
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
    const animate = this.#animate;
    this.#seconds += deltaSeconds;
    if (this.#units?.viewerId !== projection.viewerId) {
      const units = new SharedUnits(world.scenery.unitPresenter(projection.viewerId));
      for (const [kind, clips, descriptor] of this.#loadedModels) { units.modelReady(kind, clips, descriptor); }
      if (this.#blend) { units.setBlend(this.#blend); }
      if (this.#bodyOverlay) { units.setBodyOverlay(true); }
      this.#units = { viewerId: projection.viewerId, units, pushed: null, residents: false, kitPlatforms: false };
    }
    const units = this.#units.units;
    if (!this.#units.kitPlatforms && this.#fortress?.drawn) {
      // The fortress kit's lifts (#591) replace the presenter's pillars once drawn.
      this.#units.kitPlatforms = true;
      units.setKitPlatforms(this.#fortress.platforms);
    }
    this.#regions?.approach(entitiesSelf(projection), scenery.scenery.areas);
    for (const [kind, clips, descriptor] of this.#models.drainLoaded()) {
      this.#loadedModels.push([kind, clips, descriptor]);
      units.modelReady(kind, clips, descriptor);
    }
    if (!this.#units.residents && this.#models.residentsLoaded()) {
      this.#units.residents = true;
      units.residentsReady();
    }
    for (const received of receivedProjections) {
      this.#classHud.receive(received, now);
      if (connected) { this.#pushProjection(units, received); }
    }
    // The join projection (and a paused source's) reaches the presenter without an advance.
    if (this.#units.pushed !== projection) { this.#pushProjection(units, projection); }
    // The pushed projections' sound cues (#447); taken even while silent so none play late.
    const sounds = units.takeAudio();
    this.#sound?.apply(sounds);
    this.#orbit.update(deltaSeconds, !animate);
    profilePhase("projection");
    const { unitsPerMetre } = scenery.scenery;
    const sampled = source.sample();
    const presented = units.frame(sampled, { seconds: this.#seconds, dt: deltaSeconds, reducedMotion: !animate, barYaw: this.#barYaw, look });
    this.#fortress?.place(presented.platforms);
    const viewerModel = presented.imported.find((unit) => unit.entity.kind === "player" && unit.entity.id === projection.viewerId);
    this.#viewerClip = viewerModel?.clip ?? null;
    this.#viewerAnimation = viewerModel ? { upper: [viewerModel.clip, viewerModel.time], lower: viewerModel.lower, layers: viewerModel.layers } : null;
    const nodes: RendererSceneNode[] = [];
    const projected: ProjectedUnit[] = [];
    const others: MinimapUnit[] = [];
    const anchors = new Map<string, { x: number; feetY: number; z: number }>();
    let self: { focus: Vec3; x: number; feetY: number; z: number; facing: number } | null = null;
    const entities = new Map(sampled.map((entity) => [entityKey({ kind: entity.kind, id: entity.entityId }), entity]));
    for (const unit of presented.units) {
      const entity = entities.get(entityKey(unit.entity));
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
    this.#models.update(presented.imported, { headwear: viewerHeadwear(look), hair: viewerHair(look), face: look.face ?? {} });
    const intensity = animate ? "strong" : "subtle";
    this.#impacts.preload(intensity);
    for (const received of receivedProjections) {
      for (const cue of impactCues(received)) {
        const unit = presented.units.find((candidate) => sameEntity(candidate.entity, cue.target));
        if (unit) { this.#impacts.spawn(cue.family, intensity, [unit.feet[0], unit.centreY, unit.feet[2]], now); }
      }
    }
    this.#impacts.update(now);
    const roof = this.#roofOf(projection.raid ? this.#raidArea : null, scenery.scenery);
    this.#raidSmoke.update(projection.raid, roof?.top ?? null, roof?.span ?? [0, 0], now);
    const unitNodes = presented.nodes.filter((node) => !node.id.startsWith("fx-"));
    profilePhase("units");
    if (!self) {
      throw new Error("The projection is missing the viewer's own unit.");
    }
    this.#combatHud.update(projection, catalog, now);
    this.#classHud.update(projection, catalog, now);
    this.#progressionHud.update(projection, now);
    this.#bags.update(projection);
    this.#character.update(projection);
    this.#loot.update(projection);
    this.#lastSelf = { x: self.x, z: self.z, facing: self.facing };
    this.#inn?.update(projection.doors, [self.x, self.feetY, self.z], deltaSeconds, !animate);
    this.#ambient?.update(this.#seconds, animate);
    this.#raidSigns.update(projection.raid, this.#raidPrep, scene.surfaceY, scenery.scenery.unitsPerMetre);
    if (this.#framePending) {
      this.#framePending = false;
      this.#frameFirstView(self.focus, self.facing, scene.surfaceY);
    }
    const orbit = this.#orbit.view(self.focus, scene.surfaceY);
    // The fortress's walls and roofs (#552) pull the eye in; Greyhaven's camera never collides.
    const view = this.#flyTo ?? (this.#arena ? clearSightLine(orbit, this.#occluders) : orbit);
    this.#camera.position.set(...view.eye);
    this.#camera.lookAt(...view.target);
    this.#camera.updateMatrixWorld(true);
    if (this.#sound) {
      const right = new THREE.Vector3().setFromMatrixColumn(this.#camera.matrixWorld, 0);
      this.#sound.listen(view.eye, [right.x, right.y, right.z]);
      this.#sound.pump();
    }
    this.#fortress?.update(view.eye);
    const frame = sceneryFrame.nodes(view.eye, { seconds: this.#seconds, animate, viewer: [self.x, self.feetY, self.z], kits: { outpost: this.#outpost?.drawn ?? false, settlementProps: this.#settlement?.drawn ?? false, nature: this.#nature?.drawn ?? false, fortress: this.#fortress?.drawn ?? false } });
    this.#nature?.update(this.#camera, deltaSeconds, animate);
    this.#lastSceneryNodes = frame.nodes;
    this.#barYaw = Math.atan2(view.target[0] - view.eye[0], view.target[2] - view.eye[2]);
    nodes.push(...frame.nodes, ...unitNodes);
    this.#lastEffectNodes = presented.nodes.filter((node) => node.id.startsWith("fx-"));
    nodes.push(...this.#lastEffectNodes);
    this.#lastUnitNodes = unitNodes;
    this.#lastProjectedUnits = projected;
    const camera: RendererCamera = {
      viewMatrix: [...this.#camera.matrixWorldInverse.elements] as Matrix4Values,
      projectionMatrix: webGpuProjection(this.#camera),
    };
    // The sky follows the zone's clock (#158); a debug presentation time overrides it.
    // Reduced motion steps the sky once per zone hour instead of drifting continuously.
    const hour = BigInt(ZONE_DAY_TICKS / 24);
    const skyTick = animate ? projection.tick : projection.tick - ((projection.tick + BigInt(ZONE_START_TIME)) % hour);
    const seconds = this.#presentationTime ?? zoneSkySeconds(skyTick);
    const environment = environmentAt(ENVIRONMENT, seconds / DAY_SECONDS);
    this.#lastEnvironment = environment;
    this.#lastInstanceBatches = frame.instanceBatches;
    this.#lastCamera = camera;
    profilePhase("scene");
    const models = this.#models.roots();
    const inn = this.#inn?.roots() ?? { roots: [], changed: false };
    const shrine = this.#shrine?.roots() ?? { roots: [], changed: false };
    const outpost = this.#outpost?.roots() ?? { roots: [], changed: false };
    const settlement = this.#settlement?.roots() ?? { roots: [], changed: false };
    const fortress = this.#fortress?.roots() ?? { roots: [], changed: false };
    const ambient = this.#ambient?.roots() ?? { roots: [], changed: false };
    const nature = this.#nature?.roots() ?? { roots: [], changed: false };
    const roots = [...models.roots, ...inn.roots, ...shrine.roots, ...outpost.roots, ...settlement.roots, ...fortress.roots, ...ambient.roots, ...nature.roots, this.#impacts.root, this.#raidSmoke.root, this.#raidSigns.root];
    const rendererFrame = { camera, nodes, instanceBatches: frame.instanceBatches, environment };
    if (this.#preparing) {
      // `prepare` (#547): the first world frame's programs and textures, without drawing it.
      this.#preparing.push(this.#renderer.prepare(rendererFrame, { roots }));
      return;
    }
    if (models.changed || inn.changed || shrine.changed || outpost.changed || settlement.changed || fortress.changed || ambient.changed || nature.changed || !this.#impacts.root.parent || !this.#raidSmoke.root.parent || !this.#raidSigns.root.parent) {
      this.#renderer.renderImportedRoots({ camera, roots });
    }
    const work = this.#renderer.render(rendererFrame);
    this.#lastWork = work;
    profilePhase("render");
    if (profiling()) {
      profileFacts({
        ticks: receivedProjections.length, viewer: [self.x, self.z], heading: this.#orbit.heading, nodes: nodes.length, instanceBatches: frame.instanceBatches.length,
        created: { objects: work.objectCreateCount, geometries: work.geometryCreateCount, materials: work.materialCreateCount, instanceUploads: work.instanceUploadCount },
      });
    }
    this.#quests.placeMarkers(this.#markerPoints(anchors, projection));
    this.#sky.update(this.#horizon(view.eye, view.target), seconds, true);
    // A night raid's approximate disturbance area while its assault runs (#530).
    const area = this.#raidArea;
    const disturbance = area && (projection.raid?.phase ?? 0) > 0
      ? { x: area.x / unitsPerMetre, z: area.z / unitsPerMetre, radius: area.radius / unitsPerMetre }
      : null;
    this.#minimap?.draw({ x: self.x, z: self.z }, this.#orbit.heading, self.facing, others, disturbance);
    this.#frameRate.sample(now);
    this.#overlay.update({
      fps: this.#frameRate.fps,
      frameMs: this.#frameRate.frameMs,
      nodes: nodes.length + frame.instanceBatches.length,
      staticNodes: scene.stats.staticNodes,
      visibleStaticNodes: frame.visibleBatches,
      staticVertices: scene.stats.staticVertices,
      units: presented.units.length,
      work: this.#lastWork,
    }, now);
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
    const bytes = projectionBytes(projection);
    if (bytes && this.#units) {
      units.push(bytes, this.#seconds);
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

  pointerDown(event: PointerEvent): void {
    // A click or drag supersedes an aimed strike that has not been released.
    this.cancelDirectedStrike();
    if (event.pointerType === "touch") {
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
    }
    // The left mouse button swings at once where the character faces (#517); held, it still
    // orbits the view. Selection moved to a right click that never became a turn drag.
    if (event.button === 0 && event.pointerType !== "touch") {
      this.swing();
    }
    if (this.#drag || (event.button !== 0 && event.button !== 2)) {
      return;
    }
    try {
      this.#elements.canvas.setPointerCapture(event.pointerId);
    } catch {
      return;
    }
    // A finger drag looks and turns the character with the view (#434); a tap still selects.
    const mode = event.button === 2 || event.pointerType === "touch" ? "turn" : "orbit";
    this.#drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, mode };
    this.#secondaryClick.down(event);
    this.#primaryClick.down(event);
    this.#elements.canvas.dataset.drag = mode;
  }

  /** Losing focus retires the gesture without selecting. */
  cancelPointer(): void {
    this.cancelDirectedStrike();
    this.endGuard();
    this.#endDrag();
  }

  pointerMove(event: PointerEvent): void {
    this.#strikeAim.track(event.clientX, event.clientY);
    if (this.#strikeAim.active) {
      this.#classHud.showStrikeAim(this.#strikeAim.aim());
      this.#aimWindUp();
    }
    // A camera drag owns the pointer: it turns the Warden without moving the guard's lane.
    this.#guardHold.track(event.clientY, this.#drag !== null);
    if (this.#guardHold.active) {
      this.#sendGuardLane();
    }
    this.#secondaryClick.move(event);
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

  pointerEnd(event: PointerEvent): void {
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
    // A tap, or a right click that never became a turn drag, selects (#517: the left mouse
    // button swings instead).
    const tapped = this.#primaryClick.end(event) && event.pointerType === "touch";
    if (this.#secondaryClick.end(event) || tapped) {
      const rect = this.#elements.canvas.getBoundingClientRect();
      const point = new THREE.Vector2(
        ((event.clientX - rect.left) / rect.width) * 2 - 1,
        1 - ((event.clientY - rect.top) / rect.height) * 2,
      );
      // Every other living player is selectable: a foe to attack or a friend to heal and shield (#303).
      const viewerId = this.#lastProjection?.viewerId;
      const selectablePlayers = new Set(this.#lastProjection?.entities
        .filter((entity) => entity.kind === "player" && entity.entityId !== viewerId && !entity.flags.dead)
        .map((entity) => entity.entityId) ?? []);
      const picked = this.#picking.pick(point, this.#camera, this.#lastSceneryNodes, this.#lastUnitNodes, this.#lastInstanceBatches, this.#models.pickables(), this.#inn?.pickables(), selectablePlayers, { width: rect.width, height: rect.height, pointerType: event.pointerType });
      this.#pendingNpc = null;
      // A click on the door leaf asks the zone to swing it; the zone checks reach (#158).
      if (picked?.kind === "door") {
        this.queueIntent((projection) => doorToggle(projection, picked.id));
      }
      const target = picked?.kind === "door" ? null : picked;
      if (target) {
        this.queueIntent((projection) => projection.entities.some((entity) => entity.kind === target.kind && entity.entityId === target.id)
          ? { kind: "select-target", target } : null, (sequence) => {
          if (target.kind === "npc" && sequence !== null) {
            this.#pendingNpc = { target, sequence };
          }
        });
      }
    }
    if (this.#drag?.pointerId === event.pointerId) {
      this.#endDrag();
    }
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
    this.#secondaryClick.reset();
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
    this.#regions?.plan(entry, others);
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
      follow: () => {
        this.#flyTo = null;
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
      /** The Sunken Shrine kit (#431): whether its modules loaded and which are placed. */
      shrine: () => this.#shrine?.observe() ?? null,
      /** The Outpost kit buildings (#420): whether their modules loaded and which are placed. */
      outpost: () => this.#outpost?.observe() ?? null,
      /** The Battle Royale fortress kit (#591): whether it is drawn, its entries, pieces and drawn levels. */
      fortress: () => this.#fortress?.observe() ?? null,
      /** The market square's settlement props (#421): whether they loaded and which are placed. */
      settlement: () => this.#settlement?.observe() ?? null,
      /** Greyhaven's trees, rocks and bushes (#49): whether the bundle loaded, and how many instances each entry level draws. */
      nature: () => this.#nature?.observe() ?? null,
      /** The ambient animals (#419): whether they loaded, and each one's entry, clip and feet. */
      ambient: () => this.#ambient?.observe() ?? null,
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
