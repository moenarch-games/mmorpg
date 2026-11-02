import type { RendererWorkObservations } from "@moritzbrantner/three-d-renderer";
import { POSE_DETAIL_LEVELS, POSE_DETAIL_PRESETS, type PoseDetailLevel } from "./units/pose-detail";

/**
 * The F3 debug overlay: frame rate, scene node counts and the renderer's
 * work observations for the last frame. It reads numbers only; measuring
 * never changes what is drawn.
 */
export type FrameStats = {
  fps: number;
  frameMs: number;
  nodes: number;
  staticNodes: number;
  visibleStaticNodes: number;
  staticVertices: number;
  units: number;
  poses?: Readonly<{ sampled: number; reused: number }>;
  work: RendererWorkObservations | null;
};

/** Frames-per-second over a sliding window of frame timestamps. */
export class FrameRate {
  readonly #times: number[] = [];
  readonly #window: number;

  constructor(windowMs = 1_000) {
    this.#window = windowMs;
  }

  sample(nowMs: number): void {
    if (!Number.isFinite(nowMs)) {
      return;
    }
    this.#times.push(nowMs);
    while (this.#times.length > 2 && nowMs - this.#times[0]! > this.#window) {
      this.#times.shift();
    }
  }

  get fps(): number {
    const first = this.#times[0];
    const last = this.#times.at(-1);
    if (first === undefined || last === undefined || last <= first) {
      return 0;
    }
    return ((this.#times.length - 1) * 1000) / (last - first);
  }

  get frameMs(): number {
    const fps = this.fps;
    return fps > 0 ? 1000 / fps : 0;
  }

  reset(): void {
    this.#times.length = 0;
  }
}

export class DebugOverlay {
  readonly #element: HTMLElement;
  readonly #list: HTMLDListElement;
  readonly #poseSelect: HTMLSelectElement | null;
  readonly #onPoseDetail: ((level: PoseDetailLevel) => void) | null;
  #shownAt = 0;
  #latest: FrameStats | null = null;

  constructor(element: HTMLElement, onPoseDetail: ((level: PoseDetailLevel) => void) | null = null) {
    this.#element = element;
    this.#list = document.createElement("dl");
    this.#element.append(this.#list);
    this.#onPoseDetail = onPoseDetail;
    this.#poseSelect = null;
    if (onPoseDetail) {
      const label = document.createElement("label");
      label.className = "debug-pose-detail";
      label.textContent = "Distant character poses";
      const select = document.createElement("select");
      select.setAttribute("aria-label", "Distant character animation detail");
      for (const level of POSE_DETAIL_LEVELS) {
        const option = document.createElement("option");
        option.value = level;
        option.textContent = POSE_DETAIL_PRESETS[level].label;
        select.append(option);
      }
      select.addEventListener("change", () => this.setPoseDetail(select.value as PoseDetailLevel));
      label.append(select);
      this.#element.append(label);
      this.#poseSelect = select;
    }
  }

  /** Also used by the browser profiling hook; keeps the F3 select in sync. */
  setPoseDetail(level: PoseDetailLevel): void {
    if (!(level in POSE_DETAIL_PRESETS)) { throw new Error(`Unknown pose detail level: ${level}`); }
    if (this.#poseSelect) { this.#poseSelect.value = level; }
    this.#onPoseDetail?.(level);
  }

  get visible(): boolean {
    return !this.#element.hidden;
  }

  get latest(): FrameStats | null {
    return this.#latest;
  }

  toggle(): void {
    this.#element.hidden = !this.#element.hidden;
    this.#shownAt = 0;
  }

  hide(): void {
    this.#element.hidden = true;
  }

  /** Records the frame and, at most four times a second while visible, redraws the text. */
  update(stats: FrameStats, nowMs: number): void {
    this.#latest = stats;
    if (this.#element.hidden || nowMs - this.#shownAt < 250) {
      return;
    }
    this.#shownAt = nowMs;
    const work = stats.work;
    const rows: [string, string][] = [
      ["fps", `${stats.fps.toFixed(1)} (${stats.frameMs.toFixed(1)} ms)`],
      ["nodes", `${stats.nodes} (${stats.units} units)`],
      ["static batches", `${stats.visibleStaticNodes} / ${stats.staticNodes} in range`],
      ["static vertices", stats.staticVertices.toLocaleString("en-US")],
    ];
    if (stats.poses) { rows.push(["poses sampled / reused", `${stats.poses.sampled} / ${stats.poses.reused}`]); }
    if (work) {
      rows.push(
        ["live objects", String(work.liveObjectCount)],
        ["live geometries", String(work.liveGeometryCount)],
        ["live materials", String(work.liveMaterialCount)],
        ["created / removed", `${work.objectCreateCount} / ${work.objectRemoveCount}`],
        ["geometry new / evicted", `${work.geometryCreateCount} / ${work.geometryEvictCount}`],
      );
    }
    const contents: HTMLElement[] = [];
    for (const [name, value] of rows) {
      const term = document.createElement("dt");
      term.textContent = name;
      const detail = document.createElement("dd");
      detail.textContent = value;
      contents.push(term, detail);
    }
    this.#list.replaceChildren(...contents);
    this.#element.dataset.stats = JSON.stringify({
      fps: Number(stats.fps.toFixed(2)),
      frameMs: Number(stats.frameMs.toFixed(2)),
      nodes: stats.nodes,
      units: stats.units,
      staticNodes: stats.staticNodes,
      visibleStaticNodes: stats.visibleStaticNodes,
      staticVertices: stats.staticVertices,
      poseDetail: this.#poseSelect?.value ?? "off",
      posesSampled: stats.poses?.sampled ?? null,
      posesReused: stats.poses?.reused ?? null,
      liveObjects: work?.liveObjectCount ?? null,
      liveGeometries: work?.liveGeometryCount ?? null,
      liveMaterials: work?.liveMaterialCount ?? null,
    });
  }
}
