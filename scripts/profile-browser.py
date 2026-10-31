#!/usr/bin/env python3
"""Greyhaven gameplay frame-time and keyboard-latency profile in Chromium (#546).

Drives the production bundle (`web/dist`, `?debug&profile`) through four scenarios and prints
one JSON report:

- `selection-keys`: character-selection keyboard input (turntable arrows, Tab, Enter → Enter World);
- `idle`: the Greyhaven view after entry, nobody touching anything;
- `move-rotate`: W held while the mouse drags the camera, plus repeated W taps from standstill
  for keyboard → first drawn movement;
- `dense-woods` / `dense-hub` / `overview`: the woods (vegetation), the outpost hub (NPCs) and the
  whole-vale overview, through the existing debug viewpoints.

`--activity battle-royale` profiles the Battle Royale practice arena (#552) instead: `idle`,
`move-rotate` and `key-to-motion` in the fortress courtyard, and `overview` over the whole fortress.

Sources, all opt-in and read-only:
- the page's `?profile` frame phases (`web/src/world/frame-profile.ts`): main-thread CPU per phase;
- Event Timing (`event` entries: input delay, handler time, duration to next paint, i.e. INP) and
  Long Animation Frames, from a `PerformanceObserver` this script installs;
- WebGL draw calls and triangles from wrapped draw entry points;
- GPU execution time of each frame's WebGL commands via `EXT_disjoint_timer_query_webgl2`, only when
  the browser exposes it (reported as unsupported otherwise);
- a Chromium trace of `move-rotate` (`--trace`): busy time of the renderer main, GPU and
  compositor threads, GC, and the longest renderer tasks.

`requestAnimationFrame` intervals are frame pacing, not GPU time; they are reported as such.

Prerequisites: build web/ (bun run build) and Playwright for Python (see smoke-browser.py).
Run from the repository root:
    python scripts/profile-browser.py --out artifacts/profile/report.json --trace artifacts/profile
Defaults: headless Chromium on the hardware GPU (ANGLE OpenGL), 1440x900 at pixel ratio 1, then a
viewport/pixel-ratio sweep of `idle` and `overview`.
"""
from __future__ import annotations

import argparse
import functools
import http.server
import json
import os
import re
import statistics
import sys
import threading
import time
from pathlib import Path

from playwright.sync_api import TimeoutError as PlaywrightTimeout, expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
GPU_ARGS = ["--enable-gpu", "--use-angle=gl", "--ignore-gpu-blocklist"]
SWEEP = [(1280, 720, 1), (1920, 1080, 1), (1920, 1080, 2), (2560, 1440, 2)]
TRACE_CATEGORIES = [
    "toplevel", "devtools.timeline", "disabled-by-default-devtools.timeline", "blink.user_timing",
    "v8", "v8.execute", "gpu", "viz", "cc", "benchmark", "loading",
]

