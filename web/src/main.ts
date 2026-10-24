import { onlineCharacterId, hasOnlineCharacterId } from "./world/online-character-identity";
import * as THREE from "three";
import {
  createThreeSceneRenderer,
  type Matrix4Values,
  type RendererSceneNode,
} from "@moritzbrantner/three-d-renderer";
import {
  DEFAULT_CHARACTER_APPEARANCE,
  defaultAppearanceForClass,
  equipmentForAppearance,
  hatOption,
  HAIR_COLOR_OPTIONS,
  HAIR_OPTIONS,
  isHairColor,
  isHairStyle,
  isHatStyle,
  loadCharacter,
  saveCharacter,
  type CharacterAppearance,
  type HatStyle,
} from "./character-customization";
import {
  MAX_CHARACTER_SLOTS,
  PREVIEW_CHARACTER,
  createCharacterPreview,
  draftCharacterPreview,
  enterPreviewWorld,
  initialEntryState,
  isCharacterClassId,
  isCharacterSex,
  type CharacterCreationDraft,
  type CharacterPreview,
  type EntryState,
} from "./character-selection";
import "./styles.css";
import { frameDeltaSeconds } from "./demo-clock";
import { markStartup, startupMarks } from "./startup-marks";
import { installCharacterTurntable } from "./character-turntable";
import {
  characterRosterStorageKey,
  hasRetainedCharacterSaves,
  loadCreatedCharacters,
  mergeStoredRoster,
  saveCreatedCharacters,
} from "./character-roster";
import { characterVisualProfile, type CharacterVisualProfile } from "./character-visuals";
import type { LocalWorld } from "./world/local-world";
import { LocalCharacters, VisitStorage } from "./world/local-characters";
import { actionLabDefinitions, committedArea, committedLevel, createScenarioWorld, loadLocalWorld } from "./world/wasm-runtime";
import { ActionLabPanel, labQuery } from "./world/action-lab";
import "./action-lab.css";
import { scenarioChoice, type ScenarioChoice, type ScenarioSource } from "./world/scenario-source";
import { webTransportConnector } from "./session/webtransport";
import { onlineChoice, type ZoneServer } from "./world/online-config";
import { OnlineZoneSource } from "./world/online-zone-source";
import { BattleRoyaleEntry, matchStatus } from "./world/battle-royale/entry";
import { createArenaWorld, type ArenaWorld } from "./world/battle-royale/world";
import "./world/battle-royale/style.css";
import type { LinkState, ZoneWorld } from "./world/world-source";
import { WorldView, webGpuProjection } from "./world/world-view";
import { attackToggle, nextTabTarget } from "./world/units/targeting";
import { GameControls, type GameAction, type HeldAction, type HoldPhase } from "./input/game-controls";
import { actionBindingLabels } from "./input/binding-labels";
import type { Profile } from "@moritzbrantner/input-bindings";
import { cancelCast, useAbilitySlot } from "./world/units/abilities";
import { padGuardLane } from "./world/units/shield-guard";
import type { ViewerLook } from "./world/unit-presenter";
import { Preferences, type ClientPreferences } from "./settings/preferences";
import { SettingsPanel } from "./settings/settings-panel";
import { defaultBindingProfile, loadBindingProfile, saveBindingProfile } from "./settings/binding-profiles";
import { TouchControls, trackInputMode } from "./input/touch-controls";
import { mergeHeld } from "./input/touch-layout";
import "./character-selection-layout.css";
import "./character-creation.css";
import "./class-hud.css";
import "./settings/settings.css";
import "./touch-controls.css";

function requireElement<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`Tech demo shell is missing ${selector}`);
  }
  return element;
}

const canvas = requireElement<HTMLCanvasElement>("#world");
const characterSelect = requireElement<HTMLElement>("#character-select");
const enterWorldButton = requireElement<HTMLButtonElement>("#enter-world");
const enterWorldLabel = requireElement<HTMLElement>("#enter-world-label");
const enterWorldNote = requireElement<HTMLElement>("#enter-world-note");
const onlineToggle = requireElement<HTMLElement>("#online-toggle");
const playOnlineInput = requireElement<HTMLInputElement>("#play-online");
const onlineServerLabel = requireElement<HTMLElement>("#online-server");
const selectionNote = requireElement<HTMLElement>("#selection-note");
const worldMode = requireElement<HTMLElement>("#world-mode");
const connectionStatus = requireElement<HTMLElement>("#connection-status");
const saveCharacterButton = requireElement<HTMLButtonElement>("#save-character");
const loadCharacterButton = requireElement<HTMLButtonElement>("#load-character");
const saveStatus = requireElement<HTMLElement>("#save-status");
const equipmentList = requireElement<HTMLElement>("#equipment-list");
const characterName = requireElement<HTMLElement>("#character-name");
const characterSubtitle = requireElement<HTMLElement>("#character-subtitle");
const continuationCopy = requireElement<HTMLElement>("#continuation-copy");
const appearanceSummary = requireElement<HTMLElement>("#appearance-summary");
const equipmentSummary = requireElement<HTMLElement>("#equipment-summary");
// The selection sections (#362) open and close independently; toggling changes nothing else.
for (const toggle of document.querySelectorAll<HTMLButtonElement>(".section-toggle")) {
  const panel = requireElement<HTMLElement>(`#${toggle.getAttribute("aria-controls")}`);
  toggle.addEventListener("click", () => {
    const expanded = toggle.getAttribute("aria-expanded") !== "true";
    toggle.setAttribute("aria-expanded", String(expanded));
    panel.hidden = !expanded;
  });
}
const characterLocation = requireElement<HTMLElement>("#character-location");
const fallbackCharacter = requireElement<HTMLElement>("#character-stage-fallback");
const previewSurface = requireElement<HTMLElement>("#preview-surface");
const returnButton = requireElement<HTMLButtonElement>("#return-to-characters");
const rosterPanel = requireElement<HTMLElement>("#roster-panel");
const rosterContainer = requireElement<HTMLElement>("#character-roster");
const rosterStatus = requireElement<HTMLElement>("#roster-status");
const createCharacterButton = requireElement<HTMLButtonElement>("#create-character");
const creationPanel = requireElement<HTMLElement>("#character-creation");
const creationForm = requireElement<HTMLFormElement>("#character-creation-form");
const creationName = requireElement<HTMLInputElement>("#new-character-name");
const creationStatus = requireElement<HTMLElement>("#character-creation-status");
const cancelCreationButtons = [
  requireElement<HTMLButtonElement>("#cancel-character-creation"),
  requireElement<HTMLButtonElement>("#cancel-character-creation-secondary"),
];
const selectionActions = requireElement<HTMLElement>("#selection-actions");
const classInputs = [...document.querySelectorAll<HTMLInputElement>('input[name="characterClass"]')];
const sexInputs = [...document.querySelectorAll<HTMLInputElement>('input[name="characterSex"]')];
const hatButtons = [...document.querySelectorAll<HTMLButtonElement>("[data-hat-style]")];
const hairButtons = [...document.querySelectorAll<HTMLButtonElement>("[data-hair-style]")];
const hairColorButtons = [...document.querySelectorAll<HTMLButtonElement>("[data-hair-color]")];
const worldUi = [...document.querySelectorAll<HTMLElement>("[data-world-ui]")];

// A transparent canvas: the CSS sky shows through behind the world.
const renderer = createThreeSceneRenderer(canvas, {
  alpha: true,
  antialias: true,
  pixelRatioLimit: 2,
});

fallbackCharacter.hidden = true;

type RotationQuaternion = [number, number, number, number];
type LocalPoint = (x: number, y: number, z: number) => [number, number, number];

let lastTime = performance.now();
let rosterStorageHealthy = true;
let characters: CharacterPreview[] = [PREVIEW_CHARACTER, ...loadCreatedRoster()];
let entryState: EntryState = initialEntryState(PREVIEW_CHARACTER);
let creationDraft: CharacterCreationDraft | null = null;
let characterAppearance: CharacterAppearance = { ...DEFAULT_CHARACTER_APPEARANCE };
// Unsaved appearance edits survive switching characters within this page session.
const appearanceByCharacterId = new Map<string, CharacterAppearance>();
const turntable = installCharacterTurntable(previewSurface, {
  left: requireElement<HTMLButtonElement>("#rotate-left"),
  right: requireElement<HTMLButtonElement>("#rotate-right"),
  reset: requireElement<HTMLButtonElement>("#reset-rotation"),
}, () => entryState.phase === "character-selection");

