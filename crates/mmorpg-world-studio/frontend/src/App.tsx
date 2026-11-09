import { MapView } from "@moritzbrantner/maps";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { lngLatToWorld, metresPerPixel, selectionAround, worldToLngLat, type Selection } from "./world";

type Job = { id: number; label: string; status: "queued" | "running" | "finished" | "failed"; log: string[] };
type Export = { id: string; name: string; selection: Selection; state: "rendering" | "ready" | "failed" };
type Report = {
  height: { changed: number; skippedLocked: number; lowestMetres: number; highestMetres: number };
  biome: { changed: number };
};
type Edit = {
  id: string;
  export: string;
  state: "previewing" | "previewed" | "applying" | "applied" | "reconciling" | "reconciled" | "failed";
  report: Report | null;
  branch: string | null;
  worktree: string | null;
  prompt: string | null;
};
type State = { tilesReady: boolean; jobs: Job[]; exports: Export[]; edits: Edit[] };

const LAYERS = ["beauty", "ground", "hillshade", "slope", "biome", "roads", "water", "locks"];
const START = worldToLngLat(0, 20);

async function post(path: string, body?: BodyInit, contentType?: string): Promise<Response> {
  const response = await fetch(path, { method: "POST", body, headers: contentType ? { "content-type": contentType } : undefined });
  if (!response.ok) {
    throw new Error((await response.text()) || `The studio refused this (${response.status}).`);
  }
  return response;
}

