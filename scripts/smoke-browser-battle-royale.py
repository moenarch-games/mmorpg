#!/usr/bin/env python3
"""One real two-browser Battle Royale fight, result and between-life edit/next match.

Build web/dist and pass the pinned battle-royale-host executable with --host.
Reuses the online smoke's certificate, disposable ports and static-file handler.
"""
from __future__ import annotations

import argparse
from contextlib import ExitStack
import functools
import http.server
import json
import os
from pathlib import Path
import runpy
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts" / "smoke-browser-online.py"))
ARTIFACTS = ROOT / "artifacts" / "browser-battle-royale"
SOURCE = "window.__valeDebug.worldSource()"
# Signed yaw units (65536 per turn) from the viewer's facing to its selected target.
AIM_ERROR = f"""(() => {{
  const p = {SOURCE}.latestProjection();
  const self = p.entities.find((e) => e.entityId === p.viewerId);
  const target = p.entities.find((e) => e.entityId === p.viewer.target.id);
  const bearing = Math.atan2(target.position[0] - self.position[0], target.position[2] - self.position[2]) / (2 * Math.PI) * 65536;
  return ((Math.round(bearing - self.facing) % 65536) + 98304) % 65536 - 32768;
}})()"""


def face_target(page):
    """Turns the character toward its target with a right drag, as a player does (#517 free aim)."""
    page.mouse.move(800, 500)
    page.mouse.down(button="right")
    try:
        x = 800.0
        units_per_pixel = None
        for _ in range(30):
            error = page.evaluate(AIM_ERROR)
            if abs(error) < 1200:
                return
            if units_per_pixel is None:
                step = 40.0
            else:
                step = max(-300.0, min(300.0, error / units_per_pixel))
            before = error
            x += step
            page.mouse.move(x, 500, steps=4)
            page.wait_for_timeout(150)
            turned = before - page.evaluate(AIM_ERROR)
            if abs(turned) > 50:
                units_per_pixel = turned / step
        raise AssertionError(f"could not face the target: {page.evaluate(AIM_ERROR)} yaw units off")
    finally:
        page.mouse.up(button="right")


