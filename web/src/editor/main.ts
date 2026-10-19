import * as THREE from "three";
import { createThreeSceneRenderer, type Matrix4Values } from "@moritzbrantner/three-d-renderer";
import init, { LayoutPreview, layoutSource } from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import { buildSceneryScene, SceneryFrame } from "../world/scenery-nodes";
import { decodeScenery, type Scenery } from "../world/scenery";
import { ENVIRONMENT, environmentAt } from "../world/environment";
import "./styles.css";

type Collision = { id: number; position: [number, number]; removed?: boolean };
type Decoration = { id: string; kind: string; position: [number, number]; yaw: number };
type Document = { version: number; generator: string; collision: Collision[]; decoration: Decoration[] };
type ObjectPlacement = { id: string; kind: string; position: [number, number]; half: [number, number]; yaw: number; authority: "collision" | "decoration" };

function element<T extends HTMLElement>(id: string): T {
  const value = document.getElementById(id);
  if (!value) { throw new Error(`Missing editor element ${id}`); }
  return value as T;
}
const map = element<HTMLCanvasElement>("map");
const canvas = element<HTMLCanvasElement>("preview");
const status = element("status");
const xInput = element<HTMLInputElement>("x");
const zInput = element<HTMLInputElement>("z");
const yawInput = element<HTMLInputElement>("yaw");
const save = element<HTMLButtonElement>("save");
const renderer = createThreeSceneRenderer(canvas, { alpha: true, antialias: true, pixelRatioLimit: 2 });
const camera = new THREE.PerspectiveCamera(48, 1, 0.1, 1500);
const history: string[] = [];
let doc: Document;
let saved = "";
let diskSource: string | null = null;
let preview: LayoutPreview;
let frame: SceneryFrame;
let scenery: Scenery;
let objects: ObjectPlacement[] = [];
let selected: string | null = new URLSearchParams(location.search).get("object");
let dragging: { id: number; offset: [number, number]; previous: string } | null = null;
let saving = false;

function source(): string { return `${JSON.stringify(doc, null, 2)}\n`; }
function current(): ObjectPlacement | undefined { return objects.find((object) => object.id === selected); }
function dirty(): boolean { return source() !== saved; }
function report(error: unknown): void { status.textContent = error instanceof Error ? error.message : String(error); }

function accept(next: string, remember = true): boolean {
  let candidate: LayoutPreview | undefined;
  try {
    // Rust validates the same final core content used by the actual game before any edit is accepted.
    candidate = new LayoutPreview(next);
    const decoded = decodeScenery(candidate.scenery());
    const nextFrame = new SceneryFrame(buildSceneryScene(decoded, candidate));
    if (remember && doc) { history.push(source()); }
    preview?.free();
    preview = candidate;
    candidate = undefined;
    frame = nextFrame;
    scenery = decoded;
    doc = JSON.parse(next) as Document;
    objects = JSON.parse(preview.objects()) as ObjectPlacement[];
    status.textContent = "Valid layout · roads and spawn areas clear · preview uses shared game geometry";
    refresh();
    return true;
  } catch (error) { candidate?.free(); report(error); return false; }
}

function select(id: string | null): void {
  selected = id;
  const url = new URL(location.href);
  if (id) { url.searchParams.set("object", id); } else { url.searchParams.delete("object"); }
  window.history.replaceState(null, "", url);
  refresh();
}

function refresh(): void {
  element("dirty").textContent = dirty() ? "Unsaved changes" : "Saved";
  const list = element("objects");
  list.replaceChildren();
  for (const object of objects) {
    const button = document.createElement("button");
    button.textContent = `${object.kind} / ${object.id}`;
    button.setAttribute("role", "option");
    button.setAttribute("aria-selected", String(object.id === selected));
    button.addEventListener("click", () => select(object.id));
    list.append(button);
  }
  const object = current();
  element("selection").textContent = object ? `${object.kind} / ${object.id}` : "Select an object";
  element("authority").textContent = object?.authority === "collision" ? "Authoritative collision · existing house prefab · axis aligned" : "Presentation only · no gameplay collider";
  for (const input of [xInput, zInput, yawInput]) { input.disabled = !object || saving; }
  xInput.value = object ? String(object.position[0] / 100) : "";
  zInput.value = object ? String(object.position[1] / 100) : "";
  yawInput.value = object ? String(Math.round(object.yaw * 360 / 65536)) : "";
  yawInput.disabled = !object || object.authority === "collision" || saving;
  element<HTMLButtonElement>("remove").disabled = !object || saving;
  element<HTMLButtonElement>("undo").disabled = history.length === 0 || saving;
  drawMap();
}