// The world: the shared Rust zone simulation, hosted locally through WASM, or a
// zone host reached over WebTransport when the page is configured for one
// (`?server=…&certHash=…`). Both share the WASM module's scenery, and
// presentation reads only decoded player-scoped projections from a source.
const online = onlineChoice(window.location.search, {
  server: import.meta.env.VITE_ZONE_SERVER,
  certificateHash: import.meta.env.VITE_ZONE_CERT_HASH,
});
let world: LocalWorld | null = null;
/** This browser's stored local characters; set once the local world loads. */
let localCharacters: LocalCharacters | null = null;
let onlineWorld: ZoneWorld | null = null;
/** The world the player is in; null on the selection screen. */
let activeWorld: ZoneWorld | null = null;
let worldLoadError: string | null = null;
/** Why the last entry failed or the world was left after an error; cleared by the next entry. */
let worldFailure: string | null = null;
/** The world a join is in flight for; selection stays put until it settles. */
let joining: ZoneWorld | null = null;
let jumps = 0;
let arenaWorld: ArenaWorld | null = null;
// A review scenario (#371, `?scenario=<name>&seed=<n>`) skips selection and enters a fresh,
// unsaved local world in that situation; F9 or Reset scenario restarts it from the same seed.
let scenario: ScenarioChoice | null = null;
let scenarioWorld: { world: ZoneWorld; source: ScenarioSource; area: number | null } | null = null;
let scenarioAutoEntered = false;
try {
  scenario = scenarioChoice(window.location.search);
} catch (error) {
  worldLoadError = errorMessage(error);
}
const scenarioReset = requireElement<HTMLButtonElement>("#scenario-reset");
// The Action Lab (#381, `?scenario=action-lab&class=…`): a scenario with a configured character,
// pause/step/scrub and the inspector dock.
const actionLabRoot = requireElement<HTMLElement>("#action-lab");
let actionLab: ActionLabPanel | null = null;
let arenaServer: ZoneServer | null = null;
let arenaError: string | null = null;
const arenaStatus = requireElement<HTMLElement>("#battle-royale-status");
const arenaResult = requireElement<HTMLElement>("#battle-royale-result");
const arenaProgress = requireElement<HTMLElement>("#battle-royale-progress");
const arenaNext = requireElement<HTMLButtonElement>("#battle-royale-next");
const arenaEntry = new BattleRoyaleEntry(
  requireElement<HTMLSelectElement>("#activity-choice"),
  requireElement<HTMLElement>("#battle-royale-setup"),
  refreshArenaWorld,
);

function refreshArenaWorld(): void {
  if (!world || joining || entryState.phase === "world") { return; }
  arenaWorld = null;
  arenaServer = null;
  arenaError = null;
  worldFailure = null;
  document.body.dataset.activity = arenaEntry.selected ? "battle-royale" : "world";
  try {
    if (arenaEntry.selected) {
      arenaServer = arenaEntry.server();
      if (arenaServer) { arenaWorld = createArenaWorld(world, arenaServer, () => arenaEntry.equipment()); }
    }
  } catch (error) { arenaError = errorMessage(error); }
  worldView.load(arenaWorld?.world ?? world);
  onlineToggle.hidden = arenaEntry.selected || online.kind !== "online";
  refreshSelectionPresentation();
  updateEntryButton();
}

arenaNext.addEventListener("click", () => {
  returnToCharacters();
  arenaEntry.nextMatch();
});

function showArenaMatch(): void {
  const match = arenaWorld?.source.match;
  const viewerId = arenaWorld?.source.latestProjection()?.viewerId;
  const showing = Boolean(match && viewerId !== undefined && activeWorld === arenaWorld?.world);
  arenaStatus.hidden = !showing;
  if (!showing || !match || viewerId === undefined) { return; }
  const status = matchStatus(match, viewerId);
  if (arenaResult.textContent !== status.result) { arenaResult.textContent = status.result; }
  if (arenaProgress.textContent !== status.detail) { arenaProgress.textContent = status.detail; }
  arenaNext.hidden = !status.canConfigure;
  if (match.zone.viewer.dead) {
    requireElement<HTMLElement>("#unit-status").textContent = "Eliminated from this match.";
  }
}


function loadCreatedRoster(): CharacterPreview[] {
  try {
    return loadCreatedCharacters(window.localStorage);
  } catch {
    rosterStorageHealthy = false;
    return [];
  }
}

/** The committed level of a played local character, else the roster's level. */
function displayLevel(character: CharacterPreview): number {
  if (arenaEntry.selected) { return character.level; }
  if (!localCharacters || !world) {
    return character.level;
  }
  try {
    return committedLevel(localCharacters, character.id) ?? character.level;
  } catch {
    return character.level;
  }
}

function selectedCharacterId(): string {
  return entryState.phase === "character-selection" ? entryState.selectedCharacterId : entryState.characterId;
}

function selectedCharacter(): CharacterPreview {
  const id = selectedCharacterId();
  const character = characters.find((candidate) => candidate.id === id);
  if (!character) {
    throw new Error(`Selected character ${id} is missing from the local roster.`);
  }
  return character;
}

function previewCharacter(): CharacterPreview {
  return creationDraft ? draftCharacterPreview(creationDraft) : arenaEntry.preview(selectedCharacter());
}

function defaultAppearanceForCharacter(character: CharacterPreview): CharacterAppearance {
  return character.id === PREVIEW_CHARACTER.id
    ? { ...DEFAULT_CHARACTER_APPEARANCE }
    : defaultAppearanceForClass(character.classId);
}

function rememberAppearance(): void {
  appearanceByCharacterId.set(selectedCharacterId(), { ...characterAppearance });
}

function restoreAppearance(character: CharacterPreview): void {
  characterAppearance = { ...(appearanceByCharacterId.get(character.id) ?? defaultAppearanceForCharacter(character)) };
}

const camera = new THREE.PerspectiveCamera(50, 1, 0.2, 1_400);
const previewCamera = new THREE.PerspectiveCamera(48, 1, 0.1, 80);
const worldView = new WorldView(renderer, camera, {
  canvas,
  sky: requireElement<HTMLElement>("#sky"),
  minimap: {
    root: requireElement<HTMLElement>("#minimap"),
    canvas: requireElement<HTMLCanvasElement>("#minimap-canvas"),
    zoomIn: requireElement<HTMLButtonElement>("#minimap-zoom-in"),
    zoomOut: requireElement<HTMLButtonElement>("#minimap-zoom-out"),
    northUp: requireElement<HTMLButtonElement>("#minimap-north-up"),
  },
  overlay: requireElement<HTMLElement>("#debug-overlay"),
  areaName: requireElement<HTMLElement>("#area-name"),
  unitStatus: requireElement<HTMLElement>("#unit-status"),
  combatFeedback: requireElement<HTMLElement>("#combat-feedback"),
  classHud: requireElement<HTMLElement>("#class-hud"),
  experienceBar: requireElement<HTMLProgressElement>("#experience-bar"),
  experienceStatus: requireElement<HTMLElement>("#experience-status"),
  progressionFeedback: requireElement<HTMLElement>("#progression-feedback"),
  loot: {
    panel: requireElement<HTMLElement>("#loot-panel"),
    toggle: requireElement<HTMLButtonElement>("#loot-toggle"),
    close: requireElement<HTMLButtonElement>("#loot-close"),
    title: requireElement<HTMLElement>("#loot-title"),
    rewards: requireElement<HTMLElement>("#loot-rewards"),
    claim: requireElement<HTMLButtonElement>("#loot-claim"),
    feedback: requireElement<HTMLElement>("#loot-feedback"),
    copper: requireElement<HTMLElement>("#copper-status"),
  },
  bags: {
    panel: requireElement<HTMLElement>("#bags-panel"),
    toggle: requireElement<HTMLButtonElement>("#bags-toggle"),
    close: requireElement<HTMLButtonElement>("#bags-close"),
    slots: requireElement<HTMLElement>("#bag-slots"),
    quantity: requireElement<HTMLInputElement>("#bag-quantity"),
    selection: requireElement<HTMLElement>("#bag-selection"),
    status: requireElement<HTMLElement>("#bag-status"),
    feedback: requireElement<HTMLElement>("#bag-feedback"),
    equip: requireElement<HTMLButtonElement>("#bag-equip"),
  },
  character: {
    panel: requireElement<HTMLElement>("#character-panel"),
    toggle: requireElement<HTMLButtonElement>("#character-toggle"),
    close: requireElement<HTMLButtonElement>("#character-close"),
    status: requireElement<HTMLElement>("#character-status"),
    slots: requireElement<HTMLElement>("#equipment-slots"),
    stats: requireElement<HTMLElement>("#character-stats"),
    feedback: requireElement<HTMLElement>("#character-feedback"),
  },
  vendor: {
    panel: requireElement<HTMLElement>("#vendor-panel"),
    toggle: requireElement<HTMLButtonElement>("#vendor-toggle"),
    close: requireElement<HTMLButtonElement>("#vendor-close"),
    title: requireElement<HTMLElement>("#vendor-title"),
    status: requireElement<HTMLElement>("#vendor-status"),
    offers: requireElement<HTMLElement>("#vendor-offers"),
    sales: requireElement<HTMLElement>("#vendor-sales"),
    feedback: requireElement<HTMLElement>("#vendor-feedback"),
  },
  quests: {
    dialog: requireElement<HTMLElement>("#quest-dialog"),
    dialogToggle: requireElement<HTMLButtonElement>("#quest-dialog-toggle"),
    dialogClose: requireElement<HTMLButtonElement>("#quest-dialog-close"),
    dialogTitle: requireElement<HTMLElement>("#quest-dialog-title"),
    dialogStatus: requireElement<HTMLElement>("#quest-dialog-status"),
    dialogQuests: requireElement<HTMLElement>("#quest-dialog-quests"),
    dialogFeedback: requireElement<HTMLElement>("#quest-dialog-feedback"),
    log: requireElement<HTMLElement>("#quest-log"),
    logToggle: requireElement<HTMLButtonElement>("#quest-log-toggle"),
    logClose: requireElement<HTMLButtonElement>("#quest-log-close"),
    logQuests: requireElement<HTMLElement>("#quest-log-quests"),
    logFeedback: requireElement<HTMLElement>("#quest-log-feedback"),
    tracker: requireElement<HTMLElement>("#objective"),
    notice: requireElement<HTMLElement>("#quest-notice"),
    markers: requireElement<HTMLElement>("#quest-markers"),
  },
  chat: {
    frame: requireElement<HTMLElement>("#chat"),
    log: requireElement<HTMLElement>("#chat-log"),
    form: requireElement<HTMLFormElement>("#chat-form"),
    input: requireElement<HTMLInputElement>("#chat-input"),
  },
});
/** Whether the Settings modal is open: it blocks game keys and pauses only the local zone (#41 Q8). */
let settingsOpen = false;
const controls = new GameControls({
  screen: () =>
    entryState.phase === "world"
      ? { phase: "world", panelOpen: worldView.panelOpen, casting: Boolean(activeWorld?.source.latestProjection()?.viewer.cast), settingsOpen }
      // A pending join is modal like creation: Escape cancels it.
      : { phase: "selection", creating: creationDraft !== null || joining !== null, settingsOpen },
  onAction: (action) => runAction(action),
  onHold: (action, phase) => runHold(action, phase),
});
// Touch (#434): the same semantic actions from a thumbstick, face-button chords and context buttons.
trackInputMode(document.documentElement, window);
const chatFrame = requireElement<HTMLElement>("#chat");
const touchControls = new TouchControls(requireElement<HTMLElement>("#touch-controls"), {
  onAction: (action) => {
    if (entryState.phase === "world" && !settingsOpen) {
      runAction(action);
    }
  },
  onInteract: () => worldView.touchInteract(),
  onSettings: () => requireElement<HTMLButtonElement>("#world-settings").click(),
  // On phones chat is collapsed to this button; it shows while its field has focus.
  onChat: () => {
    chatFrame.dataset.touchOpen = "true";
    worldView.openChat();
  },
  context: () => worldView.touchContext(),
  facesKey: () => worldView.hotbarKey,
  faces: () => worldView.hotbarFaces(),
});
chatFrame.addEventListener("focusout", () => {
  delete chatFrame.dataset.touchOpen;
});

function browserStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

// Client preferences live in the shared settings session; the client only applies them.
function applyPreferences(preferences: ClientPreferences): void {
  camera.fov = preferences.fieldOfViewDegrees;
  camera.updateProjectionMatrix();
  worldView.setLookSensitivity(preferences.lookSensitivity);
  worldView.setLookInversion(preferences.invertVerticalLook);
  worldView.setMotionPreference(preferences.reduceMotion);
  if (preferences.reduceMotion === "system") {
    delete document.documentElement.dataset.motion;
  } else {
    document.documentElement.dataset.motion = preferences.reduceMotion;
  }
}
const preferences = Preferences.open(browserStorage());
preferences.then((loaded) => loaded.subscribe(applyPreferences), (error: unknown) => console.error(error));

// Each character's bindings are its own input-bindings profile, swapped in when the selection changes.
let boundCharacterId: string | null = null;
let boundProfile = defaultBindingProfile("");
function syncBindingProfile(): void {
  const characterId = selectedCharacterId();
  if (characterId === boundCharacterId) {
    return;
  }
  boundCharacterId = characterId;
  const storage = browserStorage();
  const loaded = storage ? loadBindingProfile(storage, characterId) : { profile: defaultBindingProfile(characterId), problem: null };
  if (loaded.problem) {
    console.warn(`Key bindings for ${characterId}: ${loaded.problem}`);
  }
  bindProfile(loaded.profile);
}

/** Runs `profile` and shows its working directed-strike binding on the Heroic Strike slot. */
function bindProfile(profile: Profile): void {
  boundProfile = profile;
  controls.setProfile(profile);
  worldView.setDirectedBinding(actionBindingLabels("combat.directedStrike", profile));
}

const settingsPanel = new SettingsPanel({
  dialog: requireElement<HTMLDialogElement>("#settings-dialog"),
  close: requireElement<HTMLButtonElement>("#settings-close"),
  categories: requireElement<HTMLElement>("#settings-categories"),
  categorySelect: requireElement<HTMLSelectElement>("#settings-category-select"),
  search: requireElement<HTMLInputElement>("#settings-search"),
  resetCategory: requireElement<HTMLButtonElement>("#settings-reset-category"),
  content: requireElement<HTMLElement>("#settings-content"),
  status: requireElement<HTMLElement>("#settings-status"),
  resetAll: requireElement<HTMLButtonElement>("#settings-reset-all"),
  exportUser: requireElement<HTMLButtonElement>("#settings-export-user"),
  importUser: requireElement<HTMLInputElement>("#settings-import-user"),
}, {
  preferences,
  bindings: () => {
    syncBindingProfile();
    const characterId = selectedCharacterId();
    return {
      characterId,
      characterName: selectedCharacter().name,
      profile: boundProfile,
      save: (profile) => {
        const storage = browserStorage();
        if (!storage) {
          return "Browser storage is unavailable; the binding change was not saved.";
        }
        try {
          saveBindingProfile(storage, characterId, profile);
        } catch (error) {
          return `The binding change was not saved: ${errorMessage(error)}`;
        }
        // Only a durably saved profile becomes the running mapping.
        if (characterId === boundCharacterId) {
          bindProfile(profile);
        }
        return null;
      },
    };
  },
  onOpenChange: (open) => {
    settingsOpen = open;
    // Opening or closing Settings never leaves movement, a drag or an action held.
    controls.retire(open ? "settingsOpened" : "settingsClosed");
    worldView.cancelPointer();
    lastTime = performance.now();
  },
});
for (const button of [requireElement<HTMLButtonElement>("#world-settings"), requireElement<HTMLButtonElement>("#selection-settings")]) {
  button.addEventListener("click", (event) => {
    // A mouse-opened Settings returns focus to the world so game keys keep working afterwards.
    settingsPanel.show(entryState.phase === "world" && event.detail > 0 ? canvas : undefined);
  });
}

// Debug-only camera and public-source hooks for deterministic acceptance; `?debug` enables them.
if (new URLSearchParams(window.location.search).has("debug")) {
  Object.assign(window, { __valeDebug: { ...worldView.debugApi(), worldSource: () => (activeWorld ?? chosenWorld())?.source ?? null,
    loadingReport: () => ({ ...loading, marks: startupMarks(), startArea: startArea(), art: worldView.artProgress(startArea()), allArt: worldView.artProgress(null) }) } });
}

const selectionStageNodes: RendererSceneNode[] = [
  node("selection-floor", "cylinder", [-1.2, -0.12, 0], "#27332f", [3.0, 0.28, 0]),
  node("selection-floor-inner", "cylinder", [-1.2, 0.04, 0], "#495b50", [2.25, 0.12, 0]),
  node("selection-column-left", "box", [-5.4, 2.2, -2.0], "#313b39", [1.1, 4.4, 1.1]),
  node("selection-column-right", "box", [3.0, 2.2, -2.0], "#313b39", [1.1, 4.4, 1.1]),
  node("selection-plinth-left", "box", [-5.4, 0.35, -2.0], "#516057", [1.7, 0.7, 1.7]),
  node("selection-plinth-right", "box", [3.0, 0.35, -2.0], "#516057", [1.7, 0.7, 1.7]),
  node("selection-lantern-left", "sphere", [-4.8, 3.8, -1.2], "#c4a35a", [0.16, 0, 0]),
  node("selection-lantern-right", "sphere", [2.4, 3.8, -1.2], "#c4a35a", [0.16, 0, 0]),
  node("selection-backdrop", "box", [-1.2, 2.25, -4.0], "#1d292a", [9.6, 4.8, 0.3]),
];

function node(
  id: string,
  kind: "box" | "sphere" | "cylinder",
  translation: [number, number, number],
  color: `#${string}`,
  dimensions: [number, number, number],
): RendererSceneNode {
  const geometry =
    kind === "box"
      ? { kind, size: dimensions }
      : kind === "sphere"
        ? { kind, radius: dimensions[0] }
        : { kind, radius: dimensions[0], height: dimensions[1] };

  return { id, geometry, color, transform: { translation } };
}