def run(host_binary: Path):
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    with ExitStack() as cleanup:
        directory = cleanup.enter_context(tempfile.TemporaryDirectory(prefix="mmorpg-arena-smoke-"))
        certificate, key, certificate_hash = HELPERS["development_certificate"](Path(directory))
        transport_port = HELPERS["free_port"](socket.SOCK_DGRAM)
        status_port = HELPERS["free_port"](socket.SOCK_STREAM)
        environment = dict(os.environ)
        environment.pop("BATTLE_ROYALE_RECOVERY_DIR", None)
        environment.update(BATTLE_ROYALE_MATCH_IDS="1,2", BATTLE_ROYALE_PORT=str(transport_port),
                           BATTLE_ROYALE_STATUS_PORT=str(status_port), BATTLE_ROYALE_CERT_PEM=str(certificate),
                           BATTLE_ROYALE_KEY_PEM=str(key), BATTLE_ROYALE_ROUTE_PREFIX="/game",
                           BATTLE_ROYALE_RECONNECT_GRACE_TICKS="300", BATTLE_ROYALE_DRAIN_GRACE_MS="0")
        log = cleanup.enter_context((ARTIFACTS / "host.log").open("w"))
        host = subprocess.Popen([str(host_binary.resolve())], env=environment, stdout=log, stderr=subprocess.STDOUT)
        def stop():
            if host.poll() is None:
                host.terminate()
                try:
                    host.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    host.kill()
                    host.wait()
        cleanup.callback(stop)
        deadline = time.monotonic() + 30
        while True:
            if host.poll() is not None:
                raise RuntimeError("Battle Royale host exited; see artifacts/browser-battle-royale/host.log")
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{status_port}/readyz", timeout=1):
                    break
            except (urllib.error.URLError, OSError):
                if time.monotonic() > deadline:
                    raise RuntimeError("Battle Royale host did not become ready") from None
                time.sleep(.1)
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0),
            functools.partial(HELPERS["Handler"], directory=str(ROOT / "web" / "dist")))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        cleanup.callback(lambda: (server.shutdown(), server.server_close(), thread.join()))
        route = f"https://127.0.0.1:{transport_port}/game/matches/br-0000000000000001"
        url = f"http://127.0.0.1:{server.server_port}/mmorpg/?" + urllib.parse.urlencode(
            {"debug": "", "activity": "battle-royale", "arenaServer": route, "certHash": certificate_hash})
        playwright = cleanup.enter_context(sync_playwright())
        browser = playwright.chromium.launch(args=["--use-angle=swiftshader", "--enable-unsafe-swiftshader"])
        cleanup.callback(browser.close)
        pages = []
        errors = []
        for _ in range(2):
            context = browser.new_context(viewport={"width": 1600, "height": 1000}, reduced_motion="no-preference")
            page = context.new_page()
            page.on("pageerror", lambda error: errors.append(str(error)))
            page.goto(url)
            expect(page.locator("#enter-world")).to_be_enabled(timeout=30_000)
            pages.append(page)
        first, second = pages
        first.locator("#arena-class").select_option("arcanist")
        expect(first.locator("#character-subtitle")).to_contain_text("Arcanist")
        for page in pages:
            page.locator("#enter-world").click()
            expect(page.locator("#character-select")).to_be_hidden(timeout=15_000)
        first.wait_for_function(f"{SOURCE}.match?.phase === 'combat'", timeout=20_000)
        first.wait_for_function(f"{SOURCE}.latestProjection()?.viewer.classChoice?.classId === 'arcanist'")
        for page in pages:
            expect(page.locator("#arena-class")).to_be_disabled()
        first.locator("#world").focus()
        first.keyboard.press("Tab")
        first.wait_for_function(f"{SOURCE}.latestProjection()?.viewer.target?.kind === 'player'")
        initial = second.evaluate(f"{SOURCE}.latestProjection().viewer.health")
        first.mouse.move(1000, 600)
        first.mouse.wheel(0, 600)
        face_target(first)
        first.wait_for_function(f"{SOURCE}.latestProjection()?.viewer.target?.kind === 'player'")
        first.keyboard.press("1")
        second.wait_for_function(f"{SOURCE}.latestProjection()?.viewer.health < {initial}", timeout=10_000)
        first.screenshot(path=str(ARTIFACTS / "pvp-hit.png"))
        for _ in range(12):
            if first.evaluate(f"{SOURCE}.match.phase === 'finished'"):
                break
            previous = second.evaluate(f"{SOURCE}.latestProjection().viewer.health")
            first.keyboard.press("1")
            second.wait_for_function(f"{SOURCE}.latestProjection()?.viewer.health < {previous}", timeout=10_000)
        expect(first.locator("#battle-royale-result")).to_have_text("Victory", timeout=10_000)
        second.wait_for_function(f"{SOURCE}.latestProjection()?.viewer.dead === true")
        winner = first.evaluate(f"{SOURCE}.match.winner")
        assert second.evaluate(f"{SOURCE}.match.winner") == winner
        for index, page in enumerate(pages):
            try:
                page.locator("#battle-royale-next").click(timeout=5000)
            except Exception:
                print(json.dumps({"failedNextPage": index, "entryNote": page.locator("#enter-world-note").text_content(),
                                  "selectionVisible": page.locator("#character-select").is_visible(),
                                  "resultVisible": page.locator("#battle-royale-status").is_visible()}), flush=True)
                raise
            expect(page.locator("#character-select")).to_be_visible()
            expect(page.locator("#arena-server")).to_have_value(route[:-1] + "2")
        first.locator("#arena-class").select_option("ranger")
        first.locator("#arena-slot-1").select_option("0")
        for page in pages:
            page.locator("#enter-world").click()
            expect(page.locator("#character-select")).to_be_hidden(timeout=15_000)
        first.wait_for_function(f"{SOURCE}.match?.matchId === 2n && {SOURCE}.latestProjection()?.viewer.classChoice?.classId === 'ranger'")
        first.wait_for_function(f"{SOURCE}.latestProjection()?.equipment?.[1] === null")
        assert not errors, errors
        receipt = {"realBrowsers": 2, "firstMatchWinner": winner, "secondMatch": 2,
                   "classChanged": "arcanist -> ranger", "offHandChanged": "empty", "screenshot": "pvp-hit.png"}
        (ARTIFACTS / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(json.dumps(receipt))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True, help="built battle-royale-host at the documented compatible pin")
    args = parser.parse_args()
    run(args.host)