function move(position: [number, number], yaw = current()?.yaw ?? 0): void {
  const object = current();
  if (!object || saving) { return; }
  const next = structuredClone(doc);
  if (object.authority === "collision") {
    let edit = next.collision.find((entry) => String(entry.id) === object.id);
    if (!edit) { edit = { id: Number(object.id), position }; next.collision.push(edit); }
    edit.position = position;
  } else {
    const edit = next.decoration.find((entry) => entry.id === object.id);
    if (edit) { edit.position = position; edit.yaw = yaw; }
  }
  accept(JSON.stringify(next));
}

function remove(): void {
  const object = current();
  if (!object || saving) { return; }
  const next = structuredClone(doc);
  if (object.authority === "collision") {
    next.collision = next.collision.filter((edit) => String(edit.id) !== object.id);
    next.collision.push({ id: Number(object.id), position: object.position, removed: true });
  } else { next.decoration = next.decoration.filter((edit) => edit.id !== object.id); }
  if (accept(JSON.stringify(next))) { select(null); }
}

// Fixed first-region viewport in centimetres. Camera and terrain remain game-owned.
const viewport = { x: -3600, z: -1600, size: 7200 };
function transform(): { scale: number; left: number; top: number } {
  const scale = Math.min(map.width, map.height) / viewport.size;
  return { scale, left: (map.width - viewport.size * scale) / 2, top: (map.height - viewport.size * scale) / 2 };
}
function world(event: PointerEvent): [number, number] {
  const rect = map.getBoundingClientRect();
  const { scale, left, top } = transform();
  return [Math.round(((event.clientX - rect.left) * map.width / rect.width - left) / scale + viewport.x),
    Math.round(((event.clientY - rect.top) * map.height / rect.height - top) / scale + viewport.z)];
}
function drawMap(): void {
  const context = map.getContext("2d");
  if (!context || !scenery) { return; }
  const { scale, left, top } = transform();
  context.fillStyle = "#26342a"; context.fillRect(0, 0, map.width, map.height);
  context.save(); context.translate(left - viewport.x * scale, top - viewport.z * scale); context.scale(scale, scale);
  const terrain = scenery.terrain;
  for (let row = 0; row < terrain.rows; row++) {
    for (let column = 0; column < terrain.columns; column++) {
      const x = terrain.originXz[0] + column * terrain.step;
      const z = terrain.originXz[1] + row * terrain.step;
      if (x < -4000 || x > 4000 || z < -2000 || z > 6000) { continue; }
      context.fillStyle = scenery.biomes.find((biome) => biome.id === terrain.biomes[row * terrain.columns + column])?.color ?? "#546443";
      context.fillRect(x, z, terrain.step, terrain.step);
    }
  }
  context.strokeStyle = "#ffffff15"; context.lineWidth = 1 / scale;
  for (let offset = -4000; offset <= 6000; offset += 200) {
    context.beginPath(); context.moveTo(offset, -1600); context.lineTo(offset, 5600); context.stroke();
    context.beginPath(); context.moveTo(-3600, offset); context.lineTo(3600, offset); context.stroke();
  }
  for (const road of scenery.roads) {
    context.strokeStyle = "#b5a27c"; context.lineWidth = road.halfWidth * 2; context.beginPath();
    road.points.forEach(([x, z], index) => { if (index === 0) { context.moveTo(x, z); } else { context.lineTo(x, z); } }); context.stroke();
  }
  context.fillStyle = "#98c9d433"; context.fillRect(-1600, 1200, 3200, 1600);
  context.strokeStyle = "#d6e8c999"; context.lineWidth = 2 / scale; context.strokeRect(-3100, -900, 6200, 5800);
  for (const prop of scenery.props) {
    const [x, , z] = prop.position;
    if (objects.some((object) => object.authority === "collision" && Number(object.id) === prop.collider?.id)) { continue; }
    if (!prop.collider) { context.fillStyle = "#31482a88"; context.fillRect(x - 15, z - 15, 30, 30); continue; }
    context.fillStyle = "#56614f";
    context.fillRect(x - prop.halfExtents[0], z - prop.halfExtents[2], prop.halfExtents[0] * 2, prop.halfExtents[2] * 2);
  }
  for (const object of objects) {
    const [x, z] = object.position;
    const hx = Math.max(45, object.half[0]); const hz = Math.max(45, object.half[1]);
    context.fillStyle = object.authority === "collision" ? "#b98459" : "#cae3a4";
    context.fillRect(x - hx, z - hz, hx * 2, hz * 2);
    if (object.id === selected) { context.strokeStyle = "#ffe289"; context.lineWidth = 4 / scale; context.strokeRect(x - hx - 20, z - hz - 20, hx * 2 + 40, hz * 2 + 40); }
    context.fillStyle = "#17221b"; context.font = `${11 / scale}px system-ui`; context.fillText(object.kind === "house" ? object.id : "●", x - hx + 20, z + 25);
  }
  context.fillStyle = "#e5f0dc"; context.font = `${12 / scale}px system-ui`;
  context.fillText("Spawn plaza · protected", -1100, 2050);
  context.restore();
}