export function App() {
  const [state, setState] = useState<State | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [view, setView] = useState({ longitude: START[0], latitude: START[1], zoom: 3 });
  const [size, setSize] = useState(64);
  const [pixelsPerMetre, setPixelsPerMetre] = useState(8);
  const [exportId, setExportId] = useState<string | null>(null);
  const [direction, setDirection] = useState("");
  const logEnd = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    const poll = async () => {
      try {
        const response = await fetch("/api/state");
        if (alive) { setState(await response.json()); }
      } catch {
        if (alive) { setProblem("The studio backend is not answering. Start it with cargo run -p mmorpg-world-studio."); }
      }
    };
    void poll();
    const timer = setInterval(() => void poll(), 1000);
    return () => { alive = false; clearInterval(timer); };
  }, []);

  const act = useCallback(async (work: () => Promise<unknown>) => {
    setProblem(null);
    try { await work(); } catch (error) { setProblem(error instanceof Error ? error.message : String(error)); }
  }, []);

  const mapStyle = useMemo(() => ({
    version: 8 as const,
    sources: { greyhaven: { type: "raster" as const, tiles: [`${location.origin}/tiles/{z}/{x}/{y}.png`], tileSize: 256, maxzoom: 6 } },
    layers: [{ id: "greyhaven", type: "raster" as const, source: "greyhaven" }],
  }), []);
  const onViewStateChange = useCallback((next: { center: [number, number]; zoom: number }) => {
    setView({ longitude: next.center[0], latitude: next.center[1], zoom: next.zoom });
  }, []);

  const centre = lngLatToWorld(view.longitude, view.latitude);
  const selection = selectionAround(centre.x, centre.z, size, pixelsPerMetre);
  const framePixels = size / metresPerPixel(view.zoom);
  const current = state?.exports.find((candidate) => candidate.id === exportId) ?? state?.exports[0] ?? null;
  const edit = current ? state?.edits.find((candidate) => candidate.export === current.id) ?? null : null;
  const job = state?.jobs.at(-1) ?? null;
  const busy = state?.jobs.some((candidate) => candidate.status === "queued" || candidate.status === "running") ?? false;
  const logLength = job?.log.length ?? 0;
  useEffect(() => { logEnd.current?.scrollIntoView({ block: "end" }); }, [logLength]);

  return (
    <main className="studio">
      <section className="survey" aria-label="Greyhaven Vale map">
        {state?.tilesReady ? (
          <MapView
            mapStyle={mapStyle}
            defaultViewState={{ center: START, zoom: 3 }}
            fitToData={false}
            maxZoom={8}
            mapLabel="Greyhaven Vale"
            onViewStateChange={onViewStateChange}
            style={{ position: "absolute", inset: 0 }}
          />
        ) : (
          <div className="unmapped">
            <h2>The valley has not been mapped yet</h2>
            <p>Render the map once to survey it. This builds the game and photographs the whole zone from above, which takes a few minutes.</p>
            <button className="primary" disabled={busy || !state} onClick={() => void act(() => post("/api/tiles"))}>Render the map</button>
          </div>
        )}
        {state?.tilesReady && (
          <div className="frame" style={{ width: framePixels, height: framePixels }} aria-hidden="true">
            <span className="reading north">z {selection.minZ}</span>
            <span className="reading west">x {selection.minX}</span>
            <span className="reading south">z {selection.maxZ}</span>
            <span className="reading east">x {selection.maxX}</span>
          </div>
        )}
      </section>

      <aside className="notebook">
        <header>
          <h1>Greyhaven World Studio</h1>
          {state?.tilesReady && (
            <button className="quiet" disabled={busy} onClick={() => void act(() => post("/api/tiles"))}>Re-render map</button>
          )}
        </header>
        {problem && <p className="problem" role="alert">{problem}</p>}

        <ol className="steps">
          <li>
            <h2>Frame the place</h2>
            <p>Pan and zoom until the frame sits over what you want to change.</p>
            <label>
              <span>Frame size: {size} m</span>
              <input type="range" min={16} max={512} step={8} value={size} onChange={(event) => setSize(Number(event.target.value))} />
            </label>
            <label>
              <span>Detail</span>
              <select value={pixelsPerMetre} onChange={(event) => setPixelsPerMetre(Number(event.target.value))}>
                <option value={4}>4 pixels per metre</option>
                <option value={8}>8 pixels per metre</option>
                <option value={16}>16 pixels per metre</option>
              </select>
            </label>
            <button
              className="primary"
              disabled={busy || !state?.tilesReady}
              onClick={() => void act(async () => {
                const response = await post("/api/exports", JSON.stringify(selection), "application/json");
                setExportId((await response.json()).id);
              })}
            >
              Export this area
            </button>
          </li>

          <li className={current ? "" : "waiting"}>
            <h2>Paint it in Krita</h2>
            {!current && <p>An exported area appears here.</p>}
            {current?.state === "rendering" && <p>Rendering x {current.selection.minX} to {current.selection.maxX}, z {current.selection.minZ} to {current.selection.maxZ}.</p>}
            {current?.state === "failed" && <p className="problem">This export failed. The log below says why.</p>}
            {current?.state === "ready" && (
              <>
                <a className="primary" href={`/api/exports/${current.id}/document`}>Download Krita document</a>
                <p>Paint on the <em>paint height</em> and <em>paint biome</em> layers, and save as .ora. Don't resize the image.</p>
                <div className="contact">
                  {LAYERS.map((layer) => (
                    <a key={layer} href={`/api/exports/${current.id}/layers/${layer}`} target="_blank" rel="noreferrer">
                      <img src={`/api/exports/${current.id}/layers/${layer}`} alt="" loading="lazy" />
                      <span>{layer}</span>
                    </a>
                  ))}
                </div>
                <label className="upload">
                  <span>Bring the painted document back</span>
                  <input
                    type="file"
                    accept=".ora"
                    disabled={busy}
                    onChange={(event) => {
                      const file = event.target.files?.[0];
                      event.target.value = "";
                      if (file) { void act(() => post(`/api/exports/${current.id}/import`, file)); }
                    }}
                  />
                </label>
              </>
            )}
          </li>

          <li className={edit ? "" : "waiting"}>
            <h2>Change the terrain</h2>
            {!edit && <p>What your paint changes appears here before anything is applied.</p>}
            {edit?.state === "previewing" && <p>Reading your paint.</p>}
            {edit?.state === "failed" && <p className="problem">This import failed. The log below says why.</p>}
            {edit?.report && (
              <dl className="figures">
                <div><dt>Terrain samples moved</dt><dd>{edit.report.height.changed}</dd></div>
                <div><dt>Height change</dt><dd>{edit.report.height.lowestMetres} to {edit.report.height.highestMetres} m</dd></div>
                <div><dt>Biome samples changed</dt><dd>{edit.report.biome.changed}</dd></div>
                <div><dt>Locked samples skipped</dt><dd>{edit.report.height.skippedLocked}</dd></div>
              </dl>
            )}
            {edit?.state === "previewed" && (
              <button className="primary" disabled={busy} onClick={() => void act(() => post(`/api/edits/${edit.id}/apply`))}>Apply to a new branch</button>
            )}
            {edit?.branch && <p>Applied on <code>{edit.branch}</code>. Your checkout is untouched.</p>}
          </li>

          <li className={edit?.branch ? "" : "waiting"}>
            <h2>Let the agent reconcile it</h2>
            {!edit?.branch && <p>Once the terrain is applied, an agent can adapt roads, spawns and scenery to it.</p>}
            {edit?.state === "applied" && (
              <>
                <label>
                  <span>What should this place become?</span>
                  <textarea rows={3} value={direction} placeholder="The new hill is a lookout. Run a footpath up to it." onChange={(event) => setDirection(event.target.value)} />
                </label>
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() => void act(() => post(`/api/edits/${edit.id}/reconcile`, JSON.stringify({ direction }), "application/json"))}
                >
                  Start the agent
                </button>
                <p className="small">It works and commits only on this branch, up to $5, and never pushes.</p>
              </>
            )}
            {edit?.state === "reconciling" && <p>The agent is working on <code>{edit.branch}</code>.</p>}
            {edit?.state === "reconciled" && <p>Finished. Review the commits on <code>{edit.branch}</code> in <code>{edit.worktree}</code>.</p>}
          </li>
        </ol>

        {job && (
          <details className="log" open={job.status === "running" || job.status === "failed"}>
            <summary>{job.label}: {job.status}</summary>
            <pre tabIndex={0}>{job.log.join("\n")}<div ref={logEnd} /></pre>
          </details>
        )}
      </aside>
    </main>
  );
}