function selectionCharacterNodes(): RendererSceneNode[] {
  const character = previewCharacter();
  const visuals = characterVisualProfile(character);
  const baseX = -1.2;
  const yaw = turntable.yaw;
  const halfYaw = yaw / 2;
  const rotationQuaternion: RotationQuaternion = [0, Math.sin(halfYaw), 0, Math.cos(halfYaw)];
  const localPoint: LocalPoint = (x, y, z) => {
    const cos = Math.cos(yaw);
    const sin = Math.sin(yaw);
    return [baseX + x * cos + z * sin, y, -x * sin + z * cos];
  };
  const bodyY = 1.28 - (1.7 - visuals.bodyHeight) / 2;

  return [
    {
      id: "preview-body",
      geometry: { kind: "cylinder", radius: visuals.bodyRadius, height: visuals.bodyHeight },
      color: visuals.bodyColor,
      transform: { translation: localPoint(0, bodyY, 0), rotationQuaternion },
    },
    {
      id: "preview-chest",
      geometry: { kind: "box", size: [visuals.chestSize[0], visuals.chestSize[1], visuals.chestSize[2]] },
      color: visuals.chestColor,
      transform: { translation: localPoint(0, 1.58, 0), rotationQuaternion },
    },
    {
      id: "preview-head",
      geometry: { kind: "sphere", radius: visuals.headRadius },
      color: "#c49b78",
      transform: { translation: localPoint(0, 2.48, 0), rotationQuaternion },
    },
    ...previewHatNodes(characterAppearance.hat, localPoint, rotationQuaternion),
    {
      id: "preview-face",
      geometry: { kind: "sphere", radius: visuals.headRadius * 0.8 },
      color: "#c49b78",
      transform: { translation: localPoint(0, 2.46, -0.18), rotationQuaternion },
    },
    {
      id: "preview-shoulder-left",
      geometry: { kind: "box", size: [0.42, 0.28, 0.62] },
      color: visuals.shoulderColor,
      transform: { translation: localPoint(-visuals.shoulderSpan, 1.96, 0), rotationQuaternion },
    },
    {
      id: "preview-shoulder-right",
      geometry: { kind: "box", size: [0.42, 0.28, 0.62] },
      color: visuals.shoulderColor,
      transform: { translation: localPoint(visuals.shoulderSpan, 1.96, 0), rotationQuaternion },
    },
    {
      id: "preview-arm-left",
      geometry: { kind: "cylinder", radius: visuals.armRadius, height: 1.02 },
      color: visuals.bodyColor,
      transform: { translation: localPoint(-visuals.shoulderSpan, 1.37, 0), rotationQuaternion },
    },
    {
      id: "preview-arm-right",
      geometry: { kind: "cylinder", radius: visuals.armRadius, height: 1.02 },
      color: visuals.bodyColor,
      transform: { translation: localPoint(visuals.shoulderSpan, 1.37, 0), rotationQuaternion },
    },
    {
      id: "preview-boot-left",
      geometry: { kind: "box", size: [0.34, 0.62, 0.48] },
      color: "#745d49",
      transform: { translation: localPoint(-0.24, 0.38, -0.06), rotationQuaternion },
    },
    {
      id: "preview-boot-right",
      geometry: { kind: "box", size: [0.34, 0.62, 0.48] },
      color: "#745d49",
      transform: { translation: localPoint(0.24, 0.38, -0.06), rotationQuaternion },
    },
    {
      id: "preview-cloak",
      geometry: { kind: "box", size: [0.82, 1.45, 0.08] },
      color: visuals.cloakColor,
      transform: { translation: localPoint(0, 1.37, 0.34), rotationQuaternion },
    },
    ...previewWeaponNodes(visuals, localPoint, rotationQuaternion),
  ];
}

function previewWeaponNodes(
  visuals: CharacterVisualProfile,
  localPoint: LocalPoint,
  rotationQuaternion: RotationQuaternion,
): RendererSceneNode[] {
  if (visuals.weapon === "bow") {
    return [
      {
        id: "preview-bow-upper",
        geometry: { kind: "box", size: [0.09, 0.9, 0.08] },
        color: visuals.weaponColor,
        transform: { translation: localPoint(0.82, 1.62, 0), rotationQuaternion },
      },
      {
        id: "preview-bow-lower",
        geometry: { kind: "box", size: [0.09, 0.9, 0.08] },
        color: visuals.weaponColor,
        transform: { translation: localPoint(0.82, 0.78, 0), rotationQuaternion },
      },
      {
        id: "preview-bow-string",
        geometry: { kind: "box", size: [0.025, 1.65, 0.025] },
        color: "#d8d5c7",
        transform: { translation: localPoint(0.7, 1.2, 0), rotationQuaternion },
      },
    ];
  }
  if (visuals.weapon === "staff") {
    return [
      {
        id: "preview-staff",
        geometry: { kind: "box", size: [0.1, 1.9, 0.1] },
        color: visuals.weaponColor,
        transform: { translation: localPoint(0.82, 1.2, 0), rotationQuaternion },
      },
      {
        id: "preview-staff-focus",
        geometry: { kind: "sphere", radius: 0.22 },
        color: "#d6a677",
        transform: { translation: localPoint(0.82, 2.18, 0), rotationQuaternion },
      },
    ];
  }
  // A dagger (#302) is the sword with a short blade under the same hilt.
  const [bladeLength, bladeCentre] = visuals.weapon === "dagger" ? [0.7, 1.53] : [1.78, 1.1];
  return [
    {
      id: "preview-sword-blade",
      geometry: { kind: "box", size: [0.13, bladeLength, 0.09] },
      color: visuals.weaponColor,
      transform: { translation: localPoint(0.82, bladeCentre, -0.02), rotationQuaternion },
    },
    {
      id: "preview-sword-hilt",
      geometry: { kind: "box", size: [0.58, 0.1, 0.12] },
      color: "#c7b66d",
      transform: { translation: localPoint(0.82, 1.93, -0.02), rotationQuaternion },
    },
    {
      id: "preview-sword-grip",
      geometry: { kind: "cylinder", radius: 0.08, height: 0.44 },
      color: "#715744",
      transform: { translation: localPoint(0.82, 2.14, -0.02), rotationQuaternion },
    },
  ];
}

function previewHatNodes(
  hat: HatStyle,
  localPoint: LocalPoint,
  rotationQuaternion: RotationQuaternion,
): RendererSceneNode[] {
  if (hat === "none") {
    return [];
  }
  if (hat === "wayfarer-hood") {
    return [{
      id: "preview-hat-hood",
      geometry: { kind: "sphere", radius: 0.4 },
      color: "#b7c6bd",
      transform: { translation: localPoint(0, 2.56, 0.06), rotationQuaternion },
    }];
  }
  if (hat === "ranger-cap") {
    return [
      {
        id: "preview-hat-cap-brim",
        geometry: { kind: "cylinder", radius: 0.5, height: 0.08 },
        color: "#6f875f",
        transform: { translation: localPoint(0, 2.72, 0), rotationQuaternion },
      },
      {
        id: "preview-hat-cap-crown",
        geometry: { kind: "cylinder", radius: 0.31, height: 0.25 },
        color: "#5d7351",
        transform: { translation: localPoint(0, 2.84, 0.04), rotationQuaternion },
      },
    ];
  }
  return [
    {
      id: "preview-hat-helm",
      geometry: { kind: "sphere", radius: 0.39 },
      color: "#858e91",
      transform: { translation: localPoint(0, 2.56, 0.04), rotationQuaternion },
    },
    {
      id: "preview-hat-brow",
      geometry: { kind: "box", size: [0.62, 0.1, 0.12] },
      color: "#aab0b2",
      transform: { translation: localPoint(0, 2.58, -0.33), rotationQuaternion },
    },
    {
      id: "preview-hat-crest",
      geometry: { kind: "box", size: [0.12, 0.44, 0.42] },
      color: "#aab0b2",
      transform: { translation: localPoint(0, 2.94, 0.04), rotationQuaternion },
    },
  ];
}

function renderCharacterDetails(character: CharacterPreview, appearance: CharacterAppearance) {
  characterName.textContent = character.name;
  const sex = character.sex === "male" ? "Male" : "Female";
  characterSubtitle.textContent = `Level ${displayLevel(character)} · ${sex} ${character.race} ${character.className}`;
  const continuation = continuationLabel();
  characterLocation.textContent = continuation.area;
  continuationCopy.textContent = continuation.copy;
  appearanceSummary.textContent = [
    hatOption(appearance.hat).name,
    appearance.hair === "bald" ? "Bald" : `${hairName(appearance.hair)}, ${hairColorName(appearance.hairColor)}`,
  ].join(" · ");
  const equipment = equipmentForAppearance(character, appearance);
  equipmentSummary.textContent = equipment.find((item) => item.slot === "Main hand")?.name ?? "";
  equipmentList.replaceChildren(...equipment.map((item) => {
    const article = document.createElement("article");
    article.className = "equipment-item";
    const swatch = document.createElement("span");
    swatch.className = "equipment-swatch";
    swatch.style.setProperty("--equipment-accent", item.accent);
    const copy = document.createElement("span");
    const slot = document.createElement("small");
    slot.textContent = item.slot;
    const name = document.createElement("strong");
    name.textContent = item.name;
    copy.append(slot, name);
    article.append(swatch, copy);
    return article;
  }));
}

