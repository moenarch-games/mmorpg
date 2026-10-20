#!/usr/bin/env python3
"""Check the production Ranger loading example against a running dev/preview server."""
import argparse
from pathlib import Path
from playwright.sync_api import sync_playwright

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("url", help="example URL, e.g. http://localhost:5173/mmorpg/ranger-loader.html")
parser.add_argument("--screenshot", type=Path)
args = parser.parse_args()
with sync_playwright() as playwright:
    browser = playwright.chromium.launch(args=["--use-angle=swiftshader", "--enable-unsafe-swiftshader"])
    page = browser.new_page(viewport={"width": 960, "height": 720})
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.goto(args.url)
    page.wait_for_function("document.querySelector('#status').textContent.startsWith('Default Ranger:')", timeout=60_000)
    page.wait_for_function("document.querySelector('canvas').width > 0")
    if args.screenshot:
        args.screenshot.parent.mkdir(parents=True, exist_ok=True)
        page.screenshot(path=str(args.screenshot))
    assert not errors, errors
    print(page.locator("#status").inner_text())
    browser.close()
