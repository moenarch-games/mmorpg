import { createElement as h, useEffect, useMemo, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { GeoJsonLayer, MapView, type MapViewState } from "@moritzbrantner/maps";
import "@moritzbrantner/maps/styles.css";
import { wasm } from "../wasm";
import type { ExplorationMap } from "./exploration";
import { areaPolygon, metresPerPixel, pointMarker, POINT_STYLES, type PointKind } from "./map-features";

/**
 * The world map (M): the continent atlas drawn by the Maps library's own flat runtime
 * (`flatRuntime="maps"`, no MapLibre and no tiles), with your position and facing, the
 * playable zone outlined and region names on hover. The layers, where a zone position
 * lies on the map and what a region is called all come from `mmorpg-client-model`
 * through `mmorpg-wasm`, so the native client's map shows the same.
 */

/** Where the local character is: zone metres (x east, z south) and yaw in radians. */
export type MapSelf = { x: number; z: number; yaw: number };

/** The Maps library's own collection type, so its layers take ours unchanged. */
type Collection = Parameters<typeof GeoJsonLayer>[0]["featureCollection"];
type Feature = GeoJSON.Feature<GeoJSON.Geometry, Record<string, unknown>>;
/** What a layer callback receives: a feature's properties. */
type Hit = { properties: Record<string, unknown> };
type Bundle = { kingdoms: { id: string; name: string; color: string }[]; layers: Record<string, Feature[]> };

const SEA = "#2f5d7c";
const OPEN_ZOOM = 13.2; // about 6 km across a typical panel

/** `[longitude, latitude]` of a zone position in metres. */
function mapPoint(x: number, z: number): [number, number] {
  const [lon = 0, lat = 0] = wasm.worldMapZoneToMap(x, z);
  return [lon, lat];
}

/** A layer's features, each with an id (the library keys them; lakes and landmarks have none). */
function collection(features: Feature[], name = "feature"): Collection {
  return { type: "FeatureCollection", features: features.map((f, i) => ({ ...f, id: f.id ?? `${name}-${i}` })) } as Collection;
}

/** A triangle pointing along `bearing` (degrees from north) around a map point, `size` metres long. */
function headingTriangle([lon, lat]: [number, number], bearing: number, size: number): Feature {
  const metre = 1 / 111_319.49; // degrees per metre at the equator, where the map sits
  const point = (along: number, across: number): [number, number] => {
    const b = (bearing * Math.PI) / 180;
    const east = along * Math.sin(b) + across * Math.cos(b);
    const north = along * Math.cos(b) - across * Math.sin(b);
    return [lon + east * metre, lat + north * metre];
  };
  const ring = [point(size, 0), point(-size * 0.45, size * 0.55), point(-size * 0.2, 0), point(-size * 0.45, -size * 0.55), point(size, 0)];
  return { type: "Feature", id: "self-heading", geometry: { type: "Polygon", coordinates: [ring] }, properties: {} };
}

/** An approximate circle of `radius` metres around a zone point, as a map polygon. */
function circle(id: string, x: number, z: number, radius: number, properties: Record<string, unknown>): Feature {
  const ring = Array.from({ length: 33 }, (_, i) => {
    const angle = (i / 32) * 2 * Math.PI;
    return mapPoint(x + radius * Math.cos(angle), z + radius * Math.sin(angle));
  });
  return { type: "Feature", id, geometry: { type: "Polygon", coordinates: [ring] }, properties };
}

/**
 * The exploration overlay (#983): the named areas as their traced outlines (#1063), discovered ones warm and named,
 * undiscovered ones dim so the geography stays readable, and the active quests' approximate areas as gold circles
 * (Atlas 5.1.13).
 */
function explorationLayers(map: ExplorationMap) {
  const areas = map.areas.map((area) => areaPolygon(`area-${area.id}`, area.rings, mapPoint, { name: area.name, discovered: area.discovered }));
  const hints = map.hints.map((hint, i) => circle(`hint-${i}`, hint.center[0], hint.center[1], hint.radius, { label: hint.label }));
  const labels = map.areas.filter((area) => area.discovered).map((area) => {
    const at = mapPoint(area.centre[0], area.centre[1]);
    return { type: "Feature", id: `label-${area.id}`, geometry: { type: "Point", coordinates: at }, properties: { name: area.name } } as Feature;
  });
  return { areas: collection(areas, "area"), hints: collection(hints, "hint"), labels: collection(labels, "label") };
}

/** The discovered areas' points of interest (#1063), each kind its own shape about 14 px across at `zoom`. */
function pointLayer(map: ExplorationMap, zoom: number): Collection {
  const size = 14 * metresPerPixel(zoom);
  return collection(map.points.map((point, i) =>
    pointMarker(`point-${i}`, point.kind as PointKind, mapPoint(point.at[0], point.at[1]), size, { name: point.name, detail: point.detail })), "point");
}

function regionTooltip(id: unknown) {
  const region = typeof id === "string" ? wasm.worldMapRegion(id) : null;
  if (!region) return null;
  return h("div", { className: "world-map-tooltip" },
    h("strong", null, region.name ?? `Unnamed region (${region.id})`),
    h("span", null, region.kingdom),
    h("span", null, region.biome),
    region.zone ? h("span", { className: "world-map-zone-note" }, `${region.zone}: playable zone`) : null);
}

function WorldMapPanel({ self, exploration, onClose }: { self: () => MapSelf | null; exploration: () => ExplorationMap | null; onClose: () => void }) {
  const bundle = useMemo(() => JSON.parse(wasm.worldMapBundle()) as Bundle, []);
  const colours = useMemo(() => new Map(bundle.kingdoms.map((k) => [k.id, k.color])), [bundle]);
  const layers = useMemo(() => {
    const layer = (name: string): Collection => collection(bundle.layers[name] ?? [], name);
    return { regions: layer("regions"), lakes: layer("lakes"), rivers: layer("rivers"), roads: layer("roads"),
      settlements: layer("settlements"), gates: layer("gates"), zones: layer("zones") };
  }, [bundle]);
  const [me, setMe] = useState<MapSelf | null>(() => self());
  const [view, setView] = useState<MapViewState>(() => {
    const at: [number, number] = me ? mapPoint(me.x, me.z) : [0, 0];
    return { center: [at[0], at[1]], zoom: OPEN_ZOOM };
  });
  const [explored, setExplored] = useState<ExplorationMap | null>(() => exploration());
  const overlay = useMemo(() => (explored ? explorationLayers(explored) : null), [explored]);
  const points = useMemo(() => (explored ? pointLayer(explored, view.zoom) : null), [explored, view.zoom]);
  useEffect(() => {
    const timer = window.setInterval(() => setMe(self()), 200);
    const discoveries = window.setInterval(() => setExplored((last) => { const next = exploration(); return next === last ? last : next; }), 500);
    return () => { window.clearInterval(timer); window.clearInterval(discoveries); };
  }, [self, exploration]);

  const marker = useMemo(() => {
    if (!me) return null;
    const at = mapPoint(me.x, me.z);
    // about 18 px long at any zoom (Web Mercator, 512 px tiles, at the equator)
    const metresPerPixel = 40_075_016.7 / (512 * 2 ** view.zoom);
    return {
      heading: collection([headingTriangle(at, wasm.worldMapBearing(me.yaw), 18 * metresPerPixel)]),
      point: collection([{ type: "Feature", id: "self", geometry: { type: "Point", coordinates: at }, properties: {} }]),
    };
  }, [me, view.zoom]);

  return h("section", { className: "world-map-panel", role: "dialog", "aria-label": "World map" },
    h("header", { className: "world-map-header" },
      h("h1", null, "World map"),
      explored ? h("span", { className: "world-map-explored", "aria-label": `${explored.discovered} of ${explored.total} areas discovered` }, `Discovered ${explored.discovered} / ${explored.total}`) : null,
      h("span", { className: "world-map-hint" }, "M or Esc to close · drag to pan · wheel to zoom"),
      h("button", { type: "button", className: "world-map-close", onClick: onClose, "aria-label": "Close the world map" }, "×")),
    h("div", { className: "world-map-surface", style: { background: SEA } },
      h(MapView, {
        flatRuntime: "maps",
        mapStyle: { tiles: false },
        mapLabel: "World map",
        fitToData: false,
        showAttributionControl: false,

        maxZoom: 17,
        viewState: view,
        onViewStateChange: (next: MapViewState) => setView(next),
        style: { height: "100%", width: "100%", background: SEA },
      },
      h(GeoJsonLayer, {
        layerId: "world-map-regions",
        featureCollection: layers.regions,
        getFeatureId: (f: Hit) => String(f.properties.id),
        getFeatureStyle: (f: Hit) => ({
          polygonFillColor: colours.get(String(f.properties.kingdom)) ?? "#888888",
          polygonFillOpacity: 0.78,
          polygonStrokeColor: "#3b2b26",
          polygonStrokeWidth: 0.6,
        }),
        renderFeatureTooltip: (f: Hit) => regionTooltip(f.properties.id),
      }),
      h(GeoJsonLayer, { layerId: "world-map-lakes", featureCollection: layers.lakes, polygonFillColor: "#4f86a8", polygonFillOpacity: 1, polygonStrokeColor: "#3c6f8f", polygonStrokeWidth: 0.6, isFeatureInteractive: () => false }),
      h(GeoJsonLayer, { layerId: "world-map-rivers", featureCollection: layers.rivers, lineColor: "#3462a0", lineWidth: 2, isFeatureInteractive: () => false }),
      h(GeoJsonLayer, { layerId: "world-map-roads", featureCollection: layers.roads, lineColor: "#7a4a2a", lineWidth: 1.6, lineDasharray: [3, 2],
        renderFeatureTooltip: (f: Hit) => h("div", { className: "world-map-tooltip" }, h("strong", null, String(f.properties.name))) }),
      h(GeoJsonLayer, { layerId: "world-map-zones", featureCollection: layers.zones, polygonFillColor: "#d23c2d", polygonFillOpacity: 0.08, polygonStrokeColor: "#d23c2d", polygonStrokeWidth: 2.5,
        renderFeatureTooltip: (f: Hit) => h("div", { className: "world-map-tooltip" }, h("strong", null, String(f.properties.name)), h("span", null, "Playable zone")) }),
      h(GeoJsonLayer, { layerId: "world-map-settlements", featureCollection: layers.settlements,
        getFeatureStyle: (f: Hit) => ({ pointColor: f.properties.kind === "capital" ? "#8c1e1e" : "#2a1e1e", pointRadius: f.properties.kind === "capital" ? 6 : 4 }),
        renderFeatureTooltip: (f: Hit) => h("div", { className: "world-map-tooltip" }, h("strong", null, String(f.properties.name)), h("span", null, String(f.properties.kind))) }),
      h(GeoJsonLayer, { layerId: "world-map-gates", featureCollection: layers.gates, pointColor: "#3c3232", pointRadius: 4,
        renderFeatureTooltip: (f: Hit) => h("div", { className: "world-map-tooltip" }, h("strong", null, String(f.properties.name)), h("span", null, "Gate of the Shattered Crown")) }),
      overlay ? h(GeoJsonLayer, { layerId: "world-map-areas", featureCollection: overlay.areas,
        getFeatureStyle: (f: Hit) => f.properties.discovered
          ? { polygonFillColor: "#ffe9a8", polygonFillOpacity: 0.12, polygonStrokeColor: "#f5d27a", polygonStrokeWidth: 1.6 }
          : { polygonFillColor: "#1d1a14", polygonFillOpacity: 0.22, polygonStrokeColor: "#5b5446", polygonStrokeWidth: 0.8 },
        renderFeatureTooltip: (f: Hit) => h("div", { className: "world-map-tooltip" },
          h("strong", null, f.properties.discovered ? String(f.properties.name) : "Unexplored"),
          h("span", null, f.properties.discovered ? "Discovered" : "Not yet discovered")) }) : null,
      overlay ? h(GeoJsonLayer, { layerId: "world-map-quest-hints", featureCollection: overlay.hints, polygonFillColor: "#ffd34d", polygonFillOpacity: 0.16, polygonStrokeColor: "#ffd34d", polygonStrokeWidth: 2,
        renderFeatureTooltip: (f: Hit) => h("div", { className: "world-map-tooltip" }, h("strong", null, String(f.properties.label)), h("span", null, "Somewhere around here")) }) : null,
      overlay ? h(GeoJsonLayer, { layerId: "world-map-area-labels", featureCollection: overlay.labels, pointColor: "#f3e6b8", pointRadius: 3.5,
        renderFeatureTooltip: (f: Hit) => h("div", { className: "world-map-tooltip" }, h("strong", null, String(f.properties.name))) }) : null,
      points ? h(GeoJsonLayer, { layerId: "world-map-points", featureCollection: points,
        getFeatureStyle: (f: Hit) => {
          const style = POINT_STYLES[f.properties.kind as PointKind] ?? POINT_STYLES.landmark;
          return { polygonFillColor: style.fill, polygonFillOpacity: 1, polygonStrokeColor: style.stroke, polygonStrokeWidth: 1.2 };
        },
        renderFeatureTooltip: (f: Hit) => h("div", { className: "world-map-tooltip" }, h("strong", null, String(f.properties.name)), h("span", null, String(f.properties.detail))) }) : null,
      marker ? h(GeoJsonLayer, { layerId: "world-map-self-heading", featureCollection: marker.heading, polygonFillColor: "#ffd34d", polygonFillOpacity: 1, polygonStrokeColor: "#1d1a14", polygonStrokeWidth: 1.5, isFeatureInteractive: () => false }) : null,
      marker ? h(GeoJsonLayer, { layerId: "world-map-self", featureCollection: marker.point, pointColor: "#ffd34d", pointRadius: 5,
        renderFeatureTooltip: () => h("div", { className: "world-map-tooltip" }, h("strong", null, "You")) }) : null),
      explored ? h("ol", { className: "world-map-discoveries", "aria-label": "Discovered areas" },
        explored.hints.map((hint, i) => h("li", { key: `hint-${i}`, className: "world-map-hint-entry" }, h("span", { className: "world-map-swatch world-map-swatch-hint" }), hint.label)),
        explored.areas.filter((area) => area.discovered).map((area) => h("li", { key: area.id }, h("span", { className: "world-map-swatch world-map-swatch-area" }), area.name))) : null,
      h("ol", { className: "world-map-legend", "aria-label": "Kingdoms and places" },
        bundle.kingdoms.map((k) => h("li", { key: k.id }, h("span", { className: "world-map-swatch", style: { background: k.color } }), k.name)),
        (Object.keys(POINT_STYLES) as PointKind[]).map((kind) => h("li", { key: `point-${kind}` },
          h("span", { className: `world-map-swatch world-map-swatch-${kind}`, style: { background: POINT_STYLES[kind].fill } }), POINT_STYLES[kind].label))),
      h("button", { type: "button", className: "world-map-recentre", disabled: !me,
        onClick: () => { if (me) { const at = mapPoint(me.x, me.z); setView({ ...view, center: [at[0], at[1]] }); } } }, "Centre on me")));
}

/** Opens and closes the world map in `host`; the map runtime only exists while it is open. */
export class WorldMap {
  readonly #host: HTMLElement;
  readonly #self: () => MapSelf | null;
  readonly #exploration: () => ExplorationMap | null;
  #root: Root | null = null;

  constructor(host: HTMLElement, self: () => MapSelf | null, exploration: () => ExplorationMap | null = () => null) {
    this.#host = host;
    this.#self = self;
    this.#exploration = exploration;
    this.#host.hidden = true;
  }

  get isOpen(): boolean {
    return this.#root !== null;
  }

  toggle(): void {
    if (this.isOpen) this.close();
    else this.open();
  }

  open(): void {
    if (this.#root) return;
    this.#host.hidden = false;
    this.#root = createRoot(this.#host);
    this.#root.render(h(WorldMapPanel, { self: this.#self, exploration: this.#exploration, onClose: () => this.close() }));
  }

  close(): void {
    this.#root?.unmount();
    this.#root = null;
    this.#host.hidden = true;
  }
}