const hairName = (style: CharacterAppearance["hair"]) => HAIR_OPTIONS.find((option) => option.id === style)?.name ?? style;
const hairColorName = (color: CharacterAppearance["hairColor"]) => HAIR_COLOR_OPTIONS.find((option) => option.id === color)?.name ?? color;

/** Marks the chosen hat, hairstyle and hair colour in the Appearance pickers. */
function syncAppearanceButtons(): void {
  const mark = (buttons: HTMLButtonElement[], selectedOf: (button: HTMLButtonElement) => boolean) => {
    for (const button of buttons) {
      const selected = selectedOf(button);
      button.classList.toggle("is-selected", selected);
      button.setAttribute("aria-pressed", String(selected));
    }
  };
  mark(hatButtons, (button) => button.dataset.hatStyle === characterAppearance.hat);
  mark(hairButtons, (button) => button.dataset.hairStyle === characterAppearance.hair);
  mark(hairColorButtons, (button) => button.dataset.hairColor === characterAppearance.hairColor);
}

function applyAppearance(nextAppearance: CharacterAppearance, message: string) {
  characterAppearance = { ...nextAppearance };
  characterSelect.dataset.hat = characterAppearance.hat;
  renderCharacterDetails(previewCharacter(), characterAppearance);
  syncAppearanceButtons();
  saveStatus.textContent = message;
}

function saveCurrentCharacter() {
  try {
    const saved = saveCharacter(localStorage, selectedCharacter().id, characterAppearance);
    saveStatus.textContent = `Saved locally · ${hatOption(saved.appearance.hat).name}`;
  } catch {
    saveStatus.textContent = "Browser storage is unavailable; character was not saved.";
  }
}

function loadSavedCharacter() {
  try {
    const saved = loadCharacter(localStorage, selectedCharacter().id);
    if (!saved) {
      saveStatus.textContent = "No local save exists for this character yet.";
      return;
    }
    applyAppearance(saved.appearance, `Loaded local save · ${hatOption(saved.appearance.hat).name}`);
  } catch {
    saveStatus.textContent = "Local save is invalid and was not loaded.";
  }
}

function renderRoster(): void {
  const selectedId = selectedCharacterId();
  const buttons = characters.map((character) => {
    const button = document.createElement("button");
    button.className = "roster-character";
    button.type = "button";
    button.dataset.characterId = character.id;
    const selected = character.id === selectedId;
    button.classList.toggle("is-selected", selected);
    button.setAttribute("aria-pressed", String(selected));

    const portrait = document.createElement("span");
    portrait.className = "roster-portrait";
    portrait.textContent = character.name.split(" ").map((part) => part[0] ?? "").join("").slice(0, 2).toUpperCase();
    const copy = document.createElement("span");
    copy.className = "roster-copy";
    const name = document.createElement("strong");
    name.textContent = character.name;
    const detail = document.createElement("small");
    detail.textContent = `${displayLevel(character)} ${character.className} · ${character.sex === "male" ? "Male" : "Female"} · Greyhaven`;
    copy.append(name, detail);
    const check = document.createElement("span");
    check.className = "roster-check";
    check.setAttribute("aria-hidden", "true");
    check.textContent = selected ? "◆" : "";
    button.append(portrait, copy, check);
    button.addEventListener("click", () => selectCharacter(character.id));
    return button;
  });
  rosterContainer.replaceChildren(...buttons);
  createCharacterButton.disabled = characters.length >= MAX_CHARACTER_SLOTS;
  createCharacterButton.title = createCharacterButton.disabled ? "All character slots are full." : "";
}

/** The zone host "Enter World" joins, when online play is chosen. */
function chosenServer(): ZoneServer | null {
  if (arenaEntry.selected) { return arenaServer; }
  return online.kind === "online" && playOnlineInput.checked ? online.server : null;
}

/** The world "Enter World" joins; null while loading or when online play is misconfigured. */
function chosenWorld(): ZoneWorld | null {
  if (scenario) { return scenarioWorld?.world ?? null; }
  if (arenaEntry.selected) { return arenaWorld?.world ?? null; }
  if (online.kind === "invalid") {
    return null;
  }
  return chosenServer() ? onlineWorld : world;
}

function entryNote(character: CharacterPreview): string {
  if (arenaEntry.selected) {
    if (!world) { return worldLoadError ?? "Loading characters…"; }
    if (arenaError) { return arenaError; }
    if (!arenaWorld) { return "Enter a match server to play with friends."; }
    if (joining) { return "Joining Battle Royale…"; }
    return worldFailure ?? `Join Battle Royale at ${arenaServer?.label}`;
  }
  if (scenario && !scenarioWorld) {
    return worldLoadError ?? `Loading review scenario ${scenario.name}…`;
  }
  const server = chosenServer();
  if (online.kind === "invalid") {
    return `Online play is misconfigured: ${online.message}`;
  }
  if (!world) {
    return worldLoadError ? `The zone simulation could not load: ${worldLoadError}` : "Loading the zone simulation…";
  }
  const { loaded, total } = worldView.artProgress(startArea());
  if (!entryArtReady()) {
    return `Loading Greyhaven… ${loaded} of ${total} pieces`;
  }
  if (joining) {
    return server ? `Connecting to ${server.label}…` : `Joining with ${character.name}…`;
  }
  if (worldFailure) { return worldFailure; }
  const start = server ? `Join ${server.label} with ${character.name}` : `Start ${character.name} in Greyhaven Outpost`;
  return artReady() ? start : `${start} · art loading ${loaded} of ${total}`;
}

function updateEntryButton(): void {
  if (scenarioWorld && !scenarioAutoEntered && entryState.phase !== "world" && !joining && artReady()) {
    scenarioAutoEntered = true;
    void enterWorld();
  }
  const server = chosenServer();
  if (artReady()) { markStartup("art-settled"); }
  enterWorldButton.disabled = chosenWorld() === null || joining !== null || !entryArtReady();
  if (!enterWorldButton.disabled) { markStartup("enter-enabled"); }
  let entryLabel = "Enter World";
  if (arenaEntry.selected) { entryLabel = "Join Battle Royale"; }
  if (joining) { entryLabel = "Entering…"; }
  enterWorldLabel.textContent = entryLabel;
  enterWorldNote.textContent = entryNote(selectedCharacter());
  playOnlineInput.disabled = joining !== null;
  arenaEntry.setLocked(joining !== null || entryState.phase === "world");
  if (arenaEntry.selected) {
    selectionNote.textContent = "Hosted PvP. Choose a build before joining; change it after elimination or between matches.";
    return;
  }
  selectionNote.textContent = server
    ? `Online: the zone host at ${server.label} runs this world for every page that joins it. Hosts with character storage save your character progress.`
    : "Single-player demo: the shared Rust zone simulation runs in this browser. Character progress and the world are saved in this browser.";
}

/** A zone host world over WebTransport, drawn with the scenery the page loaded. */
function createOnlineWorld(local: LocalWorld, server: ZoneServer): ZoneWorld {
  return {
    ...local,
    source: new OnlineZoneSource({
      route: server.route,
      // Projections must come from the content revision this scenery shows.
      contentRevision: local.scenery.scenery.contentRevision,
      connector: webTransportConnector(server.certificateHash),
      resolveCharacterId: (id) => navigator.locks.request("mmorpg-online-character-ids", () => onlineCharacterId(window.localStorage, id)),
    }),
  };
}

function showLinkState(state: LinkState): void {
  const text = state === "reconnecting" ? "Reconnecting…" : "";
  if (connectionStatus.textContent !== text) {
    connectionStatus.textContent = text;
  }
}

function refreshSelectionPresentation(): void {
  const character = previewCharacter();
  characterSelect.dataset.hat = characterAppearance.hat;
  characterSelect.dataset.sex = character.sex;
  characterSelect.dataset.characterClass = character.classId;
  renderCharacterDetails(character, characterAppearance);
  syncAppearanceButtons();
}

function selectCharacter(characterId: string): void {
  if (creationDraft || joining) {
    return;
  }
  const character = characters.find((candidate) => candidate.id === characterId);
  if (!character || character.id === selectedCharacterId()) {
    return;
  }
  rememberAppearance();
  entryState = initialEntryState(character);
  restoreAppearance(character);
  refreshSelectionPresentation();
  renderRoster();
  updateEntryButton();
  layoutPreview();
}

function setCreationMode(active: boolean): void {
  creationPanel.hidden = !active;
  rosterPanel.hidden = active;
  selectionActions.hidden = active;
}