INIT_SCRIPT = r"""(() => {
  const probe = window.__probe = {
    keys: [], events: [], loaf: [], draws: 0, triangles: 0, gpu: new Map(), gpuSupported: null,
  };
  addEventListener("keydown", (event) => probe.keys.push({ code: event.code, t: event.timeStamp, repeat: event.repeat }), true);
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        probe.events.push({ name: e.name, start: e.startTime, processingStart: e.processingStart,
          processingEnd: e.processingEnd, duration: e.duration, interactionId: e.interactionId ?? 0 });
      }
    }).observe({ type: "event", durationThreshold: 16, buffered: true });
  } catch {}
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        probe.loaf.push({ start: e.startTime, duration: e.duration, blocking: e.blockingDuration,
          render: e.renderStart ? e.startTime + e.duration - e.renderStart : 0,
          scripts: (e.scripts ?? []).slice(0, 3).map((s) => ({ invoker: s.invoker, duration: s.duration, source: s.sourceFunctionName || s.sourceURL })) });
      }
    }).observe({ type: "long-animation-frame", buffered: true });
  } catch {}
  for (const Type of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
    if (!Type) continue;
    const count = (mode, n, instances) => { probe.draws++; if (mode === 4) probe.triangles += (n / 3) * (instances ?? 1); };
    const wrap = (name, fn) => { const original = Type.prototype[name]; if (original) Type.prototype[name] = function (...a) { fn(a); return original.apply(this, a); }; };
    wrap("drawArrays", (a) => count(a[0], a[2]));
    wrap("drawElements", (a) => count(a[0], a[1]));
    wrap("drawArraysInstanced", (a) => count(a[0], a[2], a[3]));
    wrap("drawElementsInstanced", (a) => count(a[0], a[1], a[4]));
  }
  // GPU time per animation frame, only where the timer-query extension exists.
  let gl = null, ext = null, active = false;
  const pending = [];
  const getContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (kind, ...rest) {
    const context = getContext.call(this, kind, ...rest);
    if (this.id === "world" && kind === "webgl2" && context && !gl) {
      gl = context;
      ext = gl.getExtension("EXT_disjoint_timer_query_webgl2");
      probe.gpuSupported = !!ext;
    }
    return context;
  };
  const poll = () => {
    while (pending.length) {
      const { query, raf } = pending[0];
      if (!gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE)) break;
      pending.shift();
      if (!gl.getParameter(ext.GPU_DISJOINT_EXT)) {
        probe.gpu.set(raf, (probe.gpu.get(raf) ?? 0) + gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6);
      }
      gl.deleteQuery(query);
    }
  };
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (callback) => raf((time) => {
    if (!ext || active) { callback(time); return; }
    poll();
    const query = gl.createQuery();
    active = true;
    gl.beginQuery(ext.TIME_ELAPSED_EXT, query);
    try { callback(time); } finally { gl.endQuery(ext.TIME_ELAPSED_EXT); active = false; pending.push({ query, raf: time }); }
  });
  probe.take = () => {
    const out = { keys: probe.keys.splice(0), events: probe.events.splice(0), loaf: probe.loaf.splice(0),
      draws: probe.draws, triangles: probe.triangles, gpu: [...probe.gpu], gpuSupported: probe.gpuSupported };
    probe.draws = 0; probe.triangles = 0; probe.gpu.clear();
    return out;
  };
})();"""


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/mmorpg/"):
            self.path = self.path[len("/mmorpg"):]
        super().do_GET()

    def log_message(self, *_args):
        pass


def pct(values, p):
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round(p / 100 * (len(ordered) - 1))))
    return round(ordered[index], 2)


def dist(values):
    return {"n": len(values), "p50": pct(values, 50), "p95": pct(values, 95), "p99": pct(values, 99),
            "max": round(max(values), 2) if values else None, "mean": round(statistics.fmean(values), 2) if values else None}


class Session:
    def __init__(self, browser, url, width, height, scale, activity=None):
        self.width, self.height, self.scale = width, height, scale
        self.activity = activity
        self.context = browser.new_context(viewport={"width": width, "height": height}, device_scale_factor=scale)
        self.context.add_init_script(INIT_SCRIPT)
        self.page = self.context.new_page()
        self.errors = []
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        started = time.monotonic()
        self.page.goto(url + "?debug&profile" + (f"&activity={activity}" if activity else ""))
        expect(self.enter_button()).to_be_enabled(timeout=60_000)
        self.cold_selection_s = round(time.monotonic() - started, 2)

    def enter_button(self):
        # The Battle Royale practice arena (#550, #552) enters through its own button.
        name = "^Start practice" if self.activity == "battle-royale" else "^Enter World"
        return self.page.get_by_role("button", name=re.compile(name))

    def frames(self, count):
        self.page.evaluate("""count => new Promise(resolve => {
          function next() { if (--count <= 0) resolve(); else requestAnimationFrame(next); }
          requestAnimationFrame(next);
        })""", count)

    def take(self):
        return {"frames": self.page.evaluate("window.__valeProfile.take()"), **self.page.evaluate("window.__probe.take()")}

    def renderer(self):
        return self.page.evaluate("""() => { const gl = document.querySelector('#world').getContext('webgl2');
          const d = gl.getExtension('WEBGL_debug_renderer_info');
          return { renderer: d ? gl.getParameter(d.UNMASKED_RENDERER_WEBGL) : null,
            drawingBuffer: [gl.drawingBufferWidth, gl.drawingBufferHeight], devicePixelRatio };
        }""")

    def enter_world(self):
        self.page.locator("#world").focus()
        self.enter_button().focus()
        self.page.keyboard.press("Enter")
        expect(self.page.locator("#character-select")).to_be_hidden(timeout=60_000)
        expect(self.page.locator(".hud")).to_be_visible()
        if self.activity == "battle-royale":
            # The fortress kit (#591) is the arena's art; profile it, not the graybox it replaces.
            # A bundle that cannot load leaves the graybox, which is profiled instead.
            try:
                self.page.wait_for_function("() => window.__valeDebug.fortress()?.loaded", timeout=60_000)
            except PlaywrightTimeout:
                print("The fortress kit did not load; profiling the graybox", file=sys.stderr)
        self.page.locator("#world").focus()

    def warm(self, seconds=3.0):
        self.page.wait_for_timeout(seconds * 1000)
        self.take()

    def record(self, seconds):
        self.take()
        self.page.wait_for_timeout(seconds * 1000)
        return self.take()

    def close(self):
        self.context.close()


