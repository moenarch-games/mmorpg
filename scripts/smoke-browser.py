#!/usr/bin/env python3
"""Real Chromium acceptance against the production bundle and embedded WASM host.

The bundle embeds the shared Rust zone simulation as a WASM local host, so these
tests drive real movement, jumps, corpse claims and zone entry/exit through the page.
The isolated full-bag DOM stage receives a projection fixture; core tests own its
transaction-rule acceptance.

Prerequisites: build web/ (bun run build), pip install playwright==1.57.0, then
python -m playwright install --with-deps chromium.
Run from the repository root: python scripts/smoke-browser.py
"""
from __future__ import annotations

import base64
import functools
import http.server
import json
import math
import re
from pathlib import Path
import threading
import unittest

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "artifacts" / "browser"
ROSTER_KEY = "mmorpg.offline-roster.v1"
# The retired offline checkpoint key; nothing may write it any more.
LEGACY_CHECKPOINT_PREFIX = "mmorpg.offline-demo.v1."


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/mmorpg/"):
            self.path = self.path[len("/mmorpg"):]
        super().do_GET()

    def log_message(self, *_args):
        pass


class BrowserAcceptance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        dist = ROOT / "web" / "dist"
        if not (dist / "index.html").is_file():
            raise RuntimeError("Build the production web bundle before browser acceptance.")
        ARTIFACTS.mkdir(parents=True, exist_ok=True)
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(dist)))
        cls.server_thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.server_thread.start()
        cls.url = f"http://127.0.0.1:{cls.server.server_port}/mmorpg/"
        cls.playwright = sync_playwright().start()
        cls.browser = cls.playwright.chromium.launch(args=["--use-angle=swiftshader", "--enable-unsafe-swiftshader"])
        cls.evidence = []

    @classmethod
    def tearDownClass(cls):
        (ARTIFACTS / "browser-evidence.json").write_text(json.dumps(cls.evidence, indent=2))
        cls.browser.close()
        cls.playwright.stop()
        cls.server.shutdown()
        cls.server.server_close()
        cls.server_thread.join()

    def setUp(self):
        self.errors = []
        self.context = self.browser.new_context(viewport={"width": 1440, "height": 900}, reduced_motion="reduce")
        self.page = self.context.new_page()
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        self.context.add_init_script("""(() => {
          window.__calls = {reads: 0, writes: 0, draws: 0};
          for (const [name, counter] of [['getItem', 'reads'], ['setItem', 'writes']]) {
            const original = Storage.prototype[name];
            Storage.prototype[name] = function(...args) { window.__calls[counter]++; return original.apply(this, args); };
          }
          for (const Type of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
            if (!Type) continue;
            for (const name of ['drawArrays', 'drawElements', 'drawArraysInstanced', 'drawElementsInstanced']) {
              const original = Type.prototype[name];
              if (!original) continue;
              Type.prototype[name] = function(...args) { window.__calls.draws++; return original.apply(this, args); };
            }
          }
        })();""")

    def tearDown(self):
        self.page.screenshot(path=str(ARTIFACTS / f"{self._testMethodName}.png"))
        self.evidence.append({"test": self._testMethodName, "pageErrors": self.errors})
        self.context.close()
        self.assertEqual(self.errors, [], "Browser application raised an uncaught error")

    def open(self, query=""):
        self.page.goto(self.url + query)
        self.page.wait_for_function("window.__calls.draws > 0")
        expect(self.page.locator("#character-stage-fallback")).to_be_hidden()
        expect(self.page.get_by_role("slider", name="Character rotation")).to_be_visible()
        self.wait_for_zone()
        self.frames()

    def wait_for_zone(self):
        """The WASM local zone host loads asynchronously; entry is disabled until it is ready."""
        expect(self.enter_button()).to_be_enabled(timeout=20_000)
        expect(self.page.locator("#enter-world-note")).to_contain_text("Start ")

    def enter_button(self):
        return self.page.get_by_role("button", name=re.compile("^Enter World"))

    def enter_world(self, ticking=True):
        """Enters the world. The class choice resolves in the zone's first tick,
        so the resource check needs a ticking source (tests that pause the
        source check it after their own ticks)."""
        self.enter_button().click()
        expect(self.page.locator("#character-select")).to_be_hidden()
        expect(self.page.locator(".hud")).to_be_visible()
        expect(self.page.get_by_role("progressbar", name="Level 1 · 0 / 100 XP")).to_be_visible()
        self.assertEqual(self.page.locator("#experience-bar").evaluate("bar => [bar.max, bar.value]"), [100, 0])
        expect(self.page.locator("#progression-feedback")).to_have_text("")
        if ticking:
            self.expect_class_resource()
        self.page.locator("#world").focus()
        self.frames(4)

    def expect_class_resource(self):
        """Entry chose the character's class: the HUD names its resource."""
        expect(self.page.locator("#unit-status")).to_contain_text(re.compile(r"(Rage|Focus|Mana) \d+/\d+"))

    def world_view(self):
        """The world canvas between the HUD panels; the camera follows the character."""
        return self.page.screenshot(clip={"x": 200, "y": 120, "width": 1040, "height": 560})

    def wait_for_view(self, expected, timeout_frames=240):
        for _ in range(timeout_frames):
            self.frames(2)
            if self.world_view() == expected:
                return True
        return False

    def legacy_checkpoints(self):
        return self.page.evaluate("prefix => Object.keys(localStorage).filter(key => key.startsWith(prefix))", LEGACY_CHECKPOINT_PREFIX)

    def frames(self, count=3):
        self.page.evaluate("""count => new Promise(resolve => {
          function next() { if (--count <= 0) resolve(); else requestAnimationFrame(next); }
          requestAnimationFrame(next);
        })""", count)

    def click_world_setup(self):
        """Keep the real zone deterministic while physical pointer events drive the page."""
        self.open("?debug")
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          window.__clickAdvance = source.advance;
          window.__clickQueue = [];
          window.__clickQuestEntries = [];
          source.advance = () => window.__clickQueue.splice(0);
          window.__clickStep = ticks => {
            for (let i = 0; i < ticks; i++) {
              const projections = window.__clickAdvance.call(source, 1 / 30);
              for (const projection of projections) {
                if (projection.quests) window.__clickQuestEntries = projection.quests.entries;
              }
              window.__clickQueue.push(...projections);
            }
          };
        }""")
        self.enter_world(ticking=False)
        self.click_world_step()

    def click_world_step(self, ticks=3):
        self.frames(2)
        self.page.evaluate("ticks => window.__clickStep(ticks)", ticks)
        self.frames(3)

    def click_world_walk_to_woods(self):
        # The existing real-zone combat smoke route; no target or attack commands.
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          const moves = new Map([[0,[1,0]],[36,[1,49152]],[121,[1,32768]],
            [131,[1,49152]],[375,[1,32768]],[395,[0,32768]]]);
          for (let tick = 0; tick < 400; tick++) {
            const move = moves.get(tick);
            if (move) source.sendCommand({kind:'move',forward:move[0],strafe:0,facing:move[1]});
            window.__clickAdvance.call(source, 1 / 30);
          }
        }""")
        self.click_world_step()

    def click_world_target(self):
        return self.page.evaluate("window.__valeDebug.worldSource().latestProjection().viewer.target")

    def aim_at_world_unit(self, kind, identity):
        """Aim the existing screenshot camera at an actual rendered unit's body.

        This only changes the camera; it neither chooses a target nor changes the zone.
        """
        point = self.page.evaluate("""([kind, id]) => {
          const debug = window.__valeDebug;
          const unit = debug.projectedUnits().find(unit => unit.kind === kind && unit.id === id);
          if (!unit) throw Error(`Missing projected ${kind} ${id}`);
          const y = debug.reliefAt(unit.x * 100, unit.z * 100) / 100 + (kind === 'npc' ? 0.9 : 0.5);
          debug.flyToPose([unit.x, y + 1, unit.z + 5], [unit.x, y, unit.z]);
          return {x: unit.x, y, z: unit.z};
        }""", [kind, identity])
        self.frames(4)
        self.assertTrue(self.page.evaluate("([kind,id]) => window.__valeDebug.unitNodeIds(`unit-${kind}-${id}`).length > 0", [kind, identity]))
        box = self.page.locator("#world").bounding_box()
        return box["x"] + box["width"] / 2, box["y"] + box["height"] / 2, point

    def test_world_click_specific_creature_drag_empty_ground_and_alternatives(self):
        self.click_world_setup()
        self.click_world_walk_to_woods()
        creature = self.page.evaluate("window.__valeDebug.projectedUnits().find(unit => unit.kind === 'creature' && unit.family === 'wolf')")
        self.assertIsNotNone(creature, "The real Greyhaven projection must include a wolf")
        x, y, _ = self.aim_at_world_unit("creature", creature["id"])
        self.page.screenshot(path=str(ARTIFACTS / "world-click-wolf-before.png"))
        self.page.mouse.click(x, y)
        self.click_world_step()
        expected = {"kind": "creature", "id": creature["id"]}
        self.assertEqual(self.click_world_target(), expected, "Clicking the rendered wolf must select its exact authoritative identity")
        expect(self.page.locator("[data-part=target-name]")).to_contain_text("Timber Wolf")
        self.assertFalse(self.page.evaluate("window.__valeDebug.worldSource().latestProjection().viewer.autoAttacking"))
        self.page.screenshot(path=str(ARTIFACTS / "world-click-wolf.png"))
        self.page.evaluate("window.__valeDebug.follow()")
        self.frames(4)
        before = self.world_view()
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 100, y + 20, steps=8)
        self.page.mouse.up()
        self.click_world_step()
        self.assertEqual(self.click_world_target(), expected, "Camera drag must not retarget")
        self.frames(4)
        self.assertNotEqual(self.world_view(), before, "Camera drag still draws a changed view")
        # Aim at empty sky, then click the centre, which cannot denote a unit.
        self.page.evaluate("window.__valeDebug.flyToPose([0, 40, 0], [0, 80, -5])")
        self.frames(3)
        self.page.mouse.click(x, y)
        self.click_world_step()
        self.assertFalse(self.page.evaluate("window.__valeDebug.worldSource().latestProjection().viewer.autoAttacking"))
        expect(self.page.get_by_role("complementary", name="Quest dialog", exact=True)).to_be_hidden()
        expect(self.page.get_by_role("complementary", name="Vendor", exact=True)).to_be_hidden()
        self.page.locator("#world").focus()
        self.page.keyboard.press("Tab")
        self.click_world_step()
        self.assertEqual(self.click_world_target()["kind"], "creature", "Tab remains an alternative")
        self.page.mouse.click(x, y, button="right")
        self.click_world_step()
        self.assertTrue(self.page.evaluate("window.__valeDebug.worldSource().latestProjection().viewer.autoAttacking"), "Right click still toggles attack")

    def test_world_click_distinct_npcs_context_reach_and_no_fabricated_dialogue(self):
        self.click_world_setup()
        for identity, name, context in [
            (1, "Marshal Elden Greywatch", "quest"),
            (2, "Tanner Hilda Brook", "quest"),
            (3, "Innkeeper Bram Tolliver", "vendor"),
            (6, "Greyhaven Guard", None),
        ]:
            with self.subTest(npc=identity):
                # Close panels through their existing controls before the next world click.
                for label in ["Close quest dialog", "Close vendor"]:
                    close = self.page.get_by_role("button", name=label, exact=True)
                    if close.is_visible():
                        close.click()
                x, y, _ = self.aim_at_world_unit("npc", identity)
                self.page.mouse.click(x, y)
                self.click_world_step()
                self.assertEqual(self.click_world_target(), {"kind": "npc", "id": identity})
                expect(self.page.locator("[data-part=target-name]")).to_contain_text(name)
                self.assertFalse(self.page.evaluate("window.__valeDebug.worldSource().latestProjection().viewer.autoAttacking"))
                if context == "quest":
                    # The existing semantic Talk action may reveal the clicked NPC's context.
                    self.page.locator("#world").focus()
                    dialog = self.page.get_by_role("complementary", name="Quest dialog", exact=True)
                    if not dialog.is_visible():
                        self.page.keyboard.press("t")
                    expect(dialog).to_be_visible()
                    expect(dialog.get_by_role("heading", name=name, exact=True)).to_be_visible()
                    if identity == 1:
                        expect(self.page.locator("#quest-dialog-status")).to_contain_text("Move within 5 m")
                        expect(self.page.get_by_role("button", name="Accept: Trouble in the Woods")).to_be_disabled()
                    self.assertEqual(self.page.evaluate("window.__clickQuestEntries"), [], "Selecting or talking must not accept a quest")
                elif context == "vendor":
                    panel = self.page.get_by_role("complementary", name="Vendor", exact=True)
                    if not panel.is_visible():
                        self.page.locator("#world").focus()
                        self.page.keyboard.press("v")
                    expect(panel).to_be_visible()
                    expect(panel.get_by_role("heading", name=name, exact=True)).to_be_visible()
                    expect(self.page.locator("#vendor-status")).to_contain_text("Move within 5 m")
                    for buy in panel.get_by_role("button", name=re.compile("^Buy ")).all():
                        expect(buy).to_be_disabled()
                else:
                    self.page.locator("#world").focus()
                    self.page.keyboard.press("t")
                    self.frames(3)
                    # A guard has no authored quest. Talk may show a refusal/empty context,
                    # but must never silently open a nearby Marshal/Tanner's quest.
                    self.assertEqual(self.page.locator("#quest-dialog-quests .quest-card").count(), 0)
                    expect(self.page.get_by_role("heading", name="Marshal Elden Greywatch", exact=True)).to_be_hidden()
                self.page.screenshot(path=str(ARTIFACTS / f"world-click-npc-{identity}.png"))

    def test_world_click_cancel_and_capture_loss_never_select_or_attack(self):
        self.click_world_setup()
        for cancellation in ["pointercancel", "lostpointercapture"]:
            with self.subTest(cancellation=cancellation):
                x, y, _ = self.aim_at_world_unit("npc", 1)
                canvas = self.page.locator("#world")
                canvas.evaluate("el => el.addEventListener('pointerdown', e => window.__clickPointerId = e.pointerId, {once:true})")
                self.page.mouse.move(x, y)
                self.page.mouse.down()
                if cancellation == "pointercancel":
                    canvas.evaluate("el => el.dispatchEvent(new PointerEvent('pointercancel', {pointerId:window.__clickPointerId, bubbles:true}))")
                else:
                    canvas.evaluate("el => el.releasePointerCapture(window.__clickPointerId)")
                    self.page.mouse.move(x + 1, y)
                self.page.mouse.up()
                self.click_world_step()
                self.assertIsNone(self.click_world_target(), "A cancelled gesture cannot become selection")
                self.assertFalse(self.page.evaluate("window.__valeDebug.worldSource().latestProjection().viewer.autoAttacking"))
                expect(self.page.get_by_role("complementary", name="Quest dialog", exact=True)).to_be_hidden()

    def test_world_click_solid_palisade_does_not_select_hidden_guard(self):
        self.click_world_setup()
        _, _, point = self.aim_at_world_unit("npc", 6)
        # North guard stands inside the north wall. View him from outside it,
        # below the wall top: his projected centre exists but is not visible.
        self.page.evaluate("p => window.__valeDebug.flyToPose([p.x, p.y, p.z - 8], [p.x,p.y,p.z])", point)
        self.frames(4)
        self.page.screenshot(path=str(ARTIFACTS / "world-click-solid-occluder.png"))
        box = self.page.locator("#world").bounding_box()
        self.page.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2)
        self.click_world_step()
        self.assertIsNone(self.click_world_target(), "A solid wall must block clicking the guard behind it")
        expect(self.page.get_by_role("complementary", name="Quest dialog", exact=True)).to_be_hidden()

    def test_world_click_overlapping_creatures_selects_front_visible_body(self):
        self.click_world_setup()
        self.click_world_walk_to_woods()
        # Use two real projected wolves. Put the camera on the line through their
        # body centres so the nearer body covers the farther body's centre pixel.
        pair = self.page.evaluate("""() => {
          const d = window.__valeDebug;
          const wolves = d.projectedUnits().filter(u => u.kind === 'creature' && u.family === 'wolf');
          if (wolves.length < 2) throw Error('Need two real Greyhaven wolves');
          const [front, back] = wolves;
          const centre = u => [u.x, d.reliefAt(u.x * 100, u.z * 100) / 100 + 0.5, u.z];
          const a = centre(front), b = centre(back);
          const distance = Math.hypot(...a.map((v,i) => v-b[i]));
          const eye = a.map((v,i) => v + (v-b[i]) / distance * 4);
          d.flyToPose(eye, a);
          return [front.id, back.id];
        }""")
        self.frames(4)
        box = self.page.locator("#world").bounding_box()
        self.page.screenshot(path=str(ARTIFACTS / "world-click-overlap-before.png"))
        self.page.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2)
        self.click_world_step()
        self.assertEqual(self.click_world_target(), {"kind": "creature", "id": pair[0]}, "Overlapping projection centres must choose the nearer visible body")

    def test_rotation_mouse_keyboard_reset_and_rendered_pixels(self):
        self.open()
        surface = self.page.get_by_role("slider", name="Character rotation")
        # Reach the front view the way the reset check below does (a mouse click, then
        # focus), so both screenshots share the same :focus-visible state.
        self.page.get_by_role("button", name="Reset view").click()
        surface.focus()
        box = surface.bounding_box()
        clip = {"x": box["x"] + 5, "y": box["y"] + 5, "width": box["width"] - 10, "height": box["height"] - 10}
        self.frames()
        front = self.page.screenshot(clip=clip)
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-front.png"))
        x, y = box["x"] + box["width"] / 3, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + box["width"] / 4, y, steps=8)
        self.page.mouse.up()
        expect(surface).to_have_attribute("aria-valuenow", "90")
        self.frames()
        self.assertNotEqual(self.page.screenshot(clip=clip), front, "Changing yaw must rotate the rendered 3D model, not only ARIA state")
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-rotated.png"))
        surface.press("ArrowRight")
        expect(surface).to_have_attribute("aria-valuenow", "105")
        surface.press("Home")
        expect(surface).to_have_attribute("aria-valuenow", "0")
        self.page.get_by_role("button", name="Rotate character left").click()
        expect(surface).to_have_attribute("aria-valuenow", "345")
        self.page.get_by_role("button", name="Reset view").click()
        surface.focus()
        self.frames()
        self.assertEqual(self.page.screenshot(clip=clip), front, "Reset restores the rendered front view")
        reads = self.page.evaluate("window.__calls.reads")
        writes = self.page.evaluate("window.__calls.writes")
        self.frames(15)
        self.assertEqual(self.page.evaluate("window.__calls.reads"), reads)
        self.assertEqual(self.page.evaluate("window.__calls.writes"), writes)

    def test_pointer_cancel_capture_loss_and_control_isolation(self):
        self.open()
        surface = self.page.locator("#preview-surface")
        surface.evaluate("el => el.addEventListener('pointerdown', e => window.__pointerId = e.pointerId)")
        box = surface.bounding_box()
        x, y = box["x"] + 30, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 80, y)
        angle = surface.get_attribute("aria-valuenow")
        self.assertNotEqual(angle, "0")
        surface.dispatch_event("pointercancel", {"pointerId": 99})
        expect(surface).to_have_attribute("aria-valuenow", angle)
        surface.evaluate("el => el.releasePointerCapture(window.__pointerId)")
        # Capture changes are processed before the next pointer event, not on rAF.
        # https://www.w3.org/TR/pointerevents3/#process-pending-pointer-capture
        self.page.mouse.move(x + 81, y)
        self.frames()
        expect(surface).to_have_attribute("aria-valuenow", "0")
        self.page.mouse.up()
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 80, y)
        pointer_id = self.page.evaluate("window.__pointerId")
        surface.dispatch_event("pointercancel", {"pointerId": pointer_id})
        expect(surface).to_have_attribute("aria-valuenow", "0")
        self.page.mouse.up()
        self.page.locator('[data-hat-style="ranger-cap"]').press("Enter")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(surface).to_have_attribute("aria-valuenow", "0")
        self.page.get_by_role("button", name="Reset view").press("Enter")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(surface).to_have_attribute("aria-valuenow", "0")

    def test_enter_world_move_jump_orbit_and_leave(self):
        self.open()
        self.assertEqual(self.page.get_by_role("button", name=re.compile("^(Save|Load|Export|Import) (game|save)$")).count(), 0,
                         "No control may claim to save world progress the demo cannot persist")
        self.assertNotRegex(self.page.locator("body").text_content(), re.compile(r"saved (facing|position)", re.IGNORECASE),
                            "No text may claim world state is saved; the demo cannot persist it")
        self.page.locator('[data-hat-style="ironcrest-helm"]').click()
        writes = self.page.evaluate("window.__calls.writes")
        self.enter_world()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")
        expect(self.page.locator("#objective")).to_contain_text("No active quests")
        self.frames(6)
        spawn = self.world_view()
        self.page.screenshot(path=str(ARTIFACTS / "world-spawn.png"))
        self.frames(6)
        self.assertEqual(self.world_view(), spawn, "An idle character in a static zone renders identically")

        # W runs forward along the camera heading through the WASM zone; the camera follows.
        self.page.keyboard.down("KeyW")
        self.frames(24)
        self.page.keyboard.up("KeyW")
        self.frames(4)
        moved = self.world_view()
        self.assertNotEqual(moved, spawn, "Holding W must move the character and its follow camera")
        self.page.screenshot(path=str(ARTIFACTS / "world-moved.png"))

        # Space jumps: body and camera rise, then physics lands the unit where it took off.
        self.page.keyboard.press("Space")
        self.frames(3)
        self.assertNotEqual(self.world_view(), moved, "A grounded jump must lift the character")
        self.page.screenshot(path=str(ARTIFACTS / "world-jump.png"))
        self.assertTrue(self.wait_for_view(moved), "The character lands back on the ground it jumped from")

        # Dragging the canvas orbits the camera; the wheel zooms.
        box = self.page.locator("#world").bounding_box()
        x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 180, y - 40, steps=6)
        self.page.mouse.up()
        self.page.mouse.wheel(0, -240)
        self.frames(4)
        self.assertNotEqual(self.world_view(), moved, "Orbit and zoom must change the view")
        self.page.screenshot(path=str(ARTIFACTS / "world-orbit.png"))
        self.assertEqual(self.page.evaluate("window.__calls.writes"), writes, "Playing never writes browser storage")

        # Leaving removes the unit from the local zone; re-entry spawns a fresh character at the spawn.
        self.page.get_by_role("button", name="Characters", exact=True).click()
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(self.enter_button()).to_be_enabled()
        self.enter_world()
        self.assertTrue(self.wait_for_view(spawn, timeout_frames=20),
                        "Re-entering must show a new character alone at the spawn with a reset camera")
        self.page.locator("#world").focus()
        self.page.keyboard.press("Escape")
        expect(self.page.locator("#character-select")).to_be_visible()
        self.assertEqual(self.legacy_checkpoints(), [])

    def test_rejected_projections_fail_closed_without_freezing_the_page(self):
        # Byte 2 of a snapshot header is its schema version. While the flag is set the
        # strict decoder sees an unknown version, as after a WASM/decoder version skew.
        self.context.add_init_script("""(() => {
          window.__rejectSnapshots = false;
          const original = DataView.prototype.getUint16;
          DataView.prototype.getUint16 = function(offset, ...rest) {
            return window.__rejectSnapshots && offset === 2 ? 0xffff : original.call(this, offset, ...rest);
          };
        })();""")
        self.open()
        note = self.page.locator("#enter-world-note")

        # A refused entry keeps the page on selection, says why, and keeps rendering.
        self.page.evaluate("window.__rejectSnapshots = true")
        self.enter_button().click()
        expect(note).to_contain_text("Could not enter the world: Unsupported snapshot version")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(self.page.locator(".hud")).to_be_hidden()
        self.assert_rendering()

        # The refused join left nothing joined, so entry works once projections decode.
        self.page.evaluate("window.__rejectSnapshots = false")
        self.enter_world()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")

        # A projection rejected while playing leaves the world instead of stopping the loop.
        self.page.evaluate("window.__rejectSnapshots = true")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(self.page.locator(".hud")).to_be_hidden()
        expect(note).to_contain_text("Left the world after an error: Unsupported snapshot version")
        self.page.evaluate("window.__rejectSnapshots = false")
        self.assert_rendering()
        self.enter_world()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")
        self.page.keyboard.press("Escape")
        expect(note).to_contain_text("Start ")

    def colour_stats(self, png: bytes):
        """Distinct RGB colours in a screenshot and the share of pixels that differ from the page
        background, counted in the page (no image library needed)."""
        return self.page.evaluate("""async data => {
          const image = new Image();
          image.src = 'data:image/png;base64,' + data;
          await image.decode();
          const canvas = document.createElement('canvas');
          canvas.width = image.width;
          canvas.height = image.height;
          const context = canvas.getContext('2d');
          context.drawImage(image, 0, 0);
          const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
          const background = getComputedStyle(document.body).backgroundColor.match(/\\d+/g).map(Number);
          const colours = new Set();
          let samples = 0;
          let covered = 0;
          for (let i = 0; i < pixels.length; i += 4 * 5) {
            const rgb = [pixels[i], pixels[i + 1], pixels[i + 2]];
            colours.add((rgb[0] << 16) | (rgb[1] << 8) | rgb[2]);
            samples += 1;
            if (rgb.some((channel, index) => Math.abs(channel - background[index]) > 3)) covered += 1;
          }
          return {distinct: colours.size, covered: covered / samples};
        }""", base64.b64encode(png).decode())

    def canvas_colours(self, hide_canvas=False):
        """Colour statistics of what the renderer drew in the world clip. The canvas is
        transparent over the CSS sky, and the minimap, HUD and F3 overlay sit on top of it;
        with those hidden, only rendered pixels differ from the plain page background."""
        layers = "#sky, #debug-overlay, [data-world-ui]" + (", #world" if hide_canvas else "")
        self.page.evaluate("layers => document.querySelectorAll(layers).forEach(e => { e.style.visibility = 'hidden'; })", layers)
        try:
            return self.colour_stats(self.world_view())
        finally:
            self.page.evaluate("layers => document.querySelectorAll(layers).forEach(e => { e.style.visibility = ''; })", layers)

    def debug_stats(self):
        return self.page.evaluate("window.__valeDebug.stats()")

    def test_vale_viewpoints_overlay_minimap_and_mouse_look(self):
        """Debug-only camera viewpoints (never the simulation) show the subzones; F3 reports scene work."""
        self.open("?debug")
        self.enter_world()
        self.frames(6)
        spawn = self.debug_stats()["self"]
        overlay = self.page.get_by_role("region", name="Debug statistics")
        expect(overlay).to_be_hidden()
        self.page.keyboard.press("F3")
        expect(overlay).to_be_visible()
        minimap = self.page.get_by_role("complementary", name="Minimap")
        expect(minimap).to_be_visible()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")
        # Control: without the canvas the measurement sees only the plain page background.
        self.assertEqual(self.canvas_colours(hide_canvas=True), {"distinct": 1, "covered": 0})
        viewpoints = {}
        for name in ["hub", "woods", "hollow"]:
            self.page.evaluate("name => window.__valeDebug.flyTo(name)", name)
            self.frames(8)
            self.page.screenshot(path=str(ARTIFACTS / f"viewpoint-{name}.png"))
            colours = self.canvas_colours()
            self.assertGreater(colours["distinct"], 400, f"The {name} viewpoint must render a varied scene, not a uniform canvas")
            self.assertGreater(colours["covered"], 0.5, f"The {name} viewpoint must draw most of the view, not leave the canvas empty")
            viewpoints[name] = {"canvasColours": colours, "frame": self.debug_stats()["frame"]}
        self.frames(12)
        stats = self.debug_stats()
        overlay_stats = json.loads(overlay.get_attribute("data-stats"))
        self.assertGreater(overlay_stats["nodes"], overlay_stats["staticNodes"])
        self.assertLessEqual(overlay_stats["staticNodes"], 600, "Static batching keeps draw calls bounded")
        self.assertGreater(overlay_stats["fps"], 0)
        (ARTIFACTS / "scene-stats.json").write_text(json.dumps({
            "note": "fps comes from headless Chromium with SwiftShader software rendering; it is not a hardware measurement",
            "overlay": overlay_stats,
            "buildMs": stats["buildMs"],
            "scene": stats["scene"],
            "viewpoints": viewpoints,
        }, indent=2))
        # The viewpoints moved only the camera: the character never left its spawn.
        self.assertEqual(stats["self"], spawn)
        self.page.evaluate("window.__valeDebug.follow()")

        # Minimap zoom buttons step through their range; clicking them leaves the keyboard
        # with the world, so movement, jumps and F3 keep working without clicking the canvas.
        zoom_out = self.page.get_by_role("button", name="Zoom minimap out")
        for _ in range(4):
            if zoom_out.is_enabled():
                zoom_out.click()
        expect(zoom_out).to_be_disabled()
        north_up = self.page.get_by_role("button", name="Keep north up")
        north_up.click()
        expect(north_up).to_have_attribute("aria-pressed", "true")
        self.page.keyboard.down("KeyW")
        self.frames(12)
        self.page.keyboard.up("KeyW")
        self.frames(4)
        ran = self.debug_stats()["self"]
        self.assertNotEqual((ran["x"], ran["z"]), (spawn["x"], spawn["z"]), "W runs right after a minimap click")
        self.page.keyboard.press("Space")
        self.frames(2)
        expect(north_up).to_have_attribute("aria-pressed", "true")
        self.page.keyboard.press("F3")
        expect(overlay).to_be_hidden()
        self.page.keyboard.press("F3")
        expect(overlay).to_be_visible()

        # A left drag looks around without turning the character; a right drag turns it.
        box = self.page.locator("#world").bounding_box()
        x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 200, y, steps=5)
        self.page.mouse.up()
        self.frames(8)
        self.assertEqual(self.debug_stats()["self"]["facing"], ran["facing"], "Left-drag orbit leaves the facing alone")
        self.page.mouse.move(x, y)
        self.page.mouse.down(button="right")
        self.page.mouse.move(x + 200, y, steps=5)
        self.page.mouse.up(button="right")
        self.assertTrue(self.wait_for_facing_change(ran["facing"]), "Right-drag mouse-look turns the character")
        self.page.screenshot(path=str(ARTIFACTS / "world-mouse-look.png"))
        self.page.keyboard.press("F3")
        expect(overlay).to_be_hidden()

    # Wolf 107, the nearest Wolfrun Woods spawn to the hub, stands at (-62, -18) m (units.rs).
    NEAREST_ANIMAL_SPAWN = (-62.0, -18.0)

    def self_position(self):
        me = self.debug_stats()["self"]
        return me["x"], me["z"]

    def walk_until_animal_projected(self, max_steps=40):
        """The authoritative player starts on the hub grid, outside the interest radius of the woods, so
        a creature is only projected once the player walks toward the nearest animal spawn. Returns the
        nearest projected animal; fails with a clear message when none appears within the bounded walk."""
        goal_x, goal_z = self.NEAREST_ANIMAL_SPAWN
        key = None
        for _ in range(max_steps):
            x, z = self.self_position()
            animal = self.page.evaluate("([x, z]) => window.__valeDebug.nearestAnimal(x, z)", [x, z])
            if animal:
                return animal
            if key is None:
                # W/S/A/D move relative to the camera heading: probe each and keep the one that closes the distance.
                best = None
                for candidate in ("KeyW", "KeyS", "KeyA", "KeyD"):
                    before = self.self_position()
                    self.page.keyboard.down(candidate)
                    self.frames(10)
                    self.page.keyboard.up(candidate)
                    self.frames(2)
                    after = self.self_position()
                    gain = math.hypot(goal_x - before[0], goal_z - before[1]) - math.hypot(goal_x - after[0], goal_z - after[1])
                    if best is None or gain > best[0]:
                        best = (gain, candidate)
                key = best[1]
                continue
            self.page.keyboard.down(key)
            self.frames(12)
            self.page.keyboard.up(key)
            self.frames(2)
        units = self.page.evaluate("window.__valeDebug.projectedUnits()")
        self.fail(f"No wolf, boar or rat was projected after walking toward {self.NEAREST_ANIMAL_SPAWN}; player at {self.self_position()}, projected: {units}")

    def test_creature_and_npc_models_render_over_the_placeholder_boxes(self):
        """Debug viewpoints beside a projected wolf/boar/rat and a gate guard show the low-poly models,
        which draw more colours and a different silhouette than the single-colour placeholder boxes."""
        self.open("?debug")
        self.enter_world()
        self.frames(6)
        # Only units inside the interest radius are projected: walk toward the woods until an animal is.
        animal = self.walk_until_animal_projected()
        self.frames(4)
        animal = self.page.evaluate("([x, z]) => window.__valeDebug.nearestAnimal(x, z)", list(self.self_position())) or animal
        ax, az = animal["x"], animal["z"]
        # Animals wander, so the camera looks at the projected position from 5 m away, high enough to see over the grass.
        # Guard 6 stands at (-5, -8) m, inside the hub's interest range.
        views = {
            "animal": ([ax, 2.0, az - 5.0], [ax, 0.0, az]),
            "guard": ([-5.0, 1.5, -3.0], [-5.0, 0.0, -8.0]),
        }
        # The units' scene identities; a placeholder draws only `-body` and `-nose` (plus a target ring).
        identities = {"animal": f"unit-creature-{animal['id']}", "guard": "unit-npc-6"}
        shots = {}
        for name, (eye, target) in views.items():
            # Heights above the presentation relief at the target.
            self.page.evaluate("""([eye, target]) => {
              const ground = window.__valeDebug.reliefAt(target[0] * 100, target[2] * 100) / 100;
              window.__valeDebug.flyToPose([eye[0], ground + eye[1], eye[2]], [target[0], ground + target[1] + 0.6, target[2]]);
            }""", [eye, target])
            self.frames(10)
            shots[name] = self.world_view()
            self.page.screenshot(path=str(ARTIFACTS / f"creature-model-{name}.png"))
            # Model-specific observable, independent of where the unit wandered: its model nodes are
            # in the current frame's scene, and not only the placeholder box and nose.
            identity = identities[name]
            ids = self.page.evaluate("identity => window.__valeDebug.unitNodeIds(identity)", identity)
            parts = {i[len(identity) + 1:] for i in ids}
            self.assertGreaterEqual(len(parts), 8, f"{identity} must draw a multi-part model, got {sorted(parts)}")
            self.assertFalse(parts <= {"body", "nose", "target-ring"}, f"{identity} regressed to the placeholder box")
            if name == "animal":
                self.assertTrue({"ear-left", "ear-right", "eye-left", "eye-right"} <= parts, f"The {animal['family']} draws its animal parts: {sorted(parts)}")
            if name == "guard":
                self.assertIn("head", parts, "The guard draws a humanoid model")
            colours = self.canvas_colours()
            self.assertGreater(colours["distinct"], 400, f"The {name} view must render a varied scene")
            self.assertGreater(colours["covered"], 0.5, f"The {name} view must draw most of the frame")
        self.assertNotEqual(shots["animal"], shots["guard"], "Animal and NPC models render different frames")
        self.page.evaluate("window.__valeDebug.follow()")

    def wait_for_facing_change(self, facing, timeout_frames=60):
        for _ in range(timeout_frames):
            self.frames(2)
            if abs(self.debug_stats()["self"]["facing"] - facing) > 1e-3:
                return True
        return False

    def assert_rendering(self):
        draws = self.page.evaluate("window.__calls.draws")
        self.frames(3)
        self.assertGreater(self.page.evaluate("window.__calls.draws"), draws, "The render loop must keep running")

    def test_saved_grass_clearing_and_bounded_batches(self):
        self.open("?debug")
        self.enter_world()
        before = self.debug_stats()["self"]
        self.page.evaluate("window.__valeDebug.flyTo('grass')")
        self.frames(8)
        self.page.screenshot(path=str(ARTIFACTS / "outpost-grass-clearing.png"))
        stats = self.debug_stats()
        self.assertEqual(stats["self"], before)
        self.assertEqual(stats["scene"]["props"]["grass-tuft"], 2337)
        self.assertLessEqual(stats["scene"]["staticNodes"], 600)
        self.assertLessEqual(stats["scene"]["staticVertices"], 230_000)
        colours = self.canvas_colours()
        self.assertGreater(colours["distinct"], 400)
        self.assertGreater(colours["covered"], 0.5)
        (ARTIFACTS / "outpost-grass-evidence.json").write_text(json.dumps({
            "acceptedInstances": 118, "selectedInstances": 55,
            "clearingMetres": [-28, 8, -18, 16],
            "scene": stats["scene"], "canvasColours": colours,
        }, indent=2))

    def test_saved_relief_approaches_and_shared_presentation_identity(self):
        self.open("?debug")
        self.enter_world()
        before = self.debug_stats()["self"]
        points = [[1050, 650], [250, 950], [-850, 1050], [-1100, 2950], [1050, 2950]]
        heights = self.page.evaluate("points => points.map(([x,z]) => window.__valeDebug.reliefAt(x,z))", points)
        self.assertEqual(heights, [0] * len(points))
        views = []
        for name in ["relief", "hub"]:
            self.page.evaluate("name => window.__valeDebug.flyTo(name)", name)
            self.frames(8)
            self.page.screenshot(path=str(ARTIFACTS / f"outpost-relief-{name}.png"))
            stats = self.debug_stats()
            self.assertEqual(stats["self"], before)
            self.assertLessEqual(stats["scene"]["staticNodes"], 600)
            self.assertLessEqual(stats["scene"]["staticVertices"], 230_000)
            colours = self.canvas_colours()
            self.assertGreater(colours["distinct"], 400)
            self.assertGreater(colours["covered"], 0.5)
            views.append({"viewpoint": name, "scene": stats["scene"], "canvasColours": colours})
        (ARTIFACTS / "outpost-relief-evidence.json").write_text(json.dumps({
            "presentationFingerprint": stats["presentationFingerprint"],
            "originXzUnits": [-3500, -1300], "stepUnits": 50, "columns": 141, "rows": 133,
            "flatApproaches": points, "heights": heights, "views": views,
        }, indent=2))

    def test_storage_denial_keeps_the_world_playable(self):
        self.context.add_init_script("Object.defineProperty(window, 'localStorage', {get() { throw new DOMException('Storage denied', 'SecurityError'); }});")
        self.open()
        expect(self.page.locator("#roster-status")).to_contain_text("could not be read")
        self.page.locator(".appearance-storage summary").click()
        self.page.get_by_role("button", name="Save appearance").click()
        expect(self.page.locator("#save-status")).to_contain_text("storage is unavailable")
        self.enter_world()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")
        self.page.keyboard.down("KeyD")
        self.frames(10)
        self.page.keyboard.up("KeyD")
        self.page.get_by_role("button", name="Characters", exact=True).click()
        expect(self.page.locator("#character-select")).to_be_visible()

    def test_mobile_touch_rotation_scroll_and_layout(self):
        self.context.close()
        self.context = self.browser.new_context(viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True, device_scale_factor=1)
        self.page = self.context.new_page()
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        self.page.goto(self.url)
        expect(self.page.locator("#character-stage-fallback")).to_be_hidden()
        surface = self.page.locator("#preview-surface")
        surface.scroll_into_view_if_needed()
        self.frames(10)
        box = surface.bounding_box()
        session = self.context.new_cdp_session(self.page)
        x, y = box["x"] + box["width"] / 3, box["y"] + box["height"] / 2
        session.send("Input.dispatchTouchEvent", {"type": "touchStart", "touchPoints": [{"x": x, "y": y, "id": 1}]})
        session.send("Input.dispatchTouchEvent", {"type": "touchMove", "touchPoints": [{"x": x + 70, "y": y, "id": 1}]})
        session.send("Input.dispatchTouchEvent", {"type": "touchEnd", "touchPoints": []})
        self.assertNotEqual(surface.get_attribute("aria-valuenow"), "0", "A real touch drag must turn the character")
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-mobile.png"))
        overflow = self.page.locator("#character-select").evaluate("el => el.scrollWidth > el.clientWidth + 1")
        self.assertFalse(overflow, "Mobile selection must not scroll horizontally")
        self.assert_selection_layout()
        self.page.locator(".selection-actions").scroll_into_view_if_needed()
        self.frames()
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-mobile-resume.png"))
        session.detach()

    def test_projected_corpse_loot_claim_recovery_refusal_and_reset(self):
        """Actual WASM hunt/claim/loss recovery, then an isolated full-bag presentation fixture."""
        self.open("?debug")
        # Pause publication and held-input resends through the public source. Explicit
        # commands/ticks still drive the same WASM authority and production decoder.
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          window.__lootSource = source;
          window.__lootSend = source.sendCommand.bind(source);
          window.__lootAdvance = source.advance.bind(source);
          window.__lootLatest = source.latestProjection.bind(source);
          window.__lootSent = [];
          source.advance = () => [];
          source.sendCommand = command => {
            if (command.kind === 'move' || command.kind === 'jump') return;
            window.__lootSent.push(command);
            if (window.__lootFixture) {
              const fixture = window.__lootFixture;
              fixture.tick++;
              fixture.acknowledgedSequence++;
              fixture.events = command.kind === 'loot'
                ? [{kind: 'error', code: 'inventory-full', target: {kind: 'creature', id: 108}}] : [];
              return;
            }
            window.__lootSend(command);
          };
          window.__lootStep = ticks => {
            for (let tick = 0; tick < ticks; tick++) window.__lootAdvance(1 / 30);
          };
        }""")
        # The paused source has not run the tick that takes the class choice yet.
        self.enter_world(ticking=False)
        self.page.evaluate("""() => {
          const moves = new Map([[0,[1,0]],[36,[1,49152]],[121,[1,32768]],
            [131,[1,49152]],[375,[1,32768]],[395,[0,32768]]]);
          for (let tick = 0; tick < 912; tick++) {
            const move = moves.get(tick);
            if (move) window.__lootSend({kind:'move',forward:move[0],strafe:0,facing:move[1]});
            if (tick === 1) window.__lootSend({kind:'start-attack'});
            if (tick === 380) {
              window.__lootSend({kind:'select-target',target:{kind:'creature',id:108}});
              window.__lootSend({kind:'start-attack'});
            }
            window.__lootStep(1);
          }
          window.__lootBefore = window.__lootLatest();
        }""")
        self.assertEqual(self.page.evaluate("Number(window.__lootLatest().loot.diedAt)"), 912)
        self.expect_class_resource()
        self.page.evaluate("""() => {
          window.__lootSend({kind:'select-target',target:null});
          window.__lootStep(1);
        }""")
        self.page.get_by_role("button", name="Loot", exact=True).click()
        self.page.wait_for_function("window.__lootSent.some(c => c.kind === 'select-target' && c.target?.id === 108)")
        self.page.evaluate("window.__lootStep(1)")
        panel = self.page.get_by_role("complementary", name="Corpse loot", exact=True)
        rewards = self.page.locator("#loot-rewards")
        claim = self.page.get_by_role("button", name="Claim rewards", exact=True)
        expect(panel).to_be_visible()
        expect(self.page.locator("#loot-title")).to_have_text("Timber Wolf")
        expect(rewards).to_have_text("2 copper · Torn Fur × 2")
        expect(self.page.locator("#copper-status")).to_have_text("Copper: 0")
        self.page.screenshot(path=str(ARTIFACTS / "loot-before-claim.png"))
        claim.click()
        expect(claim).to_be_disabled()
        self.page.wait_for_function("window.__lootSent.some(c => c.kind === 'loot')")
        # Drop the claim publication and its bag sheet: only the last of four
        # authoritative ticks reaches the frame. Copper/loot absence still recover.
        self.page.evaluate("window.__lootStep(4)")
        expect(rewards).to_have_text("No rewards remain on this corpse.")
        expect(self.page.locator("#copper-status")).to_have_text("Copper: 2")
        sent = self.page.evaluate("window.__lootSent.filter(c => c.kind === 'loot').length")
        claim.evaluate("button => button.click()")
        self.frames(2)
        self.assertEqual(self.page.evaluate("window.__lootSent.filter(c => c.kind === 'loot').length"), sent)
        self.page.get_by_role("button", name="Bags (B)", exact=True).click()
        expect(panel).to_be_hidden()
        slots = self.page.locator("#bag-slots button")
        expect(self.page.locator("#bag-status")).to_contain_text("Waiting")
        expect(slots.nth(0)).to_have_text("Slot 1 · Torn Fur × 3")
        self.page.evaluate("window.__lootStep(10 - Number(window.__lootLatest().tick % 10n))")
        expect(slots.nth(0)).to_have_text("Slot 1 · Torn Fur × 5")
        expect(slots.nth(0)).to_be_enabled()
        self.page.evaluate("""() => {
          window.__lootSend({kind:'loot',creatureId:108,diedAt:912n});
          window.__lootStep(1);
        }""")
        self.assertEqual(self.page.evaluate("window.__lootLatest().events.some(e => e.kind === 'error' && e.code === 'empty-loot')"), True)
        self.assertEqual(self.page.evaluate("window.__lootLatest().viewer.copper"), 2)
        self.page.get_by_role("button", name="Characters", exact=True).click()
        # The source is still paused: the new player's class choice waits for a tick.
        self.enter_world(ticking=False)
        expect(self.page.locator("#copper-status")).to_have_text("Copper: 0")
        expect(panel).to_be_hidden()
        expect(self.page.locator("#loot-feedback")).to_have_text("")

        # A received projection fixture covers the full-bag DOM/refusal layout.
        # It never changes the real host. Atomic bag refusal is covered in core;
        # the actual authority claim and missing publication were exercised above.
        self.page.evaluate("""() => {
          const before = window.__lootBefore;
          const current = window.__lootLatest();
          window.__lootFixture = {...before, viewerId:current.viewerId, tick:1000n, acknowledgedSequence:100,
            entities:before.entities.map(entity => entity.kind === 'player' && entity.entityId === before.viewerId
              ? {...entity, entityId:current.viewerId} : entity),
            inventoryRevision:100n, inventory:Array.from({length:16},()=>({itemId:2,quantity:1})), events:[]};
          window.__lootSource.latestProjection = () => window.__lootFixture;
        }""")
        self.page.get_by_role("button", name="Loot", exact=True).click()
        expect(rewards).to_have_text("2 copper · Torn Fur × 2")
        claim.click()
        expect(self.page.locator("#loot-feedback")).to_contain_text("bags are full")
        expect(rewards).to_have_text("2 copper · Torn Fur × 2")
        expect(self.page.locator("#copper-status")).to_have_text("Copper: 0")
        expect(claim).to_be_enabled()
        self.assertEqual(self.page.evaluate("window.__lootLatest().viewer.copper"), 0)
        for width, height in [(390,844),(640,360)]:
            self.page.set_viewport_size({"width":width,"height":height})
            box = panel.bounding_box()
            self.assertIsNotNone(box)
            self.assertGreaterEqual(box["x"],0)
            self.assertLessEqual(box["x"]+box["width"],width)
            self.assertLessEqual(box["y"]+box["height"],height)
            self.assertFalse(panel.evaluate("p => p.scrollWidth > p.clientWidth"))
            self.assertTrue(claim.evaluate("b => {const r=b.getBoundingClientRect();return b.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2));}"))
            self.assertTrue(self.page.get_by_role("button", name="Characters", exact=True).evaluate(
                "b => {const r=b.getBoundingClientRect();return b.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2));}"))
            self.page.screenshot(path=str(ARTIFACTS / f"loot-full-bag-{width}x{height}.png"))
        self.page.get_by_role("button", name="Close loot").focus()
        self.page.keyboard.press("Escape")
        expect(panel).to_be_hidden()
        expect(self.page.locator("#character-select")).to_be_hidden()
        self.page.evaluate("""() => {
          window.__lootFixture = null;
          window.__lootSource.latestProjection = window.__lootLatest;
        }""")
        self.page.get_by_role("button", name="Characters", exact=True).click()
        # Still paused: the class choice of this third entry waits for a tick.
        self.enter_world(ticking=False)
        expect(self.page.locator("#copper-status")).to_have_text("Copper: 0")
        expect(rewards).not_to_contain_text("Torn Fur")
        expect(self.page.locator("#loot-feedback")).to_have_text("")

    def test_projected_bags_split_merge_refusal_and_session_reset(self):
        self.open("?debug")
        self.enter_world()
        self.page.keyboard.press("b")
        panel = self.page.get_by_role("complementary", name="Bags", exact=True)
        expect(panel).to_be_visible()
        slots = self.page.locator("#bag-slots button")
        self.assertEqual(slots.count(), 16)
        expect(slots.nth(0)).to_have_text("Slot 1 · Torn Fur × 3")
        expect(slots.nth(1)).to_have_text("Slot 2 · Worn Dagger × 1")
        self.page.screenshot(path=str(ARTIFACTS / "bags-desktop.png"))
        quantity = self.page.get_by_role("spinbutton", name="Quantity")
        slots.nth(0).click()
        quantity.fill("2")
        slots.nth(15).click()
        expect(slots.nth(0)).to_have_text("Slot 1 · Torn Fur × 1")
        expect(slots.nth(15)).to_have_text("Slot 16 · Torn Fur × 2")
        slots.nth(15).click()
        quantity.fill("1")
        slots.nth(1).click()
        expect(self.page.locator("#bag-feedback")).to_contain_text("refused")
        expect(slots.nth(15)).to_have_text("Slot 16 · Torn Fur × 2")
        slots.nth(15).click()
        slots.nth(0).click()
        expect(slots.nth(0)).to_have_text("Slot 1 · Torn Fur × 3")
        expect(slots.nth(15)).to_have_text("Slot 16 · Empty")
        slots.nth(1).click()
        slots.nth(14).click()
        expect(slots.nth(14)).to_have_text("Slot 15 · Worn Dagger × 1")
        for width, height in [(390, 844), (640, 360)]:
            self.page.set_viewport_size({"width": width, "height": height})
            box = panel.bounding_box()
            self.assertIsNotNone(box)
            self.assertGreaterEqual(box["x"], 0)
            self.assertLessEqual(box["x"] + box["width"], width)
            self.assertLessEqual(box["y"] + box["height"], height)
            self.assertFalse(panel.evaluate("p => p.scrollWidth > p.clientWidth"))
            self.assertTrue(slots.nth(0).evaluate("b => { const r = b.getBoundingClientRect(); return b.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)); }"),
                            "The HUD must not cover bag controls on a short viewport")
            self.page.screenshot(path=str(ARTIFACTS / f"bags-{width}x{height}.png"))
        self.page.get_by_role("button", name="Close bags").click()
        expect(panel).to_be_hidden()
        self.page.get_by_role("button", name="Characters", exact=True).click()
        self.enter_world()
        self.page.get_by_role("button", name="Bags (B)", exact=True).click()
        expect(slots.nth(0)).to_have_text("Slot 1 · Torn Fur × 3")
        expect(slots.nth(1)).to_have_text("Slot 2 · Worn Dagger × 1")
        expect(slots.nth(14)).to_have_text("Slot 15 · Empty")
        expect(self.page.locator("#bag-feedback")).to_have_text("")
        self.page.get_by_role("button", name="Close bags").focus()
        self.page.keyboard.press("Escape")
        expect(panel).to_be_hidden()
        expect(self.page.locator("#character-select")).to_be_hidden()

    def test_character_pane_equip_unequip_and_layout(self):
        """Equip the starter Worn Dagger from the bags, read it in the Character pane, unequip it."""
        self.open("?debug")
        self.enter_world()
        self.page.keyboard.press("c")
        pane = self.page.get_by_role("complementary", name="Character", exact=True)
        expect(pane).to_be_visible()
        expect(self.page.get_by_role("button", name="Character (C)", exact=True)).to_have_attribute("aria-expanded", "true")
        equipment = pane.locator(".equipment-slot span")
        self.assertEqual(equipment.count(), 6)
        expect(equipment).to_have_text(["Main hand · Empty", "Off hand · Empty", "Head · Empty", "Chest · Empty", "Legs · Empty", "Feet · Empty"])
        expect(pane.locator("dd[data-stat=strength]")).to_have_text("0")
        expect(pane.locator("dd[data-stat=health]")).to_have_text(re.compile(r"^\d+ / \d+$"))
        damage = pane.locator("dd[data-stat=damage]")
        expect(damage).to_have_text(re.compile(r"^\d+–\d+$"))
        unequipped_damage = damage.text_content()
        self.page.screenshot(path=str(ARTIFACTS / "character-pane-empty.png"))

        self.page.locator("#world").focus()
        self.page.keyboard.press("b")
        slots = self.page.locator("#bag-slots button")
        expect(slots.nth(1)).to_have_text("Slot 2 · Worn Dagger × 1")
        expect(slots.nth(1)).to_have_attribute("title", "+2 Strength, +2 Agility")
        equip = self.page.locator("#bag-equip")
        expect(equip).to_be_hidden()
        slots.nth(0).click()
        expect(equip).to_be_hidden()
        slots.nth(0).click()
        slots.nth(1).click()
        expect(equip).to_have_text("Equip Worn Dagger")
        equip.click()
        expect(equipment.nth(0)).to_have_text("Main hand · Worn Dagger (+2 Strength, +2 Agility)")
        expect(slots.nth(1)).to_have_text("Slot 2 · Empty")
        expect(pane.locator("dd[data-stat=strength]")).to_have_text("2")
        expect(pane.locator("dd[data-stat=agility]")).to_have_text("2")
        expect(damage).not_to_have_text(unequipped_damage)
        expect(self.page.locator("#character-feedback")).to_have_text("Equipment updated.")
        self.page.screenshot(path=str(ARTIFACTS / "character-pane-equipped.png"))

        self.page.get_by_role("button", name="Unequip Worn Dagger").click()
        expect(equipment.nth(0)).to_have_text("Main hand · Empty")
        expect(slots.nth(1)).to_have_text("Slot 2 · Worn Dagger × 1")
        expect(pane.locator("dd[data-stat=strength]")).to_have_text("0")
        expect(damage).to_have_text(unequipped_damage)
        self.page.get_by_role("button", name="Close bags").click()

        for width, height in [(390, 844), (640, 360)]:
            self.page.set_viewport_size({"width": width, "height": height})
            box = pane.bounding_box()
            self.assertIsNotNone(box)
            self.assertGreaterEqual(box["x"], 0)
            self.assertLessEqual(box["x"] + box["width"], width)
            self.assertLessEqual(box["y"] + box["height"], height)
            self.assertFalse(pane.evaluate("p => p.scrollWidth > p.clientWidth"))
            close = self.page.get_by_role("button", name="Close character")
            self.assertTrue(close.evaluate("b => { const r = b.getBoundingClientRect(); return b.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)); }"),
                            "The HUD must not cover the character pane on a small viewport")
            self.page.screenshot(path=str(ARTIFACTS / f"character-pane-{width}x{height}.png"))
        self.page.get_by_role("button", name="Close character").focus()
        self.page.keyboard.press("Escape")
        expect(pane).to_be_hidden()
        expect(self.page.locator("#character-select")).to_be_hidden()

    def test_vendor_window_sells_and_buys_at_bram_tolliver(self):
        """Walk the vendor-resume scenario's route to the inn, sell the starter bag and buy the dagger back."""
        self.open("?debug")
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          window.__vendorAdvance = source.advance;
          source.advance = () => [];
        }""")
        self.enter_world(ticking=False)
        self.page.keyboard.press("v")
        panel = self.page.get_by_role("complementary", name="Vendor", exact=True)
        expect(panel).to_be_visible()
        expect(self.page.locator("#vendor-status")).to_contain_text("Move within 5 m of Innkeeper Bram Tolliver")
        self.page.screenshot(path=str(ARTIFACTS / "vendor-out-of-reach.png"))
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          const moves = new Map([[0,[1,16384]],[145,[1,32768]],[150,[0,32768]]]);
          for (let tick = 0; tick < 155; tick++) {
            const move = moves.get(tick);
            if (move) source.sendCommand({kind:'move',forward:move[0],strafe:0,facing:move[1]});
            window.__vendorAdvance.call(source, 1 / 30);
          }
          window.__vendorQueue = [];
          source.advance = () => window.__vendorQueue.splice(0);
          window.__vendorStep = ticks => {
            for (let tick = 0; tick < ticks; tick++) window.__vendorQueue.push(...window.__vendorAdvance.call(source, 1 / 30));
          };
        }""")
        def step(ticks=2):
            self.frames(2)
            self.page.evaluate("ticks => window.__vendorStep(ticks)", ticks)
            self.frames(3)
        step()
        expect(self.page.get_by_role("heading", name="Innkeeper Bram Tolliver")).to_be_visible()
        expect(self.page.locator("#vendor-status")).to_have_text("Copper: 0")
        offers = self.page.locator("#vendor-offers li")
        self.assertEqual(offers.count(), 8)
        expect(offers.nth(0)).to_contain_text("Worn Dagger · 5 copper (+2 Strength, +2 Agility)")
        expect(self.page.get_by_role("button", name="Buy Worn Dagger for 5 copper")).to_be_disabled()
        sales = self.page.locator("#vendor-sales li")
        expect(sales).to_have_count(2)
        expect(sales.nth(0)).to_contain_text("Torn Fur × 3 · 3 copper")
        self.page.screenshot(path=str(ARTIFACTS / "vendor-desktop.png"))
        self.page.get_by_role("button", name="Sell Torn Fur × 3 for 3 copper").click()
        step()
        expect(self.page.locator("#vendor-feedback")).to_have_text("Sold.")
        expect(self.page.locator("#vendor-status")).to_have_text("Copper: 3")
        expect(sales).to_have_count(1)
        self.page.get_by_role("button", name="Sell Worn Dagger × 1 for 2 copper").click()
        step()
        expect(self.page.locator("#vendor-status")).to_have_text("Copper: 5")
        expect(sales).to_have_count(0)
        self.page.get_by_role("button", name="Buy Worn Dagger for 5 copper").click()
        step()
        expect(self.page.locator("#vendor-feedback")).to_have_text("Purchased.")
        expect(self.page.locator("#vendor-status")).to_have_text("Copper: 0")
        expect(sales.nth(0)).to_contain_text("Worn Dagger × 1 · 2 copper")
        expect(self.page.locator("#copper-status")).to_have_text("Copper: 0")
        for width, height in [(390, 844), (640, 360)]:
            self.page.set_viewport_size({"width": width, "height": height})
            box = panel.bounding_box()
            self.assertIsNotNone(box)
            self.assertGreaterEqual(box["x"], 0)
            self.assertLessEqual(box["x"] + box["width"], width)
            self.assertLessEqual(box["y"] + box["height"], height)
            self.assertFalse(panel.evaluate("p => p.scrollWidth > p.clientWidth"))
            close = self.page.get_by_role("button", name="Close vendor")
            self.assertTrue(close.evaluate("b => { const r = b.getBoundingClientRect(); return b.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)); }"),
                            "The HUD must not cover the vendor window on a small viewport")
            self.page.screenshot(path=str(ARTIFACTS / f"vendor-{width}x{height}.png"))
        self.page.get_by_role("button", name="Close vendor").focus()
        self.page.keyboard.press("Escape")
        expect(panel).to_be_hidden()
        expect(self.page.locator("#character-select")).to_be_hidden()

    def test_quest_dialog_log_tracker_and_marker_at_the_marshal(self):
        """Walk the quest-resume scenario's route to the Marshal, accept, track and abandon Trouble in the Woods."""
        self.open("?debug")
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          window.__questAdvance = source.advance;
          source.advance = () => [];
        }""")
        self.enter_world(ticking=False)
        markers = self.page.locator("#quest-markers .quest-marker")
        self.page.keyboard.press("t")
        dialog = self.page.get_by_role("complementary", name="Quest dialog", exact=True)
        expect(dialog).to_be_visible()
        expect(self.page.locator("#quest-dialog-status")).to_contain_text("Move within 5 m of Marshal Elden Greywatch")
        expect(self.page.get_by_role("button", name="Accept: Trouble in the Woods")).to_be_disabled()
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          const moves = new Map([[0,[1,49152]],[30,[0,49152]]]);
          for (let tick = 0; tick < 32; tick++) {
            const move = moves.get(tick);
            if (move) source.sendCommand({kind:'move',forward:move[0],strafe:0,facing:move[1]});
            window.__questAdvance.call(source, 1 / 30);
          }
          window.__questQueue = [];
          source.advance = () => window.__questQueue.splice(0);
          window.__questStep = ticks => {
            for (let tick = 0; tick < ticks; tick++) window.__questQueue.push(...window.__questAdvance.call(source, 1 / 30));
          };
        }""")
        def step(ticks=2):
            self.frames(2)
            self.page.evaluate("ticks => window.__questStep(ticks)", ticks)
            self.frames(3)
        # Look north-west over the character at the Marshal so his marker is on screen.
        self.page.evaluate("window.__valeDebug.flyToPose([-21.8, 3.5, 17], [-22, 1.5, 9])")
        step(10)
        expect(self.page.get_by_role("heading", name="Marshal Elden Greywatch")).to_be_visible()
        expect(markers.first).to_have_attribute("data-marker", "available")
        expect(markers.first).to_have_text("!")
        card = self.page.locator("#quest-dialog-quests .quest-card")
        expect(card).to_have_count(1)
        expect(card.first).to_contain_text("Timber Wolf slain: 0/6")
        expect(card.first).to_contain_text("Rewards: 150 XP · 10 copper")
        self.page.screenshot(path=str(ARTIFACTS / "quest-dialog.png"))
        self.page.get_by_role("button", name="Accept: Trouble in the Woods").click()
        step()
        expect(self.page.locator("#quest-dialog-feedback")).to_have_text("Accepted: Trouble in the Woods.")
        expect(self.page.locator("#objective")).to_contain_text("Timber Wolf slain: 0/6")
        expect(markers.first).to_have_attribute("data-marker", "in-progress")
        expect(card.first).to_have_attribute("data-state", "in-progress")
        self.page.keyboard.press("Escape")
        expect(dialog).to_be_hidden()
        self.page.evaluate("window.__valeDebug.follow()")
        self.page.locator("#world").focus()
        self.page.keyboard.press("l")
        log = self.page.get_by_role("complementary", name="Quest log", exact=True)
        expect(log).to_be_visible()
        expect(self.page.locator("#quest-log-quests .quest-card")).to_contain_text("Trouble in the Woods")
        self.page.screenshot(path=str(ARTIFACTS / "quest-log.png"))
        for width, height in [(390, 844), (640, 360)]:
            self.page.set_viewport_size({"width": width, "height": height})
            box = log.bounding_box()
            self.assertIsNotNone(box)
            self.assertGreaterEqual(box["x"], 0)
            self.assertLessEqual(box["x"] + box["width"], width)
            self.assertFalse(log.evaluate("p => p.scrollWidth > p.clientWidth"))
            self.page.screenshot(path=str(ARTIFACTS / f"quest-log-{width}x{height}.png"))
        self.page.set_viewport_size({"width": 1280, "height": 800})
        self.page.get_by_role("button", name="Abandon: Trouble in the Woods").click()
        step()
        expect(self.page.locator("#quest-log-feedback")).to_have_text("Abandoned: Trouble in the Woods.")
        expect(self.page.locator("#quest-log-quests")).to_contain_text("Your quest log is empty.")
        expect(self.page.locator("#objective")).to_contain_text("No active quests")
        expect(markers.first).to_have_attribute("data-marker", "available")
        self.page.get_by_role("button", name="Close quest log").focus()
        self.page.keyboard.press("Escape")
        expect(log).to_be_hidden()
        expect(self.page.locator("#character-select")).to_be_hidden()

    def test_zone_chat_frame_says_yells_emotes_and_releases_held_movement(self):
        """Enter opens the chat field, which releases held movement; lines and emotes come back from the zone."""
        self.open("?debug")
        self.enter_world()
        chat = self.page.get_by_role("region", name="Chat", exact=True)
        expect(chat).to_be_visible()
        field = self.page.get_by_role("textbox", name="Chat message")
        self.page.keyboard.down("KeyW")
        self.frames(10)
        self.page.keyboard.press("Enter")
        expect(field).to_be_focused()
        self.frames(4)
        before = self.self_position()
        self.frames(20)
        after = self.self_position()
        self.assertLess(math.hypot(after[0] - before[0], after[1] - before[1]), 0.05,
                        "typing in the chat field must not keep running")
        self.page.keyboard.up("KeyW")
        field.fill("Hail, Greyhaven!")
        self.page.keyboard.press("Enter")
        log = self.page.get_by_role("log", name="Chat messages")
        expect(log).to_contain_text("You say: Hail, Greyhaven!")
        expect(field).not_to_be_focused()
        self.page.keyboard.press("Enter")
        field.fill("/y Again")
        self.page.keyboard.press("Enter")
        expect(log).to_contain_text("You can speak or emote once per second.")
        self.page.keyboard.press("Enter")
        field.fill("/dance")
        self.page.keyboard.press("Enter")
        expect(log).to_contain_text("Use /s to say, /y to yell, or /wave, /bow, /cheer, /laugh, /point.")
        # An emote shares the once-per-second limit; after it, /wave comes back from the zone.
        self.frames(35)
        self.page.keyboard.press("Enter")
        field.fill("/wave")
        self.page.keyboard.press("Enter")
        expect(log).to_contain_text("You wave.")
        expect(log.locator(".chat-emote")).to_have_count(1)
        self.page.screenshot(path=str(ARTIFACTS / "chat-desktop.png"))
        self.page.keyboard.press("Enter")
        self.page.keyboard.press("Escape")
        expect(field).not_to_be_focused()
        expect(self.page.locator("#character-select")).to_be_hidden()
        for width, height in [(390, 844), (640, 360)]:
            self.page.set_viewport_size({"width": width, "height": height})
            self.page.locator("#world").focus()
            self.page.keyboard.press("Enter")
            expect(field).to_be_focused()
            box = chat.bounding_box()
            self.assertIsNotNone(box)
            self.assertGreaterEqual(box["x"], 0)
            self.assertLessEqual(box["x"] + box["width"], width)
            self.assertLessEqual(box["y"] + box["height"], height)
            self.page.screenshot(path=str(ARTIFACTS / f"chat-{width}x{height}.png"))
            self.page.keyboard.press("Escape")

    def test_character_creation_classes_sex_and_roster_persistence(self):
        self.open()
        self.page.get_by_role("button", name="Create character", exact=True).click()
        expect(self.page.get_by_role("heading", name="Create character")).to_be_visible()
        class_radios = self.page.get_by_role("group", name="Class").get_by_role("radio")
        self.assertEqual(class_radios.count(), 3)
        expect(self.page.get_by_role("radio", name=re.compile("^Warden"))).to_be_checked()
        expect(self.page.get_by_role("radio", name="Male", exact=True)).to_be_checked()

        self.page.get_by_label("Name").fill("Lyra Vale")
        self.page.get_by_role("radio", name=re.compile("^Ranger")).check()
        self.page.get_by_role("radio", name="Female").check()
        expect(self.page.locator("#character-subtitle")).to_contain_text("Female Human Ranger")
        expect(self.page.locator("#equipment-list")).to_contain_text("Ashwood Longbow")
        self.frames()
        self.page.screenshot(path=str(ARTIFACTS / "character-creation-female-ranger.png"))
        self.page.get_by_role("button", name="Create character", exact=True).filter(visible=True).click()

        roster_buttons = self.page.locator("#character-roster")
        expect(roster_buttons.get_by_role("button", name=re.compile("Lyra Vale"))).to_be_visible()
        expect(self.page.locator("#character-name")).to_have_text("Lyra Vale")
        roster = self.page.evaluate("key => JSON.parse(localStorage.getItem(key))", ROSTER_KEY)
        self.assertEqual(roster["characters"], [{
            "id": "local-1", "name": "Lyra Vale", "classId": "ranger", "sex": "female",
        }])

        self.enter_world()
        self.frames(8)
        self.page.screenshot(path=str(ARTIFACTS / "character-ranger-world.png"))
        self.page.keyboard.press("Escape")
        expect(self.page.locator("#character-name")).to_have_text("Lyra Vale")
        self.assertEqual(self.legacy_checkpoints(), [], "Entering the world must not write any progress save")
        roster_buttons.get_by_role("button", name=re.compile("Aelric Stormward")).click()
        expect(self.page.locator("#character-name")).to_have_text("Aelric Stormward")
        roster_buttons.get_by_role("button", name=re.compile("Lyra Vale")).click()

        self.page.reload()
        self.page.wait_for_function("window.__calls.draws > 0")
        roster_buttons = self.page.locator("#character-roster")
        expect(roster_buttons.get_by_role("button", name=re.compile("Lyra Vale"))).to_be_visible()
        self.page.get_by_role("button", name="Create character", exact=True).click()
        self.page.get_by_label("Name").fill("Dorian Voss")
        self.page.get_by_role("radio", name=re.compile("^Arcanist")).check()
        expect(self.page.get_by_role("radio", name="Male", exact=True)).to_be_checked()
        expect(self.page.locator("#character-subtitle")).to_contain_text("Male Human Arcanist")
        expect(self.page.locator("#equipment-list")).to_contain_text("Emberglass Staff")
        self.page.get_by_role("button", name="Create character", exact=True).filter(visible=True).click()
        expect(self.page.locator("#character-roster").get_by_role("button", name=re.compile("Dorian Voss"))).to_be_visible()
        self.page.screenshot(path=str(ARTIFACTS / "character-creation-roster.png"))

    def assert_selection_layout(self):
        selectors = [".selection-brand", "#preview-surface", ".turntable-controls", ".roster-panel", ".selection-actions", ".character-details"]
        boxes = {selector: self.page.locator(selector).bounding_box() for selector in selectors}
        (ARTIFACTS / f"{self._testMethodName}-layout.json").write_text(json.dumps(boxes, indent=2))
        for i, name in enumerate(selectors):
            a = boxes[name]
            for other in selectors[i + 1:]:
                b = boxes[other]
                overlap = min(a["x"] + a["width"], b["x"] + b["width"]) > max(a["x"], b["x"]) + 1 and min(a["y"] + a["height"], b["y"] + b["height"]) > max(a["y"], b["y"]) + 1
                self.assertFalse(overlap, f"Selection panels overlap: {name} {a}, {other} {b}")
        self.assertFalse(self.page.locator("#character-select").evaluate("el => el.scrollWidth > el.clientWidth + 1"))

    def test_selection_layout_at_narrow_short_and_tablet_sizes(self):
        self.open()
        for width, height in [(320, 568), (844, 390), (900, 700), (1280, 600)]:
            with self.subTest(width=width, height=height):
                self.page.set_viewport_size({"width": width, "height": height})
                self.frames()
                self.assert_selection_layout()
                self.page.screenshot(path=str(ARTIFACTS / f"selection-{width}x{height}.png"))


    def hud_number(self, selector, pattern):
        text = self.page.locator(selector).inner_text()
        match = re.search(pattern, text)
        self.assertIsNotNone(match, f"{selector} shows {pattern}: {text!r}")
        return int(match.group(1))

    def enter_beside_wolf(self):
        """Enters the world with the source paused and walks the deterministic route of the corpse-loot
        acceptance toward Timber Wolf 108 (selected at tick 380). The source then stays paused: the
        page's own frames publish only the ticks the test runs with `step`, so the keyboard and HUD
        are exercised at a known tick count however slowly headless Chromium renders."""
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          window.__classAdvance = source.advance;
          source.advance = () => [];
        }""")
        self.enter_world(ticking=False)
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          const moves = new Map([[0,[1,0]],[36,[1,49152]],[121,[1,32768]],
            [131,[1,49152]],[375,[1,32768]],[395,[0,32768]]]);
          for (let tick = 0; tick < 400; tick++) {
            const move = moves.get(tick);
            if (move) source.sendCommand({kind:'move',forward:move[0],strafe:0,facing:move[1]});
            if (tick === 380) source.sendCommand({kind:'select-target',target:{kind:'creature',id:108}});
            window.__classAdvance.call(source, 1 / 30);
          }
          // Combat text fades after 1.4 s of wall-clock time, which a slow headless frame can outlast,
          // so count the entries as they are added.
          window.__combatTexts = [];
          new MutationObserver(records => {
            for (const record of records) for (const node of record.addedNodes) {
              if (node.classList?.contains('combat-text-entry')) window.__combatTexts.push(node.textContent);
            }
          }).observe(document.body, {childList: true, subtree: true});
          window.__classQueue = [];
          source.advance = () => window.__classQueue.splice(0);
          window.__classStep = ticks => {
            for (let tick = 0; tick < ticks; tick++) window.__classQueue.push(...window.__classAdvance.call(source, 1 / 30));
          };
        }""")
        self.frames(2)
        self.expect_class_resource()
        expect(self.page.locator("[data-part=target]")).to_be_visible()
        expect(self.page.locator("[data-part=target-name]")).to_contain_text("Timber Wolf")

    def step(self, ticks):
        """Lets the page send its pending intents, runs `ticks` zone ticks and lets the page present them."""
        self.frames(2)
        self.page.evaluate("ticks => window.__classStep(ticks)", ticks)
        self.frames(3)

    def create_and_select(self, name, class_label):
        self.page.get_by_role("button", name="Create character", exact=True).click()
        self.page.get_by_label("Name").fill(name)
        self.page.get_by_role("radio", name=re.compile(f"^{class_label}")).check()
        self.page.get_by_role("button", name="Create character", exact=True).filter(visible=True).click()
        expect(self.page.locator("#character-name")).to_have_text(name)

    def assert_action_bar(self, locked_levels):
        slots = self.page.locator("#class-hud .action-slot")
        expect(slots).to_have_count(4)
        self.assertEqual(slots.locator(".slot-key").all_inner_texts(), ["1", "2", "3", "4"])
        states = slots.evaluate_all("els => els.map(el => el.dataset.state)")
        self.assertEqual(states[0] in ("ready", "cooldown"), True, f"Slot 1 is learned at level 1: {states}")
        self.assertEqual(states[1:], ["locked"] * 3, f"Higher abilities are not learned yet: {states}")
        self.assertEqual(slots.locator(".slot-unlock").all_inner_texts()[1:], locked_levels)

    def test_warden_action_bar_rage_and_combat_text(self):
        """A level-1 Warden fights a wolf: auto-attack builds rage, key 1 spends it on Heroic Strike."""
        self.open("?debug")
        self.enter_beside_wolf()
        self.assert_action_bar(["Lv 2", "Lv 4", "Lv 6"])
        expect(self.page.locator("[data-part=player-resource-text]")).to_contain_text(re.compile(r"Rage \d+ / 100"))
        self.page.keyboard.press("KeyF")
        # The wolf closes in and two swings land within four seconds: hits build rage and combat
        # text floats up for each damage event.
        self.step(120)
        self.assertGreaterEqual(self.hud_number("[data-part=player-resource-text]", r"Rage (\d+)"), 15)
        self.assertTrue(self.page.evaluate("window.__combatTexts.length"), "Combat text floated up")
        before = self.hud_number("[data-part=player-resource-text]", r"Rage (\d+)")
        self.page.keyboard.press("Digit1")
        self.step(1)
        self.page.screenshot(path=str(ARTIFACTS / "class-kit-warden.png"))
        self.assertLess(self.hud_number("[data-part=player-resource-text]", r"Rage (\d+)"), before)
        self.page.locator("#class-hud .action-slot").first.hover()
        expect(self.page.locator("[data-part=tooltip]")).to_contain_text("Heroic Strike")
        expect(self.page.locator("[data-part=tooltip]")).to_contain_text("Melee range")
        self.page.screenshot(path=str(ARTIFACTS / "class-kit-warden-tooltip.png"))
        # Frames, dock and the corner HUD on a portrait phone and a short landscape screen.
        for width, height in [(390, 844), (844, 390)]:
            self.page.set_viewport_size({"width": width, "height": height})
            self.frames(3)
            self.page.screenshot(path=str(ARTIFACTS / f"class-kit-warden-{width}x{height}.png"))

    def test_arcanist_action_bar_cast_bar_and_combat_text(self):
        """A level-1 Arcanist targets a wolf and casts Firebolt with key 1: mana is spent, the cast bar
        fills, damage text floats up and the target frame shows the wolf."""
        self.open("?debug")
        self.create_and_select("Dorian Voss", "Arcanist")
        self.enter_beside_wolf()
        self.assert_action_bar(["Lv 2", "Lv 4", "Lv 6"])
        expect(self.page.locator("[data-part=player-resource-text]")).to_contain_text("Mana 110 / 110")
        self.page.keyboard.press("Digit1")
        self.step(3)
        cast = self.page.locator("[data-part=cast]")
        expect(cast).to_be_visible()
        expect(self.page.locator("[data-part=cast-name]")).to_have_text("Firebolt")
        self.assertEqual(self.page.locator("#class-hud .action-slot").first.get_attribute("data-state"), "casting")
        expect(self.page.locator("#class-hud .action-slot").first).to_have_attribute("data-gcd", "true")
        self.page.screenshot(path=str(ARTIFACTS / "class-kit-arcanist-cast.png"))
        # The two-second cast completes: mana is spent, the bolt lands and the cast bar clears.
        self.step(60)
        self.assertLess(self.hud_number("[data-part=player-resource-text]", r"Mana (\d+)"), 110)
        self.assertTrue(self.page.evaluate("window.__combatTexts.length"), "Combat text floated up")
        self.page.screenshot(path=str(ARTIFACTS / "class-kit-arcanist-hit.png"))
        expect(cast).to_be_hidden()


if __name__ == "__main__":
    unittest.main(verbosity=2)
