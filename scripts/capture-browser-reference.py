#!/usr/bin/env python3
"""Browser captures of the native Greyhaven reference shots (render-engine#74).

The native test `greyhaven_lighting_shots` writes `re9-<name>.png` and `views.json`, each shot's
eye, target, field of view and zone hour. This script poses the browser's camera
(`window.__valeDebug.flyToPose`) and sky clock (`setPresentationTime`) the same way and saves the
world canvas alone, with the HUD hidden, as `<name>.web.png` at the native size and pixel ratio 1.
Like the native test it captures with reduced motion (flames' flicker, reeds and idle poses held,
levels of detail switched at once) and the default character (a male Warden), and it holds the
zone (`holdWorld`) once entered, so the shots differ only in rendering (#753).
Compare the pairs with FLIP (render-engine's `oracle --flip-images`); the numbers are records,
not a gate.

Prerequisites: build web/ (bun run build) and Playwright for Python (see smoke-browser.py).
Run from the repository root:
    python scripts/capture-browser-reference.py docs/reviews/greyhaven-reference/views.json \\
        --out artifacts/greyhaven-reference
"""
from __future__ import annotations

import argparse
import functools
import http.server
import json
import re
import threading
from pathlib import Path

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
GPU_ARGS = ["--enable-gpu", "--use-angle=gl", "--ignore-gpu-blocklist"]
# web/src/world/environment.ts: one zone day, and presentation time 0 is noon.
DAY_SECONDS = 20 * 60
# Everything but the world canvas is hidden, so the capture is the rendered frame alone.
WORLD_ONLY = "* { visibility: hidden !important; } #world { visibility: visible !important; }"
# Frames per shot before the capture: level-of-detail cross-fades and the sky settle.
SETTLE_FRAMES = 90


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/mmorpg/"):
            self.path = self.path[len("/mmorpg"):]
        super().do_GET()

    def log_message(self, *_args):
        pass


def presentation_seconds(hour: float) -> float:
    return (hour - 12.0) % 24.0 / 24.0 * DAY_SECONDS


def frames(page, count):
    page.evaluate("""count => new Promise(resolve => {
      function next() { if (--count <= 0) resolve(); else requestAnimationFrame(next); }
      requestAnimationFrame(next);
    })""", count)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("views", type=Path, help="views.json from greyhaven_lighting_shots")
    parser.add_argument("--out", type=Path, required=True, help="directory for <name>.web.png")
    parser.add_argument("--only", action="append", help="capture only these shot names")
    parser.add_argument("--channel", help="Playwright browser channel, e.g. chrome")
    args = parser.parse_args()
    manifest = json.loads(args.views.read_text())
    width, height = manifest["size"]
    shots = [shot for shot in manifest["shots"] if not args.only or shot["name"] in args.only]

    dist_dir = ROOT / "web" / "dist"
    if not (dist_dir / "index.html").is_file():
        raise SystemExit("Build the production web bundle first (cd web && bun run build).")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(dist_dir)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    args.out.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(channel=args.channel, args=GPU_ARGS)
        context = browser.new_context(viewport={"width": width, "height": height}, device_scale_factor=1,
                                      reduced_motion="reduce")
        page = context.new_page()
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto(f"http://127.0.0.1:{server.server_port}/mmorpg/?debug")
        enter = page.get_by_role("button", name=re.compile("^Enter World"))
        expect(enter).to_be_enabled(timeout=60_000)
        enter.focus()
        page.keyboard.press("Enter")
        expect(page.locator("#character-select")).to_be_hidden(timeout=60_000)
        page.evaluate("window.__valeDebug.holdWorld(true)")
        page.evaluate("window.__valeDebug.fetchAllArt()")
        page.add_style_tag(content=WORLD_ONLY)
        box = page.locator("#world").bounding_box()
        if [round(box["width"]), round(box["height"])] != [width, height]:
            raise SystemExit(f"The world canvas is {box['width']}x{box['height']}, not {width}x{height}.")
        # Let every area's art arrive before the first shot.
        page.wait_for_timeout(10_000)
        for shot in shots:
            page.evaluate("([eye, target]) => window.__valeDebug.flyToPose(eye, target)", [shot["eye"], shot["target"]])
            page.evaluate("seconds => window.__valeDebug.setPresentationTime(seconds)", presentation_seconds(shot["hour"]))
            frames(page, SETTLE_FRAMES)
            page.screenshot(path=str(args.out / f"{shot['name']}.web.png"), clip=box, animations="allow")
            print(f"saved {shot['name']}.web.png (hour {shot['hour']:.2f})")
        renderer = page.evaluate("""() => { const gl = document.querySelector('#world').getContext('webgl2');
          const d = gl.getExtension('WEBGL_debug_renderer_info');
          return d ? gl.getParameter(d.UNMASKED_RENDERER_WEBGL) : null; }""")
        print(json.dumps({"browser": browser.version, "renderer": renderer, "pageErrors": errors}))
        browser.close()
    server.shutdown()


if __name__ == "__main__":
    main()