function syncCreationDraft(): void {
  if (!creationDraft) {
    return;
  }
  const classId = classInputs.find((input) => input.checked)?.value;
  const sex = sexInputs.find((input) => input.checked)?.value;
  if (!isCharacterClassId(classId) || !isCharacterSex(sex)) {
    return;
  }
  const classChanged = creationDraft.classId !== classId;
  creationDraft = { name: creationName.value, classId, sex };
  if (classChanged) {
    characterAppearance = defaultAppearanceForClass(classId);
  }
  refreshSelectionPresentation();
}

function openCharacterCreation(): void {
  if (joining) {
    return;
  }
  if (characters.length >= MAX_CHARACTER_SLOTS) {
    rosterStatus.textContent = `All ${MAX_CHARACTER_SLOTS} character slots are full.`;
    return;
  }
  rememberAppearance();
  creationDraft = { name: "", classId: "warden", sex: "male" };
  characterAppearance = defaultAppearanceForClass("warden");
  creationForm.reset();
  creationName.value = "";
  creationStatus.textContent = "";
  setCreationMode(true);
  refreshSelectionPresentation();
  turntable.cancel();
  layoutPreview();
  creationName.focus();
}

function cancelCharacterCreation(): void {
  if (!creationDraft) {
    return;
  }
  creationDraft = null;
  setCreationMode(false);
  restoreAppearance(selectedCharacter());
  refreshSelectionPresentation();
  renderRoster();
  updateEntryButton();
  layoutPreview();
  createCharacterButton.focus();
}

function persistCreatedRoster(): boolean {
  if (!rosterStorageHealthy) {
    return false;
  }
  try {
    saveCreatedCharacters(window.localStorage, characters.filter((character) => character.id.startsWith("local-")));
    return true;
  } catch {
    rosterStorageHealthy = false;
    return false;
  }
}

// Storage events are ignored while a draft is open, so another tab may have
// appended characters since this tab last read the roster. Merge the stored
// roster before allocating an ID so this write cannot drop or reuse theirs.
function reconcileStoredRoster(): void {
  if (!rosterStorageHealthy) {
    return;
  }
  let stored: CharacterPreview[];
  try {
    stored = loadCreatedCharacters(window.localStorage);
  } catch {
    rosterStorageHealthy = false;
    return;
  }
  characters = mergeStoredRoster(characters, stored);
}

function finishCharacterCreation(): void {
  if (!creationDraft) {
    return;
  }
  syncCreationDraft();
  if (!creationDraft) {
    return;
  }
  try {
    reconcileStoredRoster();
    const character = createCharacterPreview(
      creationDraft,
      characters,
      (id) => hasRetainedCharacterSaves(window.localStorage, id) || hasOnlineCharacterId(window.localStorage, id),
    );
    characters = [...characters, character];
    const persisted = persistCreatedRoster();
    creationDraft = null;
    setCreationMode(false);
    entryState = initialEntryState(character);
    restoreAppearance(character);
    refreshSelectionPresentation();
    renderRoster();
    updateEntryButton();
    rosterStatus.textContent = persisted
      ? `${character.name} created locally.`
      : `${character.name} created for this session only; browser roster storage is unavailable.`;
    layoutPreview();
    rosterContainer.querySelector<HTMLButtonElement>(`[data-character-id="${character.id}"]`)?.focus();
  } catch (error) {
    creationStatus.textContent = error instanceof Error ? error.message : "Character could not be created.";
    creationName.focus();
  }
}

/** The framed height around the figure standing at y 0.1, and its centre. */
const PREVIEW_SPAN = 3.05;
const PREVIEW_CENTRE_Y = 1.5;

function layoutPreview() {
  if (entryState.phase !== "character-selection") return;
  const width = Math.max(canvas.clientWidth, 1);
  const height = Math.max(canvas.clientHeight, 1);
  const rect = previewSurface.getBoundingClientRect();
  // The figure (PREVIEW_HEIGHT plus headroom for hats) fills most of the preview's height (#362 owner
  // review); narrow layouts are bounded by the width instead.
  const displayHeight = Math.max(1, Math.min(rect.height * 0.9, rect.width * 1.9));
  const distance = PREVIEW_SPAN * height / (2 * Math.tan(THREE.MathUtils.degToRad(24)) * displayHeight);
  previewCamera.position.set(4.5, 1.25, 7.1).normalize().multiplyScalar(distance).add(new THREE.Vector3(-1.2, PREVIEW_CENTRE_Y, 0));
  previewCamera.lookAt(-1.2, PREVIEW_CENTRE_Y, 0);
  previewCamera.setViewOffset(width, height, width / 2 - (rect.left + rect.width / 2), height / 2 - (rect.top + rect.height / 2), width, height);
  previewCamera.updateMatrixWorld(true);
}

function resize() {
  const width = Math.max(canvas.clientWidth, 1);
  const height = Math.max(canvas.clientHeight, 1);
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
  renderer.setSize(width, height, window.devicePixelRatio);
  layoutPreview();
}

/** The turntable's real humanoid (#362) faces the preview camera at zero turn. */
const PREVIEW_FACING = Math.atan2(4.5, 7.1) + Math.PI;
const PREVIEW_HEIGHT = 2.7;
const previewStart = performance.now();

function renderSelection() {
  const camera = {
    viewMatrix: [...previewCamera.matrixWorldInverse.elements] as Matrix4Values,
    projectionMatrix: webGpuProjection(previewCamera),
  };
  // The real Ranger once it loaded; the procedural figure while it loads or if it failed.
  const real = worldView.selectionPreview(viewerLook(), [-1.2, 0.1, 0], PREVIEW_FACING + turntable.yaw, PREVIEW_HEIGHT,
    (performance.now() - previewStart) / 1000, camera);
  renderer.render({ camera, nodes: real ? selectionStageNodes : [...selectionStageNodes, ...selectionCharacterNodes()] });
}

/** The viewer's own look for the shared unit presenter: class, sex and headwear. */
function viewerLook(): ViewerLook {
  const character = previewCharacter();
  return {
    classId: character.classId, sex: character.sex, headwear: characterAppearance.hat === "none" ? null : characterAppearance.hat,
    hair: characterAppearance.hair, hairColor: characterAppearance.hairColor,
  };
}

