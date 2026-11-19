#!/usr/bin/env python3
"""Records one scripted Practice session per #680 netcode trial cell (3 options x 50/100/150 ms).

Serves the built web/dist, opens `?activity=battle-royale&netcode=<option>&delay=<ms>` in Chromium
on the host GPU, targets the sparring partner (Tab, which turns the character toward it) and plays
the same timed keyboard sequence in every cell: running strafes with frequent direction changes,
swings at the moving partner, standing trades timed to the partner's 2.5 s swing rhythm to provoke
clashes, backing off and closing in again, and swings at the edge of reach while closing.
Each video carries a burned-in caption with the option and delay.

    uv run --with playwright docs/reviews/netcode-trial-680/record.py --out ~/moritzbrantner/netcode-trial-680

These recordings are a scripted bot, not the owner's playtest (#680 decision A).
"""
import argparse
import functools
import http.server
import re
import threading
from pathlib import Path

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[3]
OPTIONS = [("baseline", "1 · Baseline (interpolated)"), ("predicted", "2 · Predicted movement"),
           ("rewind", "3 · Predicted + hit rewind")]
DELAYS = [50, 100, 150]
GPU_ARGS = ["--enable-gpu", "--use-angle=vulkan", "--enable-features=Vulkan", "--ignore-gpu-blocklist"]
SIZE = {"width": 1280, "height": 720}


def timeline():
    """(seconds after match start, action, key) for the whole session; the same in every cell."""
    events = []

    def hold(start, length, *keys):
        for key in keys:
            events.append((start, "down", key))
            events.append((start + length, "up", key))

    def swing(at):
        """A basic strike (Q): builds Rage."""
        events.append((at, "press", "KeyQ"))

    def heroic(at):
        """A directed Heroic Strike (hold G, release): the timed swing that clashes and rewinds."""
        hold(at, 0.15, "KeyG")

    # A short settle after targeting.
    for cycle in range(4):
        base = 1.0 + cycle * 12.0
        # Running strafes around the partner, changing direction often, swinging while moving.
        t = base
        for length, key in [(0.7, "KeyD"), (0.6, "KeyA"), (0.5, "KeyD"), (0.8, "KeyA"), (0.6, "KeyD"), (0.8, "KeyA")]:
            hold(t, length, key)
            t += length
        for k in range(6):
            swing(base + 0.33 + 0.75 * k)
        # Diagonal runs: forward-right, then back-left.
        hold(base + 4.0, 1.0, "KeyW", "KeyD")
        hold(base + 5.0, 1.0, "KeyS", "KeyA")
        # Standing trades (the partner swings every 2.5 s once it has Rage) to provoke clashes.
        heroic(base + 6.5)
        # Back off, then close in and swing at the edge of reach while still moving.
        hold(base + 8.5, 1.0, "KeyS")
        hold(base + 9.5, 0.9, "KeyW")
        heroic(base + 10.1)
        hold(base + 10.4, 1.4, "KeyD")
    return sorted(events)


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/mmorpg/"):
            self.path = self.path[len("/mmorpg"):]
        super().do_GET()

    def log_message(self, *_args):
        pass


CAPTION = """(text) => {
  const caption = document.createElement('div');
  caption.textContent = text;
  caption.style.cssText = 'position:fixed;top:8px;left:50%;transform:translateX(-50%);z-index:2147483647;'
    + 'background:rgba(0,0,0,.72);color:#ffd866;font:600 18px system-ui,sans-serif;padding:6px 14px;'
    + 'border-radius:6px;pointer-events:none';
  document.body.appendChild(caption);
}"""


def record(browser, url, out, option, label, delay, log):
    context = browser.new_context(viewport=SIZE, record_video_dir=str(out / "raw"), record_video_size=SIZE)
    page = context.new_page()
    page.goto(f"{url}?activity=battle-royale&netcode={option}&delay={delay}")
    start = page.get_by_role("button", name=re.compile("^Start practice"))
    expect(start).to_be_enabled(timeout=60_000)
    start.click()
    expect(page.locator("#character-select")).to_be_hidden(timeout=60_000)
    page.evaluate(CAPTION, f"#680 netcode trial · {label} · {delay} ms simulated round trip · scripted bot session")
    page.locator("#world").focus()
    # Target the sparring partner once it has closed in. Tab cycles only the Training Targets
    # ahead, so click around the character until the status names another target, which also
    # turns the camera and character toward it (focused combat).
    page.wait_for_timeout(2500)
    def partner_selected():
        status = page.locator("#unit-status").text_content() or ""
        return "Target:" in status and "Training Target" not in status
    for y in range(640, 300, -40):
        for x in range(160, 1121, 60):
            page.mouse.click(x, y)
            page.wait_for_timeout(60)
            if partner_selected():
                break
        if partner_selected():
            break
    page.mouse.move(640, 200)
    # Draw the weapon (H): Q and G are combat bindings, idle in Exploration.
    page.keyboard.press("KeyH")
    page.wait_for_timeout(500)
    began = page.evaluate("performance.now()")
    log.append(f"{option} {delay}: {(page.locator('#unit-status').text_content() or '').strip()[:160]}")
    for at, action, key in timeline():
        wait = at * 1000 - (page.evaluate("performance.now()") - began)
        if wait > 0:
            page.wait_for_timeout(wait)
        getattr(page.keyboard, action)(key)
    page.wait_for_timeout(1500)
    log.append(f"  end: {(page.locator('#unit-status').text_content() or '').strip()[:160]}")
    page.screenshot(path=str(out / f"trial-{option}-{delay}ms-end.png"))
    video = page.video
    context.close()
    target = out / f"trial-{option}-{delay}ms.webm"
    video.save_as(str(target))
    video.delete()
    return target


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cell", action="append", help="only option:delay, e.g. rewind:150 (repeatable)")
    parser.add_argument("--swiftshader", action="store_true", help="software rendering instead of the host GPU")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    dist = ROOT / "web" / "dist"
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(dist)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_port}/mmorpg/"
    log = []
    with sync_playwright() as playwright:
        launch_args = ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] if args.swiftshader else GPU_ARGS
        browser = playwright.chromium.launch(args=launch_args)
        page = browser.new_page()
        page.goto("about:blank")
        log.append("renderer: " + (page.evaluate("""() => { const c = document.createElement('canvas').getContext('webgl2');
          const e = c && c.getExtension('WEBGL_debug_renderer_info'); return e ? c.getParameter(e.UNMASKED_RENDERER_WEBGL) : 'none'; }""")))
        page.close()
        for option, label in OPTIONS:
            for delay in DELAYS:
                if args.cell and f"{option}:{delay}" not in args.cell:
                    continue
                print("recorded", record(browser, url, args.out, option, label, delay, log), flush=True)
        browser.close()
    server.shutdown()
    (args.out / "session-log.txt").write_text("\n".join(log) + "\n")
    print("\n".join(log))


if __name__ == "__main__":
    main()
