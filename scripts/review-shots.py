#!/usr/bin/env python3
# /// script
# dependencies = ["playwright"]
# ///
"""Browser review shots of the current build at named poses (#864).

Serves the production bundle (`web/dist`), opens the Greyhaven Downs review scenario
(`/mmorpg/?scenario=greyhaven-downs&debug`) and saves one PNG per pose in
`scripts/review-poses.json` (or `--poses`), plus `report.json`: the time from navigation to the
HUD, the grass and ambient-animal observations, each shot's resolved pose and the page errors.

A pose is one of:
- `{"eye": [x, y, z], "target": [x, y, z]}`: a fixed camera in metres (`flyToPose`);
- `{"ambient": n, "offset": [dx, dy, dz]}`: looks at ambient animal `n`'s feet from that offset;
- `{"player": true}`: the default third-person camera behind the character.

Prerequisites: build web/ (bun run build). Run from the repository root:
    uv run --with playwright scripts/review-shots.py [--out artifacts/review] [--only NAME ...]
Defaults to SwiftShader, which runs anywhere; `--gpu` renders on the host GPU instead (ANGLE
Vulkan, full Chromium), much faster where a GPU is available.
"""
from __future__ import annotations

import argparse
import functools
import http.server
import json
import threading
import time
from pathlib import Path

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
# ANGLE over Vulkan reaches the host GPU headless; ANGLE OpenGL falls back to SwiftShader without a display.
GPU_ARGS = ["--enable-gpu", "--use-angle=vulkan", "--enable-features=Vulkan", "--ignore-gpu-blocklist"]
SOFTWARE_ARGS = ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"]
# Frames per shot before the capture: level-of-detail cross-fades and streamed art settle.
SETTLE_FRAMES = 60


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/mmorpg/"):
            self.path = self.path[len("/mmorpg"):]
        super().do_GET()

    def log_message(self, *_args):
        pass


def frames(page, count):
    page.evaluate("""count => new Promise(resolve => {
      function next() { if (--count <= 0) resolve(); else requestAnimationFrame(next); }
      requestAnimationFrame(next);
    })""", count)


def pose_camera(page, pose):
    """Points the camera for one pose and returns the eye and target used, or None for the player view."""
    if pose.get("player"):
        page.evaluate("window.__valeDebug.follow()")
        return None
    if "ambient" in pose:
        page.wait_for_function("n => (window.__valeDebug.ambient()?.animals.length ?? 0) > n", arg=pose["ambient"],
                               timeout=60_000)
        feet = page.evaluate("n => window.__valeDebug.ambient().animals[n].feet", pose["ambient"])
        target = feet
        eye = [a + b for a, b in zip(feet, pose.get("offset", [5, 2.5, 5]))]
    else:
        eye, target = pose["eye"], pose["target"]
    page.evaluate("([eye, target]) => window.__valeDebug.flyToPose(eye, target)", [eye, target])
    return {"eye": eye, "target": target}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--poses", type=Path, default=ROOT / "scripts" / "review-poses.json")
    parser.add_argument("--out", type=Path, default=ROOT / "artifacts" / "review")
    parser.add_argument("--only", action="append", help="capture only these pose names")
    parser.add_argument("--gpu", action="store_true", help="render on the host GPU instead of SwiftShader")
    parser.add_argument("--channel", help="Playwright browser channel, e.g. chrome")
    args = parser.parse_args()
    manifest = json.loads(args.poses.read_text())
    width, height = manifest["size"]
    poses = [pose for pose in manifest["poses"] if not args.only or pose["name"] in args.only]

    dist_dir = ROOT / "web" / "dist"
    if not (dist_dir / "index.html").is_file():
        raise SystemExit("Build the production web bundle first (cd web && bun run build).")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(dist_dir)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    args.out.mkdir(parents=True, exist_ok=True)
    report = {"poses": []}
    with sync_playwright() as playwright:
        # Full Chromium in new headless mode: the headless shell does not reach the GPU.
        channel = args.channel or ("chromium" if args.gpu else None)
        browser = playwright.chromium.launch(channel=channel, args=GPU_ARGS if args.gpu else SOFTWARE_ARGS)
        context = browser.new_context(viewport={"width": width, "height": height}, device_scale_factor=1)
        page = context.new_page()
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        started = time.monotonic()
        page.goto(f"http://127.0.0.1:{server.server_port}/mmorpg/?scenario=greyhaven-downs&debug")
        expect(page.locator(".hud").first).to_be_visible(timeout=180_000)
        report["hudVisibleSeconds"] = round(time.monotonic() - started, 2)
        print(f"HUD visible after {report['hudVisibleSeconds']} s")
        page.evaluate("window.__valeDebug.fetchAllArt()")
        # Streamed art (kits, nature, ambient animals) arrives after the HUD.
        page.wait_for_function("window.__valeDebug.ambient()?.loaded === true", timeout=120_000)
        page.wait_for_timeout(5_000)
        for pose in poses:
            used = pose_camera(page, pose)
            frames(page, SETTLE_FRAMES)
            path = args.out / f"{pose['name']}.png"
            page.screenshot(path=str(path))
            report["poses"].append({"name": pose["name"], "note": pose.get("note"), "camera": used,
                                    "file": path.name})
            print(f"saved {path}")
        page.evaluate("window.__valeDebug.follow()")
        frames(page, 2)
        report["grass"] = page.evaluate("window.__valeDebug.grass()")
        ambient = page.evaluate("window.__valeDebug.ambient()")
        if ambient:
            clips = {}
            for animal in ambient["animals"]:
                clips[animal["clip"]] = clips.get(animal["clip"], 0) + 1
            report["ambient"] = {"loaded": ambient["loaded"], "count": len(ambient["animals"]), "clips": clips,
                                 "animals": ambient["animals"]}
        else:
            report["ambient"] = None
        report["pageErrors"] = errors
        report["browser"] = browser.version
        browser.close()
    server.shutdown()
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {args.out / 'report.json'}; {len(errors)} page error(s)")
    if errors:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