def summarize(sample, seconds):
    frames = sample["frames"]
    raf = [b["raf"] - a["raf"] for a, b in zip(frames, frames[1:])]
    cpu = [f["end"] - f["start"] for f in frames]
    phases = {name: dist([f["phases"][name] for f in frames]) for name in (frames[0]["phases"] if frames else {})}
    gpu = [ms for _, ms in sample["gpu"]]
    worst = max(frames, key=lambda f: f["end"] - f["start"]) if frames else None
    return {
        "seconds": seconds,
        "fps": round(len(frames) / seconds, 1),
        "rafIntervalMs": dist(raf),
        "cpuFrameMs": dist(cpu),
        "phasesMs": {name: {"p50": d["p50"], "p95": d["p95"], "p99": d["p99"], "max": d["max"]} for name, d in phases.items()},
        "worstCpuFrame": {"ms": round(worst["end"] - worst["start"], 2), "phases": {k: round(v, 2) for k, v in worst["phases"].items()}} if worst else None,
        # The five slowest frames with what the renderer created in them: churn marks a resource stall.
        "worstCpuFrames": [{"ms": round(f["end"] - f["start"], 1), "render": round(f["phases"]["render"], 1),
                            "simulation": round(f["phases"]["simulation"], 1), "ticks": f["ticks"], "created": f["created"]}
                           for f in sorted(frames, key=lambda f: f["start"] - f["end"])[:5]],
        "framesCreatingResources": sum(1 for f in frames if f["created"]["geometries"] or f["created"]["materials"] or f["created"]["objects"]),
        "ticksPerFrame": dist([f["ticks"] for f in frames]),
        "nodes": pct([f["nodes"] for f in frames], 50),
        "instanceBatches": pct([f["instanceBatches"] for f in frames], 50),
        "drawCallsPerFrame": round(sample["draws"] / max(len(frames), 1), 1),
        "trianglesPerFrame": round(sample["triangles"] / max(len(frames), 1)),
        "gpuMs": dist(gpu) if sample["gpuSupported"] else "unsupported",
        "longAnimationFrames": {
            "count": len(sample["loaf"]),
            "worst": sorted(sample["loaf"], key=lambda e: -e["duration"])[:3],
        },
        # Event Timing reports only events of 16 ms or more; faster ones are absent, not zero.
        "interactionsOver16Ms": dist([e["duration"] for e in sample["events"] if e["interactionId"]]),
        "slowEvents": sorted(({k: round(v, 1) if isinstance(v, float) else v for k, v in e.items()} for e in sample["events"]), key=lambda e: -e["duration"])[:5],
    }


def interactions(events):
    """Event Timing per interaction: input delay, handler time and duration to the next paint (INP)."""
    out = []
    for e in events:
        if e["interactionId"] == 0 and e["name"] not in ("keydown", "keyup"):
            continue
        out.append({"name": e["name"], "inputDelay": round(e["processingStart"] - e["start"], 1),
                    "handler": round(e["processingEnd"] - e["processingStart"], 1), "duration": round(e["duration"], 1)})
    return out


def selection_keys(session):
    """Character-selection keyboard: turntable arrows, Tab through controls, then Enter on Enter World."""
    page = session.page
    session.take()
    slider = page.get_by_role("slider", name="Character rotation")
    slider.focus()
    for key in ["ArrowRight"] * 10 + ["ArrowLeft"] * 10:
        page.keyboard.press(key)
        page.wait_for_timeout(120)
    for _ in range(6):
        page.keyboard.press("Tab")
        page.wait_for_timeout(120)
    arrows = session.take()
    entered = time.monotonic()

    def enter():
        session.enter_world()
        session.frames(10)
    profile = cpu_profile(session, enter)
    entry_s = round(time.monotonic() - entered, 2)
    entry = session.take()
    entry_events = interactions(entry["events"])
    return {
        "arrowsAndTab": {"interactions": len(interactions(arrows["events"])),
                         "durationMs": dist([e["duration"] for e in interactions(arrows["events"]) if e["name"] == "keydown"]),
                         "worst": sorted(interactions(arrows["events"]), key=lambda e: -e["duration"])[:3]},
        "enterWorldKey": {"wallSeconds": entry_s, "events": sorted(entry_events, key=lambda e: -e["duration"])[:3],
                          "longAnimationFrames": sorted(entry["loaf"], key=lambda e: -e["duration"])[:3],
                          "cpuProfile": profile},
    }