map.addEventListener("pointerdown", (event) => {
  if (saving || event.button !== 0) { return; }
  const [x, z] = world(event);
  const object = [...objects].reverse().find((entry) => Math.abs(x - entry.position[0]) < Math.max(90, entry.half[0]) && Math.abs(z - entry.position[1]) < Math.max(90, entry.half[1]));
  select(object?.id ?? null); map.focus();
  if (object) { dragging = { id: event.pointerId, offset: [object.position[0] - x, object.position[1] - z], previous: source() }; map.setPointerCapture(event.pointerId); }
});
map.addEventListener("pointermove", (event) => {
  const object = current();
  if (!dragging || !object) { return; }
  const [x, z] = world(event);
  object.position = [Math.round((x + dragging.offset[0]) / 10) * 10, Math.round((z + dragging.offset[1]) / 10) * 10];
  drawMap();
});
map.addEventListener("pointerup", () => {
  const object = current();
  if (!dragging || !object) { return; }
  const position = object.position;
  dragging = null;
  objects = JSON.parse(preview.objects()) as ObjectPlacement[];
  move(position);
  refresh();
});
map.addEventListener("pointercancel", () => { if (dragging) { const previous = dragging.previous; dragging = null; accept(previous, false); } });
map.addEventListener("keydown", (event) => {
  const object = current();
  if (!object) { return; }
  if (event.key === "Delete") { event.preventDefault(); remove(); return; }
  const directions: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
  const direction = directions[event.key];
  if (direction) { event.preventDefault(); const step = event.shiftKey ? 100 : 10; move([object.position[0] + direction[0] * step, object.position[1] + direction[1] * step]); }
});
for (const input of [xInput, zInput, yawInput]) {
  input.addEventListener("change", () => {
    if (![xInput, zInput, yawInput].every((field) => field.value !== "" && Number.isFinite(field.valueAsNumber))) { report("Enter finite coordinates and yaw"); return; }
    move([Math.round(xInput.valueAsNumber * 100), Math.round(zInput.valueAsNumber * 100)], ((Math.round(yawInput.valueAsNumber / 360 * 65536) % 65536) + 65536) % 65536);
    refresh();
  });
}
element("remove").addEventListener("click", remove);
element("undo").addEventListener("click", () => { const previous = history.pop(); if (previous && !saving) { accept(previous, false); } });
element("add").addEventListener("click", () => {
  if (saving) { return; }
  const next = structuredClone(doc);
  const kind = element<HTMLSelectElement>("prefab").value;
  let id: string;
  if (kind === "house") {
    let available = 60000;
    while (next.collision.some((entry) => entry.id === available)) { available++; }
    id = String(available);
    // Start in the open west-side plot, away from the roads and spawn plaza.
    next.collision.push({ id: available, position: [-2300, 2800] });
  } else {
    id = `authored-${crypto.randomUUID()}`;
    next.decoration.push({ id, kind, position: [-1800, 1700], yaw: 0 });
  }
  if (accept(JSON.stringify(next))) { select(id); }
});