function worldInput() {
  const pads = typeof navigator.getGamepads === "function" ? navigator.getGamepads() : [];
  return { held: mergeHeld(controls.heldIntent(), touchControls.heldIntent()), jumps, padGuard: padGuardLane(pads) };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function frame(now: number) {
  // Re-arm first: one failed frame must never stop the render loop.
  requestAnimationFrame(frame);
  const deltaSeconds = frameDeltaSeconds(now, lastTime);
  lastTime = now;
  syncBindingProfile();
  if (entryState.phase === "world" && activeWorld) {
    // Settings pauses the browser-local zone; an online zone keeps running with input blocked.
    const paused = settingsOpen && activeWorld !== onlineWorld && activeWorld !== arenaWorld?.world;
    try {
      worldView.frame(activeWorld, worldInput(), viewerLook(), paused ? 0 : deltaSeconds, now);
      touchControls.update();
      showLinkState(activeWorld.source.linkState());
      showArenaMatch();
      showScenarioStatus();
      updateActionLab();
      if (loading.entry !== null && loading.playableMs === null) {
        loading.playableMs = Math.round(now - loading.entry);
        markStartup("first-frame");
        console.info(`Greyhaven playable ${loading.playableMs} ms after Enter World; ${loading.bytesBeforeEntry} bytes fetched before entry.`);
      }
    } catch (error) {
      // Fail closed and visibly: leave the world and say why on the selection screen.
      console.error(error);
      returnToCharacters(`Left the world after an error: ${errorMessage(error)}`);
    }
    return;
  }
  renderSelection();
}

/**
 * Progressive loading (#240): what the page fetched before Enter World and how long entry took
 * until the first world frame. Greyhaven's code, scenery and art load during character selection.
 */
const loading: { entry: number | null; bytesBeforeEntry: number; playableMs: number | null } = {
  entry: null, bytesBeforeEntry: 0, playableMs: null,
};

/** Bytes the page has fetched so far, from resource timing (decoded size where transfer is hidden). */
function fetchedBytes(): number {
  return performance.getEntriesByType("resource").reduce((sum, entry) => {
    const resource = entry as PerformanceResourceTiming;
    return sum + (resource.transferSize || resource.encodedBodySize || 0);
  }, 0);
}

/** Greyhaven Outpost, where new characters start. */
const OUTPOST_AREA = 1;

/**
 * The read-only continuation label (#362): the selected local character's committed area, the
 * outpost for a first entry, or neutral copy where the page cannot know it (online, before load).
 * It never chooses where Enter World spawns.
 */
function continuationLabel(): { area: string; copy: string } {
  const neutral = { area: "Your last location", copy: "Continue where you left off" };
  if (arenaEntry.selected) { return { area: "Battle Royale", copy: "Join a hosted match" }; }
  if (scenario) { return { area: `Review scenario ${scenario.name}`, copy: "A fresh, unsaved world" }; }
  if (chosenServer() || !world || !localCharacters) { return neutral; }
  let committed: number | null = null;
  try {
    committed = creationDraft ? null : committedArea(localCharacters, selectedCharacter().id);
  } catch {
    return neutral;
  }
  const area = world.catalog.areas.get(committed ?? OUTPOST_AREA);
  if (!area) { return neutral; }
  return committed === null ? { area, copy: "Begin your journey" } : { area, copy: "Continue where you left off" };
}

/** The area the selected character enters in: its committed location's, else the outpost. */
function startArea(): number {
  if (scenarioWorld) {
    return scenarioWorld.area ?? OUTPOST_AREA;
  }
  if (!localCharacters || creationDraft) {
    return OUTPOST_AREA;
  }
  try {
    return committedArea(localCharacters, selectedCharacter().id) ?? OUTPOST_AREA;
  } catch {
    return OUTPOST_AREA;
  }
}

/**
 * Whether all of the starting area's art has settled (loaded or failed to its fallback); fetching
 * it starts here, so it streams while the player is still choosing (#240).
 */
function artReady(): boolean {
  const area = startArea();
  worldView.prefetchRegion(area);
  const { loaded, total } = worldView.artProgress(area);
  return total > 0 && loaded >= total;
}

/**
 * Whether the starting area's entry-blocking art has settled: only the inn, where it stands.
 * The Ranger and creatures enter as procedural stand-ins and swap in once loaded (#395).
 */
function entryArtReady(): boolean {
  const area = startArea();
  worldView.prefetchRegion(area);
  return worldView.entryArtReady(area);
}

/** Joins the chosen world, then switches the page to it. Never rejects: failures are shown on the selection screen. */
async function enterWorld(): Promise<void> {
  const target = chosenWorld();
  if (creationDraft || !target || entryState.phase === "world" || joining || !entryArtReady()) {
    return;
  }
  markStartup("entry-click");
  loading.entry = performance.now();
  loading.bytesBeforeEntry = fetchedBytes();
  loading.playableMs = null;
  const server = chosenServer();
  const next = enterPreviewWorld(entryState, selectedCharacter());
  // Join before switching the page: a refused join leaves the source unjoined, so
  // the page stays on selection, says why, and entry can be retried.
  joining = target;
  worldFailure = null;
  updateEntryButton();
  try {
    const character = selectedCharacter();
    const entryCharacter = arenaEntry.selected ? arenaEntry.character(character) : character;
    await target.source.join({ id: entryCharacter.id, classId: entryCharacter.classId, sex: entryCharacter.sex });
    markStartup("joined");
  } catch (error) {
    console.error(error);
    worldFailure = `Could not enter the world: ${errorMessage(error)}`;
    return;
  } finally {
    joining = null;
    updateEntryButton();
  }
  activeWorld = target;
  const worldNotice = world && target === world ? world.source.worldNotice : null;
  if (scenario) {
    worldMode.textContent = `Review scenario ${scenario.name} · seed ${scenario.seed} · content rev ${world?.scenery.scenery.contentRevision} · nothing is saved`;
  } else if (arenaEntry.selected) {
    worldMode.textContent = `Battle Royale · ${server?.label}`;
  } else if (server) {
    worldMode.textContent = `Greyhaven Vale · online · ${server.label}`;
  } else {
    worldMode.textContent = `Greyhaven Vale · local zone host${worldNotice ? ` · ${worldNotice}` : ""}`;
  }
  showLinkState("connected");
  turntable.cancel();
  entryState = next;
  arenaEntry.setLocked(true);
  controls.retire("enteredWorld");
  worldView.enter(worldInput(), target.source.latestProjection());
  characterSelect.hidden = true;
  for (const element of worldUi) {
    element.hidden = false;
  }
  scenarioReset.hidden = !scenario;
  actionLabRoot.hidden = actionLab === null;
  document.body.classList.toggle("action-lab-open", actionLab !== null);
  lastTime = performance.now();
  canvas.focus();
}

/** Restarts the review scenario from its seed: a fresh zone, character and encounter. */
async function resetScenario(): Promise<void> {
  if (!scenarioWorld || activeWorld !== scenarioWorld.world || entryState.phase !== "world") {
    return;
  }
  worldView.leave();
  await scenarioWorld.source.reset();
  worldView.enter(worldInput(), scenarioWorld.source.latestProjection());
  lastTime = performance.now();
  canvas.focus();
}

/** Syncs the Action Lab dock to the tick the arena shows. */
function updateActionLab(): void {
  if (!actionLab || !scenarioWorld || activeWorld !== scenarioWorld.world) { return; }
  const source = scenarioWorld.source;
  const shown = source.shownFrame();
  const viewer = (shown?.snapshot ?? source.latestProjection())?.viewer;
  if (!viewer) { return; }
  actionLab.update(source.frames, shown, source.paused, {
    level: viewer.level, health: viewer.health, maxHealth: viewer.maxHealth,
    resource: viewer.resource ? `${viewer.resource.value} / ${viewer.resource.max} ${viewer.resource.kind}` : null,
    clip: worldView.viewerClip,
  });
}

/** Builds the lab dock once its world loaded. */
function mountActionLab(loaded: LocalWorld, choice: ScenarioChoice, source: ScenarioSource): void {
  const setup = choice.lab;
  if (!setup) { return; }
  const { graph, options } = actionLabDefinitions();
  actionLab = new ActionLabPanel(actionLabRoot, graph, options, loaded.catalog, setup, {
    togglePause: () => source.setPaused(!source.paused),
    step: () => source.step(),
    scrub: (tick) => source.scrubTo(tick === null ? null : BigInt(tick)),
    apply: (next) => { window.location.search = labQuery(next, choice.seed); },
  });
  new ResizeObserver(() => {
    document.body.style.setProperty("--lab-width", `${actionLabRoot.offsetWidth}px`);
  }).observe(actionLabRoot);
}

/** A dead scenario character is offered the reset instead of the long run back. */
function showScenarioStatus(): void {
  if (activeWorld && activeWorld === scenarioWorld?.world && activeWorld.source.latestProjection()?.viewer.dead) {
    requireElement<HTMLElement>("#unit-status").textContent = "You died. Press F9 or Reset scenario to fight again.";
  }
}

/** Back to selection; `failure` says why when the world was left after an error. */
function returnToCharacters(failure: string | null = null) {
  if (scenario && !failure) {
    // Leaving a review scenario is leaving review: the ordinary page has no scenario.
    window.location.search = "";
    return;
  }
  turntable.cancel();
  const character = selectedCharacter();
  try {
    // Leaving removes the unit (a zone host after its reconnect grace); the next entry spawns a new one.
    activeWorld?.source.leave();
  } catch (error) {
    console.error(error);
    failure ??= `Could not leave the world cleanly: ${errorMessage(error)}`;
  }
  activeWorld = null;
  arenaStatus.hidden = true;
  arenaEntry.setLocked(false);
  showLinkState("connected");
  worldFailure = failure;
  entryState = initialEntryState(character);
  controls.retire("leftWorld");
  touchControls.reset();
  worldView.leave();
  characterSelect.hidden = false;
  for (const element of worldUi) {
    element.hidden = true;
  }
  setCreationMode(false);
  creationDraft = null;
  refreshSelectionPresentation();
  renderRoster();
  updateEntryButton();
  lastTime = performance.now();
  characterSelect.scrollTop = 0;
  layoutPreview();
  previewSurface.focus({ preventScroll: true });
}

refreshSelectionPresentation();
renderRoster();
updateEntryButton();
if (!rosterStorageHealthy) {
  rosterStatus.textContent = "Saved local roster could not be read; new characters will remain in this session only.";
}

for (const button of hatButtons) {
  button.addEventListener("click", () => {
    const style = button.dataset.hatStyle;
    if (!isHatStyle(style)) {
      return;
    }
    applyAppearance({ ...characterAppearance, hat: style }, `Unsaved change · ${hatOption(style).name}`);
  });
}
for (const button of hairButtons) {
  button.addEventListener("click", () => {
    const hair = button.dataset.hairStyle;
    if (isHairStyle(hair)) { applyAppearance({ ...characterAppearance, hair }, `Unsaved change · ${button.textContent} hair`); }
  });
}
for (const button of hairColorButtons) {
  button.addEventListener("click", () => {
    const hairColor = button.dataset.hairColor;
    if (isHairColor(hairColor)) { applyAppearance({ ...characterAppearance, hairColor }, `Unsaved change · ${button.title} hair`); }
  });
}
saveCharacterButton.addEventListener("click", saveCurrentCharacter);
loadCharacterButton.addEventListener("click", loadSavedCharacter);
enterWorldButton.addEventListener("click", () => void enterWorld());
returnButton.addEventListener("click", () => returnToCharacters());
scenarioReset.addEventListener("click", () => void resetScenario());
window.addEventListener("keydown", (event) => {
  if (scenario && event.key === "F9" && !event.repeat) {
    event.preventDefault();
    void resetScenario();
  }
  const labSource = actionLab && scenarioWorld && activeWorld === scenarioWorld.world ? scenarioWorld.source : null;
  if (labSource && (event.key === "F7" || event.key === "F8") && !event.repeat) {
    event.preventDefault();
    if (event.key === "F7") { labSource.setPaused(!labSource.paused); } else { labSource.step(); }
  }
});
createCharacterButton.addEventListener("click", openCharacterCreation);
for (const button of cancelCreationButtons) {
  button.addEventListener("click", cancelCharacterCreation);
}
creationName.addEventListener("input", syncCreationDraft);
for (const input of [...classInputs, ...sexInputs]) {
  input.addEventListener("change", syncCreationDraft);
}
creationForm.addEventListener("submit", (event) => {
  event.preventDefault();
  finishCharacterCreation();
});
// Left drag orbits freely, right drag turns the character with the view; the wheel zooms.
canvas.addEventListener("pointerdown", (event) => {
  if (entryState.phase !== "world") {
    return;
  }
  canvas.focus();
  worldView.pointerDown(event);
});
canvas.addEventListener("pointermove", (event) => worldView.pointerMove(event));
for (const name of ["pointerup", "pointercancel", "lostpointercapture"] as const) {
  canvas.addEventListener(name, (event) => worldView.pointerEnd(event));
}
canvas.addEventListener("contextmenu", (event) => {
  if (entryState.phase === "world") {
    event.preventDefault();
  }
});
canvas.addEventListener("wheel", (event) => {
  if (entryState.phase !== "world") {
    return;
  }
  event.preventDefault();
  worldView.wheel(event);
}, { passive: false });

// One input path: keyboard and gamepad resolve to semantic actions through input-bindings.
// Movement is sampled from held actions each frame; one-shot actions are intents the zone decides.
function runAction(action: GameAction): void {
  if (arenaEntry.selected && (action === "ui.toggleBags" || action === "ui.toggleVendor" ||
    action === "ui.toggleQuestLog" || action === "ui.talk" || action === "player.releaseSpirit")) {
    return;
  }
  switch (action) {
    case "move.forward":
    case "move.back":
    case "move.strafeLeft":
    case "move.strafeRight":
    case "move.descend":
      return;
    case "move.jump":
      // The outbox sends one Jump per counted press; the zone decides whether it lifts off.
      jumps += 1;
      return;
    case "target.next":
      worldView.queueIntent((projection) => {
        const target = nextTabTarget(projection);
        return target ? { kind: "select-target", target } : null;
      });
      return;
    case "combat.toggleAutoAttack":
      worldView.queueIntent(attackToggle);
      return;
    case "ability.slot1":
    case "ability.slot2":
    case "ability.slot3":
    case "ability.slot4":
    case "ability.slot5":
    case "ability.slot6":
    case "ability.slot7":
    case "ability.slot8": {
      const slot = Number(action.slice(-1));
      const catalog = world?.catalog;
      worldView.queueIntent((projection) => (catalog ? useAbilitySlot(slot, projection, catalog) : null));
      return;
    }
    case "combat.cancelCast":
      worldView.queueIntent(cancelCast);
      return;
    case "player.releaseSpirit":
      worldView.queueIntent((projection) => (projection.viewer.dead ? { kind: "release-spirit" } : null));
      return;
    case "ui.toggleBags":
      worldView.toggleBags();
      return;
    case "ui.toggleCharacter":
      worldView.toggleCharacter();
      return;
    case "ui.toggleVendor":
      worldView.toggleVendor();
      return;
    case "ui.talk":
      worldView.interact();
      return;
    case "ui.toggleQuestLog":
      worldView.toggleQuestLog();
      return;
    case "ui.openChat":
      worldView.openChat();
      return;
    case "ui.closePanel":
      if (!worldView.closeLoot() && !worldView.closeVendor() && !worldView.closeQuestDialog()
        && !worldView.closeQuestLog() && !worldView.closeCharacter()) {
        worldView.closeBags();
      }
      return;
    case "ui.leaveWorld":
      returnToCharacters();
      return;
    case "ui.toggleDebug":
      worldView.toggleOverlay();
      return;
    case "ui.enterWorld":
      void enterWorld();
      return;
    case "ui.cancelCreation":
      if (joining) {
        // Cancelling rejects the pending join, which says so under Enter World.
        joining.source.leave();
      } else {
        cancelCharacterCreation();
      }
      return;
  }
}
/**
 * Held gestures. The shield guard (#156) is raised while its key is held and lowered by release or
 * any retirement. The held directed Heroic Strike (#154): press starts aiming, a deliberate release in the open
 * world sends one strike, and every retirement (blur, Settings, profile or screen change) or an
 * open panel cancels it without a strike.
 */
function runHold(action: HeldAction, phase: HoldPhase): void {
  switch (action) {
    case "combat.directedStrike":
      if (entryState.phase !== "world" || settingsOpen || worldView.panelOpen || phase === "cancel") {
        worldView.cancelDirectedStrike();
      } else if (phase === "press") {
        worldView.beginDirectedStrike();
      } else {
        worldView.releaseDirectedStrike();
      }
      return;
    case "combat.guard":
      // Release and every retirement lower the guard; only a press in the open world raises it.
      if (phase === "press" && entryState.phase === "world" && !settingsOpen && !worldView.panelOpen) {
        worldView.beginGuard();
      } else {
        worldView.endGuard();
      }
      return;
  }
}
controls.attach({ window, document });
// A mouse click leaves focus on a HUD or panel button (or a panel moves it to its close button),
// which would then own Space, Tab and Enter: Space re-pressed the button instead of jumping.
// After a mouse click in the world, focus on a button returns to the world; text fields (chat,
// quantities) keep it, and keyboard activation (detail 0) keeps normal keyboard navigation.
document.addEventListener("click", (event) => {
  if (entryState.phase !== "world" || event.detail === 0 || settingsPanel.open) {
    return;
  }
  const active = document.activeElement;
  if (active instanceof Element && active.closest("button, summary, a") && !active.closest("dialog")) {
    canvas.focus({ preventScroll: true });
  }
});
window.addEventListener("blur", () => {
  worldView.cancelPointer();
  touchControls.reset();
});
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    worldView.cancelPointer();
    touchControls.reset();
  }
});
window.addEventListener("resize", resize);
window.addEventListener("storage", (event) => {
  if (event.key !== characterRosterStorageKey() || !rosterStorageHealthy || creationDraft) {
    return;
  }
  try {
    const currentId = selectedCharacterId();
    const created = loadCreatedCharacters(window.localStorage);
    const next = [PREVIEW_CHARACTER, ...created];
    if (!next.some((character) => character.id === currentId)) {
      return;
    }
    characters = next;
    renderRoster();
  } catch {
    rosterStorageHealthy = false;
    rosterStatus.textContent = "Saved local roster became invalid; current in-memory characters were retained.";
  }
});
characterSelect.addEventListener("scroll", layoutPreview, { passive: true });
new ResizeObserver(layoutPreview).observe(previewSurface);