def key_to_motion(session, taps=12):
    """W taps from standstill: keydown timestamp → end of the first frame whose drawn viewer moved."""
    page = session.page
    results = []
    for _ in range(taps):
        session.frames(30)
        session.take()
        page.keyboard.down("KeyW")
        session.frames(20)
        page.keyboard.up("KeyW")
        sample = session.take()
        keys = [k for k in sample["keys"] if k["code"] == "KeyW" and not k["repeat"]]
        frames = sample["frames"]
        if not keys or not frames:
            continue
        t = keys[0]["t"]
        before = [f for f in frames if f["raf"] < t]
        after = [f for f in frames if f["start"] >= t]
        if not before or not after:
            continue
        origin = before[-1]["viewer"]
        moved = next((f for f in after if abs(f["viewer"][0] - origin[0]) + abs(f["viewer"][1] - origin[1]) > 1e-4), None)
        event = next((e for e in sample["events"] if e["name"] == "keydown"), None)
        if moved is None:
            continue
        index = frames.index(moved)
        following = frames[index + 1] if index + 1 < len(frames) else None
        results.append({
            "toFirstMovedFrameEndMs": round(moved["end"] - t, 1),
            "framesUntilMoved": after.index(moved) + 1,
            "ticksBeforeMoved": sum(f["ticks"] for f in after[: after.index(moved) + 1]),
            # The moved frame is presented no earlier than the next frame's begin; an estimate, not an observed present.
            "toNextFrameBeginMs": round(following["raf"] - t, 1) if following else None,
            "eventTimingDurationMs": round(event["duration"], 1) if event else None,
        })
    return {"taps": len(results), "toFirstMovedFrameEndMs": dist([r["toFirstMovedFrameEndMs"] for r in results]),
            "toNextFrameBeginMs": dist([r["toNextFrameBeginMs"] for r in results if r["toNextFrameBeginMs"] is not None]),
            "eventTimingDurationMs": dist([r["eventTimingDurationMs"] for r in results if r["eventTimingDurationMs"] is not None]),
            "framesUntilMoved": dist([r["framesUntilMoved"] for r in results]),
            "ticksBeforeMoved": dist([r["ticksBeforeMoved"] for r in results])}


def move_rotate(session, seconds, trace=None, browser=None):
    page = session.page
    box = page.locator("#world").bounding_box()
    x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
    session.take()
    if trace:
        browser.start_tracing(page=page, path=str(trace), categories=TRACE_CATEGORIES)
    started = time.monotonic()
    page.keyboard.down("KeyW")
    page.mouse.move(x, y)
    page.mouse.down()
    steps = int(seconds * 30)
    for step in range(steps):
        page.mouse.move(x + 200 * ((step % 60) / 60 - 0.5), y, steps=1)
        page.wait_for_timeout(33)
    page.mouse.up()
    page.keyboard.up("KeyW")
    elapsed = time.monotonic() - started
    sample = session.take()
    if trace:
        browser.stop_tracing()
    return summarize(sample, elapsed)


def view(session, name, seconds):
    if isinstance(name, tuple):
        session.page.evaluate("([eye, target]) => window.__valeDebug.flyToPose(eye, target)", list(name))
    else:
        session.page.evaluate("name => window.__valeDebug.flyTo(name)", name)
    session.frames(30)
    session.page.wait_for_timeout(1500)
    result = summarize(session.record(seconds), seconds)
    session.page.evaluate("window.__valeDebug.follow()")
    return result