async function readDisk(): Promise<string | null> {
  const response = await fetch("/__layout");
  if (!response.ok || !response.headers.get("Content-Type")?.includes("application/json")) { return null; }
  return (await response.json() as { source: string }).source;
}
save.addEventListener("click", () => {
  if (diskSource === null || saving) { return; }
  saving = true; save.disabled = true; refresh();
  status.textContent = "Validating, saving and rebuilding the game. Keep this page open…";
  const next = source();
  fetch("/__layout", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ source: next, expected: diskSource }) })
    .then(async (response) => {
      const result = await response.json() as { source?: string; error?: string };
      if (!response.ok || !result.source) { throw new Error(result.error ?? "Save failed"); }
      diskSource = result.source; saved = next;
      status.textContent = "Saved and rebuilt. Open game to play this layout. Native host/client use it on their next build.";
    }).catch(report).finally(() => { saving = false; save.disabled = false; refresh(); });
});
element("reopen").addEventListener("click", () => {
  if (saving || (dirty() && !confirm("Discard unsaved layout changes?"))) { return; }
  readDisk().then((value) => {
    const next = value ?? saved;
    if (accept(next)) { diskSource = value; saved = source(); history.length = 0; refresh(); }
  }).catch(report);
});
element("export").addEventListener("click", () => {
  const url = URL.createObjectURL(new Blob([source()], { type: "application/json" }));
  const link = document.createElement("a"); link.href = url; link.download = "outpost-layout.json"; link.click(); URL.revokeObjectURL(url);
});
element<HTMLInputElement>("import").addEventListener("change", (event) => {
  const file = (event.target as HTMLInputElement).files?.[0];
  if (file && !saving) { file.text().then((text) => { accept(text); }).catch(report); }
});
window.addEventListener("beforeunload", (event) => { if (dirty() && !saving) { event.preventDefault(); } });
new ResizeObserver(() => {
  map.width = Math.round(map.clientWidth * devicePixelRatio); map.height = Math.round(map.clientHeight * devicePixelRatio); drawMap();
}).observe(map);

function render(): void {
  if (!frame) { return; }
  const object = current();
  const follow = element<HTMLSelectElement>("view").value === "selection" && object;
  const target = follow ? [object.position[0] / 100, 2, object.position[1] / 100] : [0, 1, 18];
  const distance = follow ? 20 : 70;
  camera.aspect = canvas.clientWidth / Math.max(1, canvas.clientHeight);
  camera.position.set(target[0]! + distance * .65, target[1]! + distance * .8, target[2]! + distance * .7);
  camera.lookAt(target[0]!, target[1]!, target[2]!); camera.updateProjectionMatrix(); camera.updateMatrixWorld(true);
  const projection = [...camera.projectionMatrix.elements];
  for (const index of [2, 6, 10, 14]) { projection[index] = (projection[index]! + projection[index + 1]!) / 2; }
  const rendered = frame.nodes([camera.position.x, camera.position.y, camera.position.z], { seconds: 0, animate: false });
  renderer.render({ camera: { viewMatrix: [...camera.matrixWorldInverse.elements] as Matrix4Values, projectionMatrix: projection as Matrix4Values }, nodes: rendered.nodes, instanceBatches: rendered.instanceBatches, environment: environmentAt(ENVIRONMENT, 0) });
  requestAnimationFrame(render);
}

async function start(): Promise<void> {
  await init();
  diskSource = await readDisk().catch(() => null);
  if (!accept(diskSource ?? layoutSource(), false)) { throw new Error("Cannot open the committed layout"); }
  saved = source(); refresh();
  save.disabled = diskSource === null;
  if (diskSource === null) { status.textContent = "Preview/export mode. Run bun run editor locally to save and rebuild the game."; }
  requestAnimationFrame(render);
}
start().catch(report);
