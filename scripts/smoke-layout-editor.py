#!/usr/bin/env python3
"""Focused edit/save/reopen/play journey. Requires built WASM, Bun and Playwright.

Runs a local authoring server, restores the original layout and rebuilds WASM on exit.
Screenshots land in artifacts/layout-editor/. Never run against another active editor.
"""
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from urllib.request import urlopen

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
LAYOUT = ROOT / "content/greyhaven/outpost-layout.json"
ARTIFACTS = ROOT / "artifacts/layout-editor"


def main():
    original = LAYOUT.read_text()
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    url = f"http://127.0.0.1:{port}"
    environment = dict(os.environ, MMORPG_LAYOUT_EDITOR="1")
    log = (ARTIFACTS / "server.log").open("w")
    server = subprocess.Popen(["bun", "--bun", "node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", str(port)],
                              cwd=ROOT / "web", env=environment, stdout=log, stderr=log)
    try:
        for _ in range(150):
            try:
                with urlopen(url + "/__layout", timeout=1) as response:
                    assert json.load(response)["source"] == original
                break
            except OSError:
                if server.poll() is not None:
                    raise RuntimeError("Editor server failed; inspect artifacts/layout-editor/server.log")
                time.sleep(.2)
        else:
            raise RuntimeError("Editor server did not become ready")
        with sync_playwright() as playwright:
            arguments = ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"]
            if os.environ.get("MMORPG_BROWSER_SINGLE_PROCESS"):
                arguments += ["--single-process", "--no-zygote"]
            options = {"args": arguments}
            if os.environ.get("BROWSER_EXECUTABLE"):
                options["executable_path"] = os.environ["BROWSER_EXECUTABLE"]
            browser = playwright.chromium.launch(**options)
            page = browser.new_page(viewport={"width": 1600, "height": 1000})
            errors = []
            page.on("pageerror", lambda error: errors.append(str(error)))
            page.goto(url + "/mmorpg/editor.html")
            house = page.get_by_role("option", name="house / 103", exact=True)
            expect(house).to_be_visible(timeout=30000)
            house.click()
            # Move the existing house through the real placement canvas, then undo.
            bounds = page.locator("#map").bounding_box()
            scale = min(bounds["width"], bounds["height"]) / 7200
            start_x = bounds["x"] + (bounds["width"] - 7200 * scale) / 2 + 1200 * scale
            start_z = bounds["y"] + (bounds["height"] - 7200 * scale) / 2 + 5200 * scale
            page.mouse.move(start_x, start_z)
            page.mouse.down()
            page.mouse.move(start_x - 100 * scale, start_z, steps=4)
            page.mouse.up()
            expect(page.locator("#x")).to_have_value("-25")
            page.locator("#undo").click()
            expect(page.locator("#x")).to_have_value("-24")
            page.locator("#x").fill("-24.5")
            page.locator("#x").press("Tab")
            expect(page.locator("#dirty")).to_have_text("Unsaved changes")
            # Keyboard nudge follows the same validation and persists exact centimetres.
            page.locator("#map").focus()
            page.keyboard.press("ArrowLeft")
            expect(page.locator("#x")).to_have_value("-24.6")
            page.locator("#undo").click()
            expect(page.locator("#x")).to_have_value("-24.5")
            page.locator("#prefab").select_option("house")
            page.locator("#add").click()
            expect(page.get_by_role("option", name="house / 60000", exact=True)).to_be_visible()
            page.locator("#remove").click()
            expect(page.get_by_role("option", name="house / 60000", exact=True)).to_have_count(0)
            page.locator("#undo").click()
            expect(page.get_by_role("option", name="house / 60000", exact=True)).to_be_visible()
            page.get_by_role("option", name="house / 103", exact=True).click()
            page.locator("#view").select_option("selection")
            page.screenshot(path=str(ARTIFACTS / "editor.png"))
            with page.expect_response(lambda response: response.url.endswith("/__layout") and response.request.method == "POST", timeout=180000) as save:
                page.locator("#save").click()
            assert save.value.ok, save.value.text()
            committed = json.loads(LAYOUT.read_text())
            assert next(edit for edit in committed["collision"] if edit["id"] == 103)["position"] == [-2450, 3600]
            page.reload()
            expect(page.get_by_role("option", name="house / 103", exact=True)).to_be_visible(timeout=30000)
            page.locator("#reopen").click()
            page.get_by_role("option", name="house / 103", exact=True).click()
            expect(page.locator("#x")).to_have_value("-24.5")
            expect(page.get_by_role("option", name="house / 60000", exact=True)).to_be_visible()
            page.goto(url + "/mmorpg/?debug")
            # Inspect the same built scene the running game consumes, then enter its real host.
            center = page.evaluate("""async () => {
              const game = await import('/mmorpg/src/generated/mmorpg-wasm/mmorpg_wasm.js');
              await game.default();
              return JSON.parse(game.scenery()).structures.find(structure => structure.colliderId === 103).center;
            }""")
            assert center[0] == -2450, center
            enter = page.get_by_role("button", name="Enter World", exact=False)
            expect(enter).to_be_enabled(timeout=60000)
            enter.click()
            expect(page.locator("#character-select")).to_be_hidden(timeout=30000)
            page.wait_for_function("window.__valeDebug.stats().self !== null")
            page.evaluate("window.__valeDebug.flyTo('hub')")
            page.screenshot(path=str(ARTIFACTS / "game.png"))
            assert not errors, errors
            browser.close()
            print("PASS: edit, nudge, add, remove, undo, save, rebuild, reopen and play the authored game")
    finally:
        server.terminate()
        server.wait(timeout=10)
        log.close()
        LAYOUT.write_text(original)
        subprocess.run(["bun", "scripts/build-wasm.ts"], cwd=ROOT / "web", check=True)


if __name__ == "__main__":
    main()