def cpu_profile(session, action):
    """A V8 sampling profile while `action()` runs: self and inclusive time per function."""
    cdp = session.context.new_cdp_session(session.page)
    cdp.send("Profiler.enable")
    cdp.send("Profiler.setSamplingInterval", {"interval": 200})
    cdp.send("Profiler.start")
    action()
    profile = cdp.send("Profiler.stop")["profile"]
    cdp.detach()
    nodes = {node["id"]: node for node in profile["nodes"]}
    parent = {child: node["id"] for node in profile["nodes"] for child in node.get("children", [])}

    def label(node):
        frame = node["callFrame"]
        url = frame["url"].rsplit("/", 1)[-1]
        return f'{frame["functionName"] or "(anonymous)"} {url}:{frame["lineNumber"] + 1}' if url else frame["functionName"]

    total = sum(profile["timeDeltas"]) / 1000
    own, inclusive = {}, {}
    for sample, delta in zip(profile["samples"], profile["timeDeltas"]):
        ms = delta / 1000
        own[label(nodes[sample])] = own.get(label(nodes[sample]), 0) + ms
        seen, current = set(), sample
        while current is not None:
            name = label(nodes[current])
            if name not in seen:
                seen.add(name)
                inclusive[name] = inclusive.get(name, 0) + ms
            current = parent.get(current)

    def top(table):
        return [{"function": k, "percent": round(100 * v / total, 1)} for k, v in sorted(table.items(), key=lambda kv: -kv[1])[:25]]
    return {"sampledMs": round(total), "self": top(own), "inclusive": top(inclusive)}