resize();
requestAnimationFrame(frame);
if (online.kind === "online") {
  onlineToggle.hidden = false;
  playOnlineInput.checked = online.preferred;
  onlineServerLabel.textContent = online.server.label;
}
playOnlineInput.addEventListener("change", () => {
  worldFailure = null;
  updateEntryButton();
});
window.addEventListener("pagehide", () => {
  // Commit the local character before the page goes; its lease stays until a newer tab supersedes it.
  if (world && activeWorld === world) {
    try {
      world.source.persist();
    } catch (error) {
      console.error(error);
    }
  }
});
function loadPageWorld(): Promise<LocalWorld> {
  // Without browser storage the characters still follow the record and lease rules, for this visit only.
  const characters = new LocalCharacters(browserStorage() ?? new VisitStorage(), navigator.locks);
  return loadLocalWorld(characters).then((loaded) => {
    localCharacters = characters;
    return loaded;
  });
}
worldView.onArtProgress = updateEntryButton;
void loadPageWorld().then((loaded) => {
  worldView.load(loaded);
  markStartup("view-loaded");
  world = loaded;
  refreshSelectionPresentation();
  renderRoster();
  markStartup("picker-ready");
  onlineWorld = online.kind === "online" ? createOnlineWorld(loaded, online.server) : null;
  arenaEntry.ready(loaded.catalog);
  refreshArenaWorld();
  if (scenario) {
    try {
      scenarioWorld = createScenarioWorld(loaded, scenario);
      mountActionLab(loaded, scenario, scenarioWorld.source);
    } catch (error) {
      worldLoadError = `Review scenario ${scenario.name}: ${errorMessage(error)}`;
    }
    updateEntryButton();
  }
}, (error: unknown) => {
  worldLoadError = errorMessage(error);
  updateEntryButton();
});