def analyze_trace(path):
    """Thread busy time and the longest renderer-main tasks inside the traced window."""
    data = json.loads(Path(path).read_text())
    events = data["traceEvents"] if isinstance(data, dict) else data
    names = {}
    for e in events:
        if e.get("ph") == "M" and e.get("name") == "thread_name":
            names[(e["pid"], e["tid"])] = e["args"]["name"]
    complete = [e for e in events if e.get("ph") == "X" and "dur" in e]
    if not complete:
        return {"error": "no complete events"}
    start = min(e["ts"] for e in complete)
    end = max(e["ts"] + e["dur"] for e in complete)
    window = (end - start) / 1000
    intervals, gc, longest = {}, {}, []
    for e in complete:
        thread = names.get((e["pid"], e["tid"]), "?")
        if e.get("cat") == "toplevel":
            intervals.setdefault(thread, []).append((e["ts"], e["ts"] + e["dur"]))
            if thread == "CrRendererMain":
                longest.append(e)
        if "GC" in e["name"] and thread == "CrRendererMain" and e["name"] in ("MajorGC", "MinorGC", "V8.GC_SCAVENGER", "V8.GC_MARK_COMPACTOR", "BlinkGC.AtomicPhase"):
            gc[e["name"]] = gc.get(e["name"], 0) + e["dur"] / 1000
    busy = {}
    for thread, spans in intervals.items():
        # Nested top-level tasks count once: the union of their spans.
        total, reach = 0, None
        for begin, finish in sorted(spans):
            if reach is None or begin > reach:
                total += finish - begin
                reach = finish
            elif finish > reach:
                total += finish - reach
                reach = finish
        busy[thread] = total / 1000
    renderer_main = [e for e in complete if names.get((e["pid"], e["tid"])) == "CrRendererMain"]
    tops = []
    outermost, reach = [], None
    for task in sorted(longest, key=lambda e: (e["ts"], -e["dur"])):
        if reach is None or task["ts"] >= reach:
            outermost.append(task)
            reach = task["ts"] + task["dur"]
    for task in sorted(outermost, key=lambda e: -e["dur"])[:5]:
        wrappers = {"ThreadControllerImpl::RunTask", "ProxyMain::BeginMainFrame", "FireAnimationFrame", "v8.callFunction", "FunctionCall", "RunTask"}
        inner = [c for c in renderer_main if c is not task and c["name"] not in wrappers
                 and c["ts"] >= task["ts"] and c["ts"] + c["dur"] <= task["ts"] + task["dur"]]
        by_name = {}
        for c in inner:
            by_name[c["name"]] = by_name.get(c["name"], 0) + c["dur"] / 1000
        tops.append({"ms": round(task["dur"] / 1000, 2),
                     "eventsByName": {k: round(v, 1) for k, v in sorted(by_name.items(), key=lambda kv: -kv[1])[:8]}})
    # Synchronous round trips to the GPU process (e.g. three.js reading a program's link status).
    waits = [e for e in renderer_main if e["name"] == "ImplementationBase::WaitForCmd"]
    return {
        "rendererMainSyncGpuWaits": {"count": len(waits), "ms": round(sum(e["dur"] for e in waits) / 1000, 1),
                                     "maxMs": round(max((e["dur"] for e in waits), default=0) / 1000, 1)},
        "windowMs": round(window),
        "busyPercent": {thread: round(100 * ms / window, 1) for thread, ms in sorted(busy.items(), key=lambda kv: -kv[1])[:8]},
        "rendererMainGcMs": {k: round(v, 1) for k, v in gc.items()},
        "longestRendererMainTasks": tops,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, help="write the JSON report here as well as stdout")
    parser.add_argument("--trace", type=Path, help="directory for the move-rotate Chromium trace")
    parser.add_argument("--seconds", type=float, default=6.0)
    parser.add_argument("--channel", help="Playwright browser channel, e.g. chrome")
    parser.add_argument("--headed", action="store_true")
    parser.add_argument("--no-sweep", action="store_true", help="skip the viewport/pixel-ratio sweep")
    parser.add_argument("--activity", choices=["battle-royale"],
                        help="profile the Battle Royale practice arena (#552) instead of Greyhaven")
    args = parser.parse_args()
    # The arena's overview: the whole fortress from above its south side.
    overview = ((0, 70, -150), (0, 0, 0)) if args.activity else "vale"

    dist_dir = ROOT / "web" / "dist"
    if not (dist_dir / "index.html").is_file():
        raise SystemExit("Build the production web bundle first (cd web && bun run build).")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(dist_dir)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_port}/mmorpg/"
    # Other work on the machine inflates every CPU phase alike; the load is part of the record.
    report = {"source": "scripts/profile-browser.py", "cpus": os.cpu_count(), "loadAverageAtStart": [round(x, 1) for x in os.getloadavg()],
              "scenarios": {}, "sweep": []}
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(headless=not args.headed, channel=args.channel, args=GPU_ARGS)
        report["browser"] = {"version": browser.version, "channel": args.channel or "playwright-chromium",
                             "headless": not args.headed, "args": GPU_ARGS}
        report["activity"] = args.activity or "greyhaven"
        session = Session(browser, url, 1440, 900, 1, args.activity)
        report["coldSelectionSeconds"] = session.cold_selection_s
        session.frames(60)
        if args.activity:
            session.enter_world()
            session.frames(10)
        else:
            report["scenarios"]["selection-keys"] = selection_keys(session)
        report["display"] = {"viewport": [1440, 900], "deviceScaleFactor": 1, **session.renderer()}
        session.warm()
        report["scenarios"]["idle"] = summarize(session.record(args.seconds), args.seconds)
        report["scenarios"]["idle"]["cpuProfile"] = cpu_profile(session, lambda: session.page.wait_for_timeout(args.seconds * 1000))
        trace = None
        if args.trace:
            args.trace.mkdir(parents=True, exist_ok=True)
            trace = args.trace / "move-rotate.trace.json"
        report["scenarios"]["move-rotate"] = move_rotate(session, args.seconds, trace, browser)
        if trace:
            report["scenarios"]["move-rotate"]["trace"] = {"path": str(trace), **analyze_trace(trace)}
        report["scenarios"]["key-to-motion"] = key_to_motion(session)
        views = [("overview", overview)] if args.activity else [("dense-woods", "woods"), ("dense-hub", "hub"), ("overview", overview)]
        for name, viewpoint in views:
            report["scenarios"][name] = view(session, viewpoint, args.seconds)
        report["pageErrors"] = session.errors
        session.close()
        if not args.no_sweep:
            for width, height, scale in SWEEP:
                sweep = Session(browser, url, width, height, scale, args.activity)
                sweep.enter_world()
                sweep.warm()
                idle = summarize(sweep.record(args.seconds / 2), args.seconds / 2)
                seen = view(sweep, overview, args.seconds / 2)
                report["sweep"].append({"viewport": [width, height], "deviceScaleFactor": scale, **sweep.renderer(),
                                        "idle": {k: idle[k] for k in ("fps", "rafIntervalMs", "cpuFrameMs", "gpuMs")},
                                        "overview": {k: seen[k] for k in ("fps", "rafIntervalMs", "cpuFrameMs", "gpuMs", "drawCallsPerFrame")},
                                        "pageErrors": sweep.errors})
                sweep.close()
        browser.close()
    report["loadAverageAtEnd"] = [round(x, 1) for x in os.getloadavg()]
    server.shutdown()
    text = json.dumps(report, indent=2)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text)
    print(text)


if __name__ == "__main__":
    main()
