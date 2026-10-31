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
CHARACTER_ENTRY_PREFIX = "mmorpg.character.v1."
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
          // Pose evidence needs actual submitted identities/transforms, not repeated immutable mesh buffers.
          // An imported Quaternius model contributes its actual clip, clip time, root and bone world positions.
          window.__acceptanceNodes = identity => {
            const nodes = window.__valeDebug.unitNodes(identity).map(({id,transform,modelMatrix}) => ({id,transform,modelMatrix}));
            const model = window.__valeDebug.unitModelPose(identity);
            return model ? [...nodes, {id: `${identity}-model`, model}] : nodes;
          };
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

    def enter_world(self, ticking=True, fresh=True):
        """Enters the world. A fresh character starts at level 1 without XP; a
        returning one (`fresh=False`) resumes its stored progress. Tests that
        pause the source check the class resource after their own ticks."""
        self.enter_button().click()
        expect(self.page.locator("#character-select")).to_be_hidden()
        expect(self.page.locator(".hud")).to_be_visible()
        if fresh:
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

    def click_world_setup(self, query="?debug"):
        """Keep the real zone deterministic while physical pointer events drive the page."""
        self.open(query)
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
          // NPC torsos occupy roughly 0.94–1.50 m above their feet. Aim at
          // solid chest geometry, above the central gap between their legs.
          const y = debug.reliefAt(unit.x * 100, unit.z * 100) / 100 + (kind === 'npc' ? 1.2 : 0.5);
          debug.flyToPose([unit.x, y + 1, unit.z + 5], [unit.x, y, unit.z]);
          return {x: unit.x, y, z: unit.z};
        }""", [kind, identity])
        self.frames(4)
        self.assertTrue(self.page.evaluate("([kind,id]) => window.__valeDebug.unitNodeIds(`unit-${kind}-${id}`).length > 0", [kind, identity]))
        box = self.page.locator("#world").bounding_box()
        return box["x"] + box["width"] / 2, box["y"] + box["height"] / 2, point

    def test_settings_profiles_navigation_mobile_and_local_pause(self):
        self.open("?debug")
        self.page.locator("#selection-settings").click()
        dialog = self.page.locator("#settings-dialog")
        expect(dialog).to_be_visible()
        expect(self.page.locator("#settings-export-user")).to_be_visible()
        self.page.locator("#settings-search").fill("animation")
        dialog.get_by_role("button", name="Reduce motion", exact=True).click()
        motion = self.page.locator("#setting-accessibility-reduce_motion")
        expect(motion).to_be_focused()
        motion.select_option("full")
        self.page.set_viewport_size({"width": 360, "height": 780})
        expect(self.page.locator("#settings-categories")).to_be_hidden()
        selector = self.page.locator("#settings-category-select")
        expect(selector).to_be_visible()
        selector.select_option("settings.category.controls")
        self.page.locator("#setting-controls-invert_vertical_look").check()
        before = self.page.evaluate("localStorage.getItem('mmorpg.settings.user.v1')")
        with self.page.expect_download() as downloaded:
            self.page.locator("#settings-export-user").click()
        exported = json.loads(Path(downloaded.value.path()).read_text())
        self.assertEqual(exported["scope"], "user")
        self.assertTrue(exported["overrides"]["controls.invert_vertical_look"]["value"])
        profile = ROOT / "fixtures" / "settings" / "user-profile-v2.json"
        self.page.locator("#settings-import-user").set_input_files(profile)
        expect(dialog.get_by_role("heading", name="Preview User preferences replacement")).to_be_visible()
        self.assertEqual(self.page.evaluate("localStorage.getItem('mmorpg.settings.user.v1')"), before)
        dialog.get_by_role("button", name="Cancel import", exact=True).click()
        self.assertEqual(self.page.evaluate("localStorage.getItem('mmorpg.settings.user.v1')"), before)
        self.page.locator("#settings-import-user").set_input_files(profile)
        dialog.get_by_role("button", name="Replace User preferences", exact=True).click()
        expect(self.page.locator("#settings-status")).to_have_text("User preferences replaced and saved.")
        self.assertEqual(json.loads(self.page.evaluate("localStorage.getItem('mmorpg.settings.user.v1')")), json.loads(profile.read_text()))
        self.page.once("dialog", lambda confirmation: confirmation.accept())
        self.page.locator("#settings-reset-category").click()
        saved = json.loads(self.page.evaluate("localStorage.getItem('mmorpg.settings.user.v1')"))["overrides"]
        self.assertNotIn("controls.invert_vertical_look", saved)
        self.assertNotIn("controls.look_sensitivity", saved)
        self.assertEqual(saved["camera.field_of_view"]["value"], 70)
        self.assertIn("other_platform.preference", saved)
        self.assertTrue(dialog.evaluate("element => element.scrollWidth <= element.clientWidth + 1"), "The 360px Settings dialog must not overflow horizontally")
        self.page.screenshot(path=str(ARTIFACTS / "settings-mobile.png"))
        self.page.set_viewport_size({"width": 1440, "height": 900})
        self.page.locator("#settings-close").click()
        expect(self.page.locator("#selection-settings")).to_be_focused()
        self.enter_world()
        self.page.keyboard.down("KeyW")
        self.frames(4)
        self.page.locator("#world-settings").click()
        tick = self.page.evaluate("String(window.__valeDebug.worldSource().latestProjection().tick)")
        self.frames(12)
        self.assertEqual(self.page.evaluate("String(window.__valeDebug.worldSource().latestProjection().tick)"), tick, "Settings pauses the local zone")
        self.page.keyboard.up("KeyW")
        self.page.screenshot(path=str(ARTIFACTS / "settings-world.png"))
        self.page.locator("#settings-close").click()
        self.frames(12)
        velocity = self.page.evaluate("""() => {
          const snapshot = window.__valeDebug.worldSource().latestProjection();
          const player = snapshot.entities.find(entity => entity.kind === 'player' && entity.entityId === snapshot.viewerId);
          return [player.velocity[0], player.velocity[2]];
        }""")
        self.assertEqual(velocity, [0, 0], "Movement held before Settings does not resurrect after close")

    def test_frame_profile_only_reads_clocks(self):
        """#546: `?profile` times frame phases; the same stepped route ends in the same zone state, and keys still move."""
        ends = []
        for query in ("?debug", "?debug&profile"):
            if ends:
                # A fresh character again: leaving the world saved the first run's character.
                self.page.goto(self.url)
                self.page.evaluate("localStorage.clear()")
            self.click_world_setup(query)
            self.click_world_walk_to_woods()
            ends.append(self.page.evaluate("""() => {
              const snapshot = window.__valeDebug.worldSource().latestProjection();
              const viewer = snapshot.entities.find(entity => entity.kind === 'player' && entity.entityId === snapshot.viewerId);
              return {tick: String(snapshot.tick), position: viewer.position, facing: viewer.facing};
            }"""))
        self.assertEqual(ends[0], ends[1], "Profiling must not change the zone")
        # Real time again, so held W moves the character through the ordinary frame loop.
        self.page.evaluate("() => { window.__valeDebug.worldSource().advance = window.__clickAdvance; }")
        self.page.evaluate("window.__valeProfile.take()")
        self.page.keyboard.down("KeyW")
        self.frames(20)
        self.page.keyboard.up("KeyW")
        frames = self.page.evaluate("window.__valeProfile.take()")
        self.assertGreater(len(frames), 10)
        self.assertNotEqual(frames[0]["viewer"], frames[-1]["viewer"], "W still moves the character while profiling")
        self.assertTrue(all(frame["phases"]["render"] > 0 for frame in frames))

    def test_world_click_specific_creature_drag_empty_ground_and_alternatives(self):
        self.click_world_setup()
        self.click_world_walk_to_woods()
        creature = self.page.evaluate("window.__valeDebug.projectedUnits().find(unit => unit.kind === 'creature' && unit.family === 'wolf')")
        self.assertIsNotNone(creature, "The real Greyhaven projection must include a wolf")
        x, y, _ = self.aim_at_world_unit("creature", creature["id"])
        self.page.screenshot(path=str(ARTIFACTS / "world-click-wolf-before.png"))
        self.page.mouse.click(x, y, button="right")
        self.click_world_step()
        expected = {"kind": "creature", "id": creature["id"]}
        self.assertEqual(self.click_world_target(), expected, "Clicking the rendered wolf must select its exact authoritative identity")
        expect(self.page.locator("[data-part=target-name]")).to_contain_text("Timber Wolf")
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
        self.page.mouse.click(x, y, button="right")
        self.click_world_step()
        expect(self.page.get_by_role("complementary", name="Quest dialog", exact=True)).to_be_hidden()
        expect(self.page.get_by_role("complementary", name="Vendor", exact=True)).to_be_hidden()
        self.page.locator("#world").focus()
        self.page.keyboard.press("Tab")
        self.click_world_step()
        self.assertEqual(self.click_world_target()["kind"], "creature", "Tab remains an alternative")
        # A left click swings where the character faces (#517) and never selects.
        self.page.mouse.click(x, y)
        self.click_world_step()
        self.assertEqual(self.click_world_target()["kind"], "creature", "A swing keeps the selection")

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
                self.page.mouse.click(x, y, button="right")
                self.click_world_step()
                self.assertEqual(self.click_world_target(), {"kind": "npc", "id": identity})
                expect(self.page.locator("[data-part=target-name]")).to_contain_text(name)
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
                self.page.mouse.down(button="right")
                if cancellation == "pointercancel":
                    canvas.evaluate("el => el.dispatchEvent(new PointerEvent('pointercancel', {pointerId:window.__clickPointerId, bubbles:true}))")
                else:
                    canvas.evaluate("el => el.releasePointerCapture(window.__clickPointerId)")
                    self.page.mouse.move(x + 1, y)
                self.page.mouse.up(button="right")
                self.click_world_step()
                self.assertIsNone(self.click_world_target(), "A cancelled gesture cannot become selection")
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
        self.page.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2, button="right")
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
        self.page.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2, button="right")
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

        # Leaving commits and releases the durable character; re-entry resumes it where it left.
        self.page.get_by_role("button", name="Characters", exact=True).click()
        expect(self.page.locator("#character-select")).to_be_visible()
        self.assertEqual(self.page.evaluate("prefix => Object.keys(localStorage).filter(key => key.startsWith(prefix)).length",
                                            CHARACTER_ENTRY_PREFIX), 1, "Leaving stores the played character")
        expect(self.enter_button()).to_be_enabled()
        self.enter_world()

        self.page.screenshot(path=str(ARTIFACTS / "world-reentered.png"))
        self.assertFalse(self.wait_for_view(spawn, timeout_frames=20),
                         "Re-entering must resume the character where it left, not at the spawn")
        self.page.locator("#world").focus()
        self.page.keyboard.press("Escape")
        expect(self.page.locator("#character-select")).to_be_visible()
        self.assertEqual(self.legacy_checkpoints(), [])

    def test_rejected_projections_fail_closed_without_freezing_the_page(self):
        # Bytes 0-2 of a snapshot header are its protocol ID. While the flag is set the
        # strict decoder sees another ID in byte 2, as after a WASM/decoder skew.
        self.context.add_init_script("""(() => {
          window.__rejectSnapshots = false;
          const original = DataView.prototype.getUint8;
          DataView.prototype.getUint8 = function(offset, ...rest) {
            const value = original.call(this, offset, ...rest);
            return window.__rejectSnapshots && offset === 2 ? value ^ 0xff : value;
          };
        })();""")
        self.open()
        note = self.page.locator("#enter-world-note")

        # A refused entry keeps the page on selection, says why, and keeps rendering.
        self.page.evaluate("window.__rejectSnapshots = true")
        self.enter_button().click()
        expect(note).to_contain_text("Could not enter the world: Unsupported snapshot protocol")
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
        expect(note).to_contain_text("Left the world after an error: Unsupported snapshot protocol")
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

    def test_nature_candidate_at_the_same_woods_camera(self):
        """One real opt-in broadleaf; current/candidate use identical authority and camera."""
        self.open("?debug")
        self.page.evaluate("window.__valeDebug.worldSource().advance = () => []")
        self.enter_world(ticking=False)
        self.page.evaluate("""() => {
          window.__valeDebug.flyTo('woods');
          window.__valeDebug.setPresentationTime(120);
        }""")
        self.frames(6)
        self.page.add_style_tag(content="[data-world-ui] { visibility: hidden !important; }")
        before = self.debug_stats()["self"]
        projection = self.page.evaluate("window.__valeDebug.projection()")
        current = self.page.evaluate("window.__valeDebug.natureCandidate()")
        self.assertFalse(current["enabled"])
        self.assertFalse(current["loaded"])
        current_pixels = self.page.screenshot(path=str(ARTIFACTS / "nature-current.png"))
        candidate = self.page.evaluate("window.__valeDebug.setNatureCandidate(true)")
        self.assertTrue(candidate["loaded"])
        self.assertEqual(candidate["key"], "greyhaven.nature.tree.broadleaf/default")
        self.frames(6)
        candidate_pixels = self.page.screenshot(path=str(ARTIFACTS / "nature-candidate.png"))
        self.assertNotEqual(candidate_pixels, current_pixels, "The candidate must change the actual rendered image")
        self.assertEqual(self.page.evaluate("window.__valeDebug.projection()"), projection)
        self.assertEqual(self.debug_stats()["self"], before)
        self.assertGreater(self.canvas_colours()["distinct"], 400)
        self.page.evaluate("window.__valeDebug.setNatureCandidate(false)")
        self.frames(4)
        self.assertEqual(self.debug_stats()["self"], before)
        self.assertFalse(self.page.evaluate("window.__valeDebug.natureCandidate().enabled"))
        (ARTIFACTS / "nature-review.json").write_text(json.dumps({
            "camera": "woods", "presentationSeconds": 120,
            "viewport": {"width": 1440, "height": 900},
            "candidate": candidate, "self": before,
        }, indent=2))

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
        self.assertLessEqual(overlay_stats["staticNodes"], 1_500, "Static batching keeps draw calls bounded")
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
    # The hub spawn is (-15.5, 12.5) m. A building south of it (z ≈ 7.3) and the palisade west of
    # it (x ≈ -31.4) block the straight line to the animal spawn, so the route leaves north, heads
    # west past the palisade and then turns to the woods.
    ROUTE_TO_ANIMAL_SPAWN = ((-15.5, 19.0), (-40.0, 19.0), NEAREST_ANIMAL_SPAWN)

    def self_position(self):
        me = self.debug_stats()["self"]
        return me["x"], me["z"]

    def walk_until_animal_projected(self, max_legs=40, ticks_per_leg=15):
        """The authoritative player starts on the hub grid, outside the interest radius of the woods, so
        a creature is only projected once the player walks toward the nearest animal spawn.

        The zone's frame ticking pauses while the walk issues real move commands and advances real WASM
        ticks, re-aiming at the next route waypoint from the projected player position every tick, so
        the walk depends on neither frame timing nor the camera. Returns the nearest projected animal;
        on failure reports each leg's issued input, tick, position and progress."""
        self.page.evaluate("""route => {
          const source = window.__valeDebug.worldSource();
          window.__walk = {advance: source.advance, queue: [], route};
          source.advance = () => window.__walk.queue.splice(0);
        }""", [list(point) for point in self.ROUTE_TO_ANIMAL_SPAWN])
        log = []
        try:
            for _ in range(max_legs):
                leg = self.page.evaluate("""ticks => {
                  const source = window.__valeDebug.worldSource();
                  const {advance, queue, route} = window.__walk;
                  const selfAt = projection => {
                    const me = projection.entities.find(e => e.kind === 'player' && e.entityId === projection.viewerId);
                    return [me.position[0] / 100, me.position[2] / 100];
                  };
                  let latest = source.latestProjection();
                  const start = selfAt(latest);
                  let facing = null;
                  for (let i = 0; i < ticks; i++) {
                    const [x, z] = selfAt(latest);
                    // Drop reached waypoints; the last one, the spawn itself, stays the heading.
                    while (route.length > 1 && Math.hypot(route[0][0] - x, route[0][1] - z) < 1) route.shift();
                    // Core yaw: 65536 steps per turn, facing direction (sin, cos) in (x, z).
                    facing = Math.round(Math.atan2(route[0][0] - x, route[0][1] - z) / (2 * Math.PI) * 65536) & 0xffff;
                    source.sendCommand({kind: 'move', forward: 1, strafe: 0, facing});
                    const projections = advance.call(source, 1 / 30);
                    queue.push(...projections);
                    if (projections.length) latest = projections[projections.length - 1];
                  }
                  return {tick: Number(latest.tick), facing, waypoint: route[0], start, end: selfAt(latest)};
                }""", ticks_per_leg)
                self.frames(3)
                (sx, sz), (x, z) = leg["start"], leg["end"]
                moved = math.hypot(x - sx, z - sz)
                remaining = math.hypot(self.NEAREST_ANIMAL_SPAWN[0] - x, self.NEAREST_ANIMAL_SPAWN[1] - z)
                log.append(f"tick {leg['tick']}: move forward facing {leg['facing']} toward {leg['waypoint']},"
                           f" player ({sx:.2f}, {sz:.2f}) -> ({x:.2f}, {z:.2f}), moved {moved:.2f} m, {remaining:.2f} m from the spawn")
                animal = self.page.evaluate("([x, z]) => window.__valeDebug.nearestAnimal(x, z)", list(self.self_position()))
                if animal:
                    return animal
                if moved < 0.5:
                    log.append("stalled: the player made no progress over the last leg")
                    break
        finally:
            self.page.evaluate("() => { window.__valeDebug.worldSource().advance = window.__walk.advance; }")
        units = self.page.evaluate("window.__valeDebug.projectedUnits()")
        self.fail(f"No wolf, boar or rat was projected walking toward {self.NEAREST_ANIMAL_SPAWN}; projected: {units}\n" + "\n".join(log))

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
            self.assertFalse(parts <= {"body", "nose", "target-ring"}, f"{identity} regressed to the placeholder box")
            # The imported Quaternius model reports `-model`; until its GLB loads the procedural model draws instead.
            if "model" not in parts:
                self.assertGreaterEqual(len(parts), 8, f"{identity} must draw a multi-part model, got {sorted(parts)}")
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
        self.assertEqual(stats["scene"]["props"]["grass-tuft"], 3936)
        self.assertLessEqual(stats["scene"]["staticNodes"], 1_500)
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
            self.assertLessEqual(stats["scene"]["staticNodes"], 1_500)
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
          for (let tick = 0; tick < 972; tick++) {
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
        self.assertEqual(self.page.evaluate("Number(window.__lootLatest().loot.diedAt)"), 915)
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
          window.__lootSend({kind:'loot',creatureId:108,diedAt:915n});
          window.__lootStep(1);
        }""")
        self.assertEqual(self.page.evaluate("window.__lootLatest().events.some(e => e.kind === 'error' && e.code === 'empty-loot')"), True)
        self.assertEqual(self.page.evaluate("window.__lootLatest().viewer.copper"), 2)
        self.page.get_by_role("button", name="Characters", exact=True).click()
        # The stored character returns with its looted copper and a closed, empty loot window.
        self.enter_world(ticking=False, fresh=False)
        expect(self.page.locator("#copper-status")).to_have_text("Copper: 2")
        expect(panel).to_be_hidden()
        expect(self.page.locator("#loot-feedback")).to_have_text("")

        # A received projection fixture covers the full-bag DOM/refusal layout.
        # It never changes the real host. Atomic bag refusal is covered in core;
        # the actual authority claim and missing publication were exercised above.
        self.page.evaluate("""() => {
          const before = window.__lootBefore;
          const current = window.__lootLatest();
          window.__lootFixture = {...before, viewerId:current.viewerId, tick:1200n, acknowledgedSequence:100,
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
        self.assertEqual(self.page.evaluate("window.__lootLatest().viewer.copper"), 2)
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
        # The third entry also resumes the stored character, never the fixture.
        self.enter_world(ticking=False, fresh=False)
        expect(self.page.locator("#copper-status")).to_have_text("Copper: 2")
        expect(rewards).not_to_contain_text("Torn Fur")
        expect(self.page.locator("#loot-feedback")).to_have_text("")

    def test_projected_bags_split_merge_refusal_and_resume(self):
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
        # Re-entry resumes the stored bags.
        self.enter_world()
        self.page.get_by_role("button", name="Bags (B)", exact=True).click()
        expect(slots.nth(0)).to_have_text("Slot 1 · Torn Fur × 3")
        expect(slots.nth(1)).to_have_text("Slot 2 · Empty")
        expect(slots.nth(14)).to_have_text("Slot 15 · Worn Dagger × 1")
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

    def test_inn_journey_door_interior_and_vendor_at_bram_tolliver(self):
        """The inn journey (#158): walk to the inn, open its door with interact, walk in to Bram
        behind his bar, trade, walk out and close the door by clicking its leaf."""
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
        self.page.keyboard.press("Escape")
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          window.__vendorQueue = [];
          source.advance = () => window.__vendorQueue.splice(0);
          window.__vendorStep = ticks => {
            for (let tick = 0; tick < ticks; tick++) window.__vendorQueue.push(...window.__vendorAdvance.call(source, 1 / 30));
          };
          // Held moves keyed by tick, then the remaining ticks run; the vendor-resume scenario's route.
          window.__vendorWalk = (moves, ticks) => {
            for (let tick = 0; tick < ticks; tick++) {
              const move = moves[tick];
              if (move) source.sendCommand({kind:'move',forward:move[0],strafe:0,facing:move[1]});
              window.__vendorStep(1);
            }
          };
        }""")
        def step(ticks=2):
            self.frames(2)
            self.page.evaluate("ticks => window.__vendorStep(ticks)", ticks)
            self.frames(3)
        def walk(moves, ticks):
            self.page.evaluate("([moves, ticks]) => window.__vendorWalk(moves, ticks)", [{str(tick): move for tick, move in moves.items()}, ticks])
            self.frames(4)
        def door_open():
            return self.page.evaluate("window.__valeDebug.worldSource().latestProjection().doors")[0]["open"]
        # East along the plaza, then north to the closed front door.
        walk({0: [1, 16384], 145: [1, 32768], 176: [0, 32768]}, 178)
        self.assertFalse(door_open(), "The door is closed at zone start")
        for _ in range(60):
            if self.page.evaluate("window.__valeDebug.inn()?.loaded"):
                break
            self.frames(5)
        inn = self.page.evaluate("window.__valeDebug.inn()")
        self.assertEqual(sorted(inn["shown"]), sorted(["greyhaven.inn.shell", "greyhaven.inn.furniture", "greyhaven.inn.ceiling", "greyhaven.inn.roof", "greyhaven.inn.door"]))
        self.page.screenshot(path=str(ARTIFACTS / "inn-outside-closed.png"))
        # Interact (T) with no NPC selected swings the door in reach.
        self.page.locator("#world").focus()
        self.page.keyboard.press("t")
        step()
        self.assertTrue(door_open(), "Interact opened the inn door")
        self.frames(30)
        self.page.screenshot(path=str(ARTIFACTS / "inn-outside-open.png"))
        # Through the doorway, along the room and up to the bar.
        walk({0: [1, 32768], 29: [1, 16384], 49: [1, 32768], 57: [0, 32768]}, 60)
        inside = self.page.evaluate("window.__valeDebug.inn()")["shown"]
        self.assertNotIn("greyhaven.inn.roof", inside, "The roof hides while the character is inside")
        self.assertNotIn("greyhaven.inn.ceiling", inside)
        self.page.screenshot(path=str(ARTIFACTS / "inn-inside.png"))
        # From above with the roof hidden: Bram behind his bar, the player at the stools.
        self.page.evaluate("window.__valeDebug.flyToPose([15, 14, 0.01], [15, 0, 0])")
        self.frames(6)
        self.page.screenshot(path=str(ARTIFACTS / "inn-topdown.png"))
        self.page.evaluate("window.__valeDebug.follow()")
        self.page.locator("#world").focus()
        self.page.keyboard.press("v")
        expect(panel).to_be_visible()
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
        self.page.set_viewport_size({"width": 1440, "height": 900})
        # Back out through the doorway, then close the door by clicking its swung leaf.
        walk({0: [1, 0], 8: [1, 49152], 28: [1, 0], 60: [0, 0]}, 62)
        self.assertGreater(self.page.evaluate("window.__valeDebug.worldSource().latestProjection().entities[0].position[2]"), 480, "Outside again")
        relief = self.page.evaluate("window.__valeDebug.reliefAt(1500, 0)") / 100
        self.page.evaluate("([y]) => window.__valeDebug.flyToPose([15.6, y + 1.3, 7.5], [14.4, y + 1.1, 3.9])", [relief])
        self.frames(6)
        box = self.page.locator("#world").bounding_box()
        self.page.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2, button="right")
        step()
        self.assertFalse(door_open(), "Clicking the leaf closed the inn door")
        self.page.evaluate("window.__valeDebug.follow()")
        self.frames(30)
        self.page.screenshot(path=str(ARTIFACTS / "inn-closed-again.png"))

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
        selectors = [".selection-brand", "#preview-surface", ".turntable-controls", "#roster-panel", ".selection-actions"]
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
        expect(slots).to_have_count(len(locked_levels) + 1)
        self.assertEqual(slots.locator(".slot-key").all_inner_texts(), [str(i) for i in range(1, len(locked_levels) + 2)])
        states = slots.evaluate_all("els => els.map(el => el.dataset.state)")
        self.assertEqual(states[0] in ("ready", "cooldown"), True, f"Slot 1 is learned at level 1: {states}")
        self.assertEqual(states[1:], ["locked"] * len(locked_levels), f"Higher abilities are not learned yet: {states}")
        self.assertEqual(slots.locator(".slot-unlock").all_inner_texts()[1:], locked_levels)

    def test_warden_action_bar_rage_and_combat_text(self):
        """A level-1 Warden fights a wolf: auto-attack builds rage, key 1 spends it on Heroic Strike."""
        self.open("?debug")
        self.enter_beside_wolf()
        self.assert_action_bar(["Lv 2", "Lv 4", "Lv 6", "Lv 3"])
        self.page.wait_for_function("Array.from(document.querySelectorAll('.action-slot .slot-icon img')).length === 4 && Array.from(document.querySelectorAll('.action-slot .slot-icon img')).every(i => i.complete && i.naturalWidth > 0)")
        for identity in [1, 2, 3, 4]:
            image = self.page.locator(f'.action-slot[data-ability="{identity}"] .slot-icon img')
            expect(image).to_have_count(1)
            self.assertIn(f"warden-{identity}-64", image.evaluate("i => i.currentSrc"))
        expect(self.page.locator('.action-slot[data-ability="17"] .slot-icon svg')).to_have_count(1)
        self.assertEqual(self.page.locator('.action-slot').first.get_attribute('data-resource'), 'short')
        self.page.screenshot(path=str(ARTIFACTS / "warden-approved-desktop.png"))
        self.page.locator('.action-slot[data-ability="2"]').click(button="right")
        expect(self.page.locator("[data-part=tooltip]")).to_contain_text("Shield Bash")
        self.page.mouse.move(0, 0)
        expect(self.page.locator("[data-part=player-resource-text]")).to_contain_text(re.compile(r"Rage \d+ / 100"))
        self.page.keyboard.press("KeyF")
        # The wolf closes in and two swings land within four seconds: hits build rage and combat
        # text floats up for each damage event.
        self.step(120)
        for _ in range(20):
            if self.hud_number("[data-part=player-resource-text]", r"Rage (\d+)") >= 15:
                break
            self.step(10)
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

        # One real touch/high-DPI layout with the same approved artwork and long-press tooltip.
        mobile = self.browser.new_context(viewport={"width": 390, "height": 844}, device_scale_factor=2, is_mobile=True, has_touch=True, reduced_motion="reduce")
        try:
            page = mobile.new_page()
            page.on("pageerror", lambda error: self.errors.append(str(error)))
            page.goto(self.url + "?debug")
            page.get_by_role("button", name=re.compile("^Enter World")).click()
            expect(page.locator(".hud")).to_be_visible()
            page.wait_for_function("Array.from(document.querySelectorAll('.action-slot .slot-icon img')).length === 4 && Array.from(document.querySelectorAll('.action-slot .slot-icon img')).every(i => i.complete && i.naturalWidth > 0)")
            self.assertEqual(page.locator('.action-slot .slot-unlock').all_inner_texts()[1:], ["Lv 2", "Lv 4", "Lv 6", "Lv 3"])
            for identity in [1, 2, 3, 4]:
                self.assertIn(f"warden-{identity}-512", page.locator(f'.action-slot[data-ability="{identity}"] img').evaluate("i => i.currentSrc"))
            page.screenshot(path=str(ARTIFACTS / "warden-approved-mobile-2x.png"))
            button = page.locator('.action-slot[data-ability="3"]').bounding_box()
            touch = mobile.new_cdp_session(page)
            touch.send("Input.dispatchTouchEvent", {"type": "touchStart", "touchPoints": [{"x": button["x"] + button["width"] / 2, "y": button["y"] + button["height"] / 2}]})
            expect(page.locator("[data-part=tooltip]")).to_contain_text("Rallying Cry")
            touch.send("Input.dispatchTouchEvent", {"type": "touchEnd", "touchPoints": []})
            page.screenshot(path=str(ARTIFACTS / "warden-approved-touch-tooltip.png"))
        finally:
            mobile.close()

    def set_pad(self, trigger, stick_y):
        """Replaces the page's gamepads with one standard pad holding LT at `trigger` and the right
        stick at `stick_y` (W3C: negative is up); `trigger=None` disconnects it."""
        self.page.evaluate("""([trigger, stickY]) => {
          const pads = trigger === null ? [] : [{
            index: 0, connected: true, mapping: 'standard', id: 'smoke pad', timestamp: performance.now(),
            buttons: Array.from({length: 17}, (_, i) => ({pressed: i === 6 && trigger >= 0.5, touched: false, value: i === 6 ? trigger : 0})),
            axes: [0, 0, 0, stickY],
          }];
          Object.defineProperty(navigator, 'getGamepads', {configurable: true, value: () => pads});
        }""", [trigger, stick_y])

    def guard_label(self):
        return self.page.locator("[data-part=shield-guard] em")

    def test_warden_shield_guard_keyboard_pointer_and_gamepad(self):
        """A Warden beside a wolf holds Z and aims the guard with the pointer, then holds LT and aims
        with the right stick (#156); the HUD shows the zone's protecting lane and release lowers it."""
        self.open("?debug")
        self.enter_beside_wolf()
        guard = self.page.locator("[data-part=shield-guard]")
        expect(guard).to_be_hidden()
        box = self.page.locator("#world").bounding_box()
        x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.keyboard.down("KeyZ")
        self.step(2)
        expect(guard).to_be_visible()
        expect(self.guard_label()).to_have_text("Guarding middle")
        self.page.mouse.move(x, y - 40, steps=4)
        self.step(2)
        expect(self.guard_label()).to_have_text("Guarding high")
        self.page.screenshot(path=str(ARTIFACTS / "shield-guard-high.png"))
        self.page.keyboard.up("KeyZ")
        self.step(2)
        expect(guard).to_be_hidden()

        # Gamepad: holding LT raises the guard, the right stick picks the lane, release lowers it.
        self.set_pad(1.0, 0.0)
        self.step(2)
        expect(self.guard_label()).to_have_text("Guarding middle")
        self.set_pad(1.0, 0.9)
        self.step(2)
        expect(self.guard_label()).to_have_text("Guarding low")
        self.set_pad(1.0, -0.9)
        self.step(2)
        expect(self.guard_label()).to_have_text("Guarding high")
        self.page.screenshot(path=str(ARTIFACTS / "shield-guard-pad-high.png"))
        self.set_pad(0.0, -0.9)
        self.step(2)
        expect(guard).to_be_hidden()
        self.set_pad(None, 0.0)

    def aim_action_unit(self, kind, identity):
        """Elevated side view for action proof; legacy click-test camera defaults stay intact."""
        _, _, point = self.aim_at_world_unit(kind, identity)
        self.page.evaluate("p => window.__valeDebug.flyToPose([p.x+3,p.y+4,p.z+5],[p.x,p.y,p.z])", point)
        self.frames(3)

    def capture_action_frame(self):
        """Read-only actual last-rendered nodes; never inject projections, poses or outcomes."""
        return self.page.evaluate("""() => {
          const d = window.__valeDebug;
          const p = d.worldSource().latestProjection();
          return {
            tick: Number(p.tick), health: p.viewer.health, dead: p.viewer.dead,
            viewer: window.__acceptanceNodes(`unit-player-${p.viewerId}`),
            target: p.viewer.target ? window.__acceptanceNodes(`unit-${p.viewer.target.kind}-${p.viewer.target.id}`) : [],
            effects: d.presentationFrame().effectNodes,
            events: p.events.map(e => e.kind),
          };
        }""")

    def test_procedural_actions_and_confirmed_impacts_through_real_wasm(self):
        """Normal Greyhaven commands must animate actual model nodes, then land richer target feedback."""
        self.open("?debug")
        self.page.emulate_media(reduced_motion="no-preference")
        self.create_and_select("ActionMage", "Arcanist")
        self.enter_beside_wolf()
        self.aim_action_unit("creature", 108)
        rest = self.capture_action_frame()
        self.assertTrue(rest["viewer"] and rest["target"])
        self.page.keyboard.press("Digit1")
        self.step(3)
        cast = self.capture_action_frame()
        self.assertNotEqual(cast["viewer"], rest["viewer"], "The real Arcanist model raises into its cast")
        expect(self.page.locator("[data-part=cast]")).to_be_visible()
        # Preserve each actual received outcome, advancing the authoritative WASM one tick at a time.
        self.assertTrue(self.advance_action_until("damage-dealt", 65, 1), "Firebolt resolves with authoritative damage")
        landed = self.capture_action_frame()
        self.assertGreater(len(landed["effects"]), 2, "Confirmed target impact adds richer feedback to the baseline bolt")
        self.page.screenshot(path=str(ARTIFACTS / "procedural-actions-impact.png"))
        self.page.wait_for_function("window.__valeDebug.presentationFrame().effectNodes.length === 0", timeout=5_000)
        settled = self.capture_action_frame()
        self.assertEqual(settled["tick"], landed["tick"], "Expiry uses presentation time, without advancing gameplay")
        self.assertEqual(settled["effects"], [], "Transient effects expire without another simulation tick")
        (ARTIFACTS / "procedural-actions-local.json").write_text(json.dumps({"rest": rest, "cast": cast, "landed": landed, "settled": settled}, indent=2))

    def test_ordinary_actions_creature_death_and_player_death_through_real_wasm(self):
        self.open("?debug")
        self.page.emulate_media(reduced_motion="no-preference")
        self.enter_beside_wolf()
        self.page.evaluate("""() => {
          window.__lifeFrames = [];
          const record = () => {
            const d = window.__valeDebug, p = d.worldSource().latestProjection();
            if (p) {
              const target = p.entities.find(e => e.kind === 'creature' && e.entityId === 108);
              window.__lifeFrames.push({
                tick: Number(p.tick), playerDead: p.viewer.dead, targetDead: target?.flags.dead ?? false,
                player: window.__acceptanceNodes(`unit-player-${p.viewerId}`), target: window.__acceptanceNodes('unit-creature-108'),
              });
            }
            if (window.__lifeFrames.length > 64) window.__lifeFrames.shift();
            requestAnimationFrame(record);
          };
          requestAnimationFrame(record);
          window.__actualCombatEvents = [];
          const advance = window.__classAdvance;
          window.__classAdvance = function(...args) {
            const received = advance.apply(this, args);
            for (const projection of received) window.__actualCombatEvents.push(...projection.events.map(e => ({...e})));
            return received;
          };
        }""")
        self.page.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'start-attack'})")
        # Pause only presentation between outcomes. All commands and simulation ticks are real.
        # Sample the first accepted ordinary actions before progressing to the deaths.
        self.advance_action_until("ordinary-attempt", 100)
        self.frames(8)
        ordinary_frames = self.page.evaluate("window.__lifeFrames")
        self.assertGreater(len({json.dumps(f["player"], sort_keys=True) for f in ordinary_frames}), 1, "Real ordinary player action plays through submitted transforms")
        self.page.evaluate("window.__lifeFrames = []")
        dead = self.advance_action_until("target-dead", 100)
        self.assertTrue(dead, "Ordinary combat kills the actual selected wolf")
        events = self.page.evaluate("window.__actualCombatEvents")
        self.assertTrue(any(e["kind"] == "attack-performed" and e["source"]["kind"] == "player" for e in events))
        self.assertTrue(any(e["kind"] == "attack-performed" and e["source"]["kind"] == "creature" for e in events))
        self.wait_for_settled_corpse("target", "targetDead")
        frames = self.page.evaluate("window.__lifeFrames")
        corpses = [frame["target"] for frame in frames if frame["targetDead"]]
        self.assertGreater(len({json.dumps(nodes, sort_keys=True) for nodes in corpses}), 1, "Real corpse transitions before settling")
        wolf_frames = frames
        self.aim_action_unit("creature", 108)
        self.page.screenshot(path=str(ARTIFACTS / "procedural-wolf-corpse.png"))
        self.page.evaluate("window.__valeDebug.follow(); window.__lifeFrames = []")
        # Stop attacking and approach the neighbouring living wolves using actual movement.
        self.page.evaluate("""() => {
          const s = window.__valeDebug.worldSource();
          s.sendCommand({kind:'stop-attack'});
          s.sendCommand({kind:'move',forward:1,strafe:0,facing:32768});
        }""")
        self.page.evaluate("window.__classStep(35)")
        self.page.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'move',forward:0,strafe:0,facing:32768})")
        self.advance_action_until("player-dead", 600)
        self.assertTrue(self.page.evaluate("window.__valeDebug.worldSource().latestProjection().viewer.dead"), "Real creature attacks kill the player without injected health")
        self.wait_for_settled_corpse("player", "playerDead")
        frames = self.page.evaluate("window.__lifeFrames")
        fallen = [frame["player"] for frame in frames if frame["playerDead"]]
        self.assertGreater(len({json.dumps(nodes, sort_keys=True) for nodes in fallen}), 1, "Player death plays through before a stable corpse")
        self.page.screenshot(path=str(ARTIFACTS / "procedural-player-corpse.png"))
        (ARTIFACTS / "procedural-actions-life.json").write_text(json.dumps({"events": events, "ordinaryFrames":ordinary_frames, "wolfFrames":wolf_frames, "playerFrames":frames}, indent=2))

    def wait_for_settled_corpse(self, who, dead, max_frames=240):
        """The death clip advances by at most 0.05 s per rendered frame, so bound the wait in frames, not wall time."""
        for _ in range(max_frames // 4):
            if self.page.evaluate("""([who, dead]) => {
              const frames = window.__lifeFrames.filter(f => f[dead] && f[who].length);
              return frames.length >= 8 && frames.slice(-5).every(f => JSON.stringify(f[who]) === JSON.stringify(frames.at(-1)[who]));
            }""", [who, dead]):
                return
            self.frames(4)
        self.fail(f"The {who} corpse did not settle within {max_frames} frames")

    def advance_action_until(self, outcome, limit, group_ticks=10):
        """Progress actual zone ticks in the original ten-tick groups; render only required phases."""
        reached = self.page.evaluate("""({outcome,limit,groupTicks}) => {
          const s = window.__valeDebug.worldSource();
          for (let group = 0; group < limit; group++) {
            window.__classStep(groupTicks);
            const p = s.latestProjection();
            let done = false;
            if (outcome === 'target-dead') done = p.entities.some(e => e.kind === 'creature' && e.entityId === 108 && e.flags.dead);
            else if (outcome === 'player-dead') done = p.viewer.dead;
            else if (outcome === 'target-below-half') done = p.entities.some(e => e.kind === 'creature' && e.entityId === 401 && e.healthPercent < 50);
            else if (outcome === 'ordinary-attempt') done = window.__actualCombatEvents.some(e => e.kind === 'attack-performed' && e.source.kind === 'player');
            else if (outcome === 'damage-dealt') done = p.events.some(e => e.kind === 'damage-dealt');
            else {
              // "ability" or "ability@creature": the ability's use, optionally by one creature.
              const [ability, caster] = outcome.split('@').map(Number);
              done = p.events.some(e => e.kind === 'ability-used' && e.ability === ability
                && (!caster || (e.source?.kind === 'creature' && e.source.id === caster)));
            }
            if (done) return true;
          }
          return false;
        }""", {"outcome":outcome,"limit":limit,"groupTicks":group_ticks})
        self.frames(3)
        return reached

    def prepare_paused_action_world(self):
        self.open("?debug")
        self.page.emulate_media(reduced_motion="no-preference")
        self.create_and_select("CreatureWitness", "Arcanist")
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          window.__classAdvance = source.advance;
          source.advance = () => [];
        }""")
        self.enter_world(ticking=False)
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          window.__classQueue = [];
          source.advance = () => window.__classQueue.splice(0);
          window.__classStep = ticks => {
            for (let tick = 0; tick < ticks; tick++) window.__classQueue.push(...window.__classAdvance.call(source, 1 / 30));
          };
        }""")

    def action_route(self, points):
        """Steer actual movement toward clear authored waypoints, never change coordinates directly."""
        result = self.page.evaluate("""points => {
          const source = window.__valeDebug.worldSource(), route = [];
          const advance = () => window.__classQueue.push(...window.__classAdvance.call(source, 1 / 30));
          for (const [x,z] of points) {
            let arrived = false, stalled = 0;
            for (let count = 0; count < 1500; count++) {
              const p = source.latestProjection(), at = p.entities[0].position;
              if (p.viewer.dead) throw Error('Died on action route');
              const dx = x-at[0], dz = z-at[2];
              if (Math.hypot(dx,dz) < 25) {
                source.sendCommand({kind:'move',forward:0,strafe:0,facing:0}); advance();
                route.push({goal:[x,z],at:[...source.latestProjection().entities[0].position],tick:Number(source.latestProjection().tick)});
                arrived = true; break;
              }
              const facing = (Math.round(Math.atan2(dx,dz)*65536/(2*Math.PI))+65536)%65536;
              source.sendCommand({kind:'move',forward:1,strafe:0,facing}); advance();
              const next = source.latestProjection().entities[0].position;
              stalled = next[0] === at[0] && next[2] === at[2] ? stalled+1 : 0;
              if (stalled > 60) throw Error('Stalled on action route');
            }
            if (!arrived) throw Error('Action route exceeded tick bound');
          }
          return route;
        }""", points)
        self.frames(3)
        return result

    def creature_action_frame(self, identity):
        return self.page.evaluate("""id => {
          const d = window.__valeDebug, p = d.worldSource().latestProjection();
          return {tick:Number(p.tick), health:p.viewer.health, nodes:window.__acceptanceNodes(`unit-creature-${id}`),
            target:p.entities.find(e => e.kind === 'creature' && e.entityId === id)?.healthPercent,
            events:p.events.filter(e => (e.source?.kind === 'creature' && e.source.id === id) || e.kind === 'error'),
            effects:d.presentationFrame().effectNodes};
        }""", identity)

    def test_real_greyhaven_muck_bolt_model_playback(self):
        self.prepare_paused_action_world()
        route = self.action_route([[0,1250],[0,-4000],[2500,-4800]])
        self.aim_action_unit("creature", 300)
        before = self.creature_action_frame(300)
        self.assertTrue(before["nodes"])
        self.page.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'select-target',target:{kind:'creature',id:300}})")
        route += self.action_route([[3200,-4900],[3500,-5000]])
        casting = self.creature_action_frame(300)
        self.assertNotEqual(casting["nodes"], before["nodes"], "The actual Mirefin caster model changes during Muck Bolt")
        self.page.screenshot(path=str(ARTIFACTS / "procedural-muck-bolt-cast.png"))
        frames = [casting]
        self.frames(4)
        frames.append(self.creature_action_frame(300))
        self.assertTrue(self.advance_action_until("13@300", 70, 1))
        frames.append(self.creature_action_frame(300))
        self.page.screenshot(path=str(ARTIFACTS / "procedural-muck-bolt-release.png"))
        self.frames(8)
        frames.append(self.creature_action_frame(300))
        self.assertTrue(any(any(e["kind"] == "ability-used" and e.get("ability") == 13 for e in f["events"]) for f in frames))
        self.assertGreater(frames[-1]["health"], 0)
        self.assertGreater(len({json.dumps(f["nodes"], sort_keys=True) for f in frames}), 1, "Cast releases through changing actual nodes")
        self.page.screenshot(path=str(ARTIFACTS / "procedural-muck-bolt.png"))
        (ARTIFACTS / "procedural-muck-bolt.json").write_text(json.dumps({"route":route,"before":before,"frames":frames}, indent=2))

    def test_real_greyhaven_crude_bandage_model_playback(self):
        self.prepare_paused_action_world()
        route = self.action_route([[0,1250],[0,-4000],[0,-5400],[1500,-5400]])
        self.page.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'select-target',target:{kind:'creature',id:401}})")
        self.step(1)
        for _ in range(4):
            self.page.keyboard.press("Digit1")
            self.step(61)
        self.page.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'start-attack'})")
        self.assertTrue(self.advance_action_until("target-below-half", 240, 1))
        frames = [self.creature_action_frame(401)]
        self.assertLess(frames[-1]["target"], 50, "Real damage reaches the existing bandage threshold")
        self.page.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'stop-attack'})")
        self.aim_action_unit("creature", 401)
        self.step(1)
        frames.append(self.creature_action_frame(401))
        self.frames(4)
        frames.append(self.creature_action_frame(401))
        self.page.screenshot(path=str(ARTIFACTS / "procedural-bandage-cast.png"))
        self.assertTrue(self.advance_action_until("14", 120, 1))
        frames.append(self.creature_action_frame(401))
        starts = [f for f in frames if any(e["kind"] == "cast-started" and e.get("ability") == 14 for e in f["events"])]
        healed = [f for f in frames if any(e["kind"] == "healed" and e.get("ability") == 14 for e in f["events"])]
        self.assertTrue(starts and healed, "Actual bandit begins and resolves its own bandage")
        self.assertGreater(healed[-1]["target"], starts[0]["target"])
        self.assertGreater(len({json.dumps(f["nodes"], sort_keys=True) for f in frames if f["tick"] >= starts[0]["tick"]}), 1)
        self.assertGreater(healed[-1]["health"], 0)
        self.page.screenshot(path=str(ARTIFACTS / "procedural-bandage.png"))
        (ARTIFACTS / "procedural-bandage.json").write_text(json.dumps({"route":route,"frames":frames}, indent=2))

    def test_complete_world_environment_instancing_and_night_readability(self):
        """Observe actual submitted renderer frames; cosmetic time never changes the real WASM zone."""
        self.open("?debug")
        self.enter_world()
        self.page.evaluate("window.__valeDebug.flyTo('hub')")
        before = self.page.evaluate("window.__valeDebug.projection().entities[0]")
        observations = []
        views = []
        for seconds in [0, 300, 600, 900]:
            self.page.evaluate("seconds => window.__valeDebug.setPresentationTime(seconds)", seconds)
            self.frames(4)
            frame = self.page.evaluate("window.__valeDebug.presentationFrame()")
            self.assertGreater(frame["work"]["instanceBatchCount"], 0)
            self.assertGreater(frame["work"]["instanceCount"], frame["work"]["instanceBatchCount"])
            self.assertGreater(frame["vertexColoredNodes"], 0)
            self.assertGreater(frame["emissiveNodes"], 0)
            self.assertGreater(frame["environment"]["fog"]["far"], frame["environment"]["fog"]["near"])
            self.assertGreater(self.canvas_colours()["distinct"], 100, "Night must remain a readable rendered landscape")
            expect(self.page.get_by_role("complementary", name="Minimap")).to_be_visible()
            views.append(self.world_view())
            observations.append(frame)
            self.page.screenshot(path=str(ARTIFACTS / f"world-day-{seconds}.png"))
        self.assertLess(observations[2]["environment"]["sun"]["intensity"], observations[0]["environment"]["sun"]["intensity"])
        self.assertLess(observations[2]["environment"]["sky"]["intensity"], observations[0]["environment"]["sky"]["intensity"])
        self.assertNotEqual(views[0], views[2])
        self.assertEqual(self.page.evaluate("window.__valeDebug.projection().entities[0]"), before)
        (ARTIFACTS / "world-presentation-frames.json").write_text(json.dumps(observations, indent=2))

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

    def practice_fight(self, entered, walk, select, swing, reset, change_build, touch=False):
        """#550/#551: Battle Royale practice from the Pages URL, without a server: enter, walk into the
        training targets, hit one through real zone combat with floating damage, reset and change build."""
        self.page.goto(self.url + "?activity=battle-royale")
        start = self.page.get_by_role("button", name=re.compile("^Start practice"))
        expect(start).to_be_enabled(timeout=30_000)
        # A phone taps Start, which keeps the page in touch input mode.
        start.tap() if touch else start.click()
        expect(self.page.locator("#character-select")).to_be_hidden()
        expect(self.page.locator("#unit-status")).to_contain_text("Level 10")
        # Floating combat text is short-lived; record every entry the class HUD adds.
        self.page.evaluate("""() => { window.__combatTexts = [];
          new MutationObserver(records => { for (const record of records) for (const node of record.addedNodes)
            if (node.classList?.contains('combat-text-entry')) window.__combatTexts.push(node.className + ' ' + node.textContent);
          }).observe(document.body, {subtree: true, childList: true}); }""")
        entered()
        # Walk straight into the target ahead (its body stops the character in reach) and select it.
        walk()
        select()
        expect(self.page.locator("#unit-status")).to_contain_text("Target: Training Target")
        for _ in range(30):
            swing()
            # Let the swing land before the next one.
            self.page.wait_for_timeout(500)
            if "100%)" not in (self.page.locator("#unit-status").text_content() or ""):
                break
        expect(self.page.locator("#unit-status")).to_contain_text(re.compile(r"Training Target \(level 10, \d{1,2}%\)"))
        self.assertTrue(any("ct-damage" in text or "ct-critical" in text for text in self.page.evaluate("window.__combatTexts")),
                        "A landed hit floats its authoritative damage")
        self.page.screenshot(path=str(ARTIFACTS / f"{self._testMethodName}-fight.png"))
        reset()
        expect(self.page.locator("#unit-status")).not_to_contain_text("In combat")
        expect(self.page.locator("#unit-status")).not_to_contain_text("Target:")
        change_build()
        expect(start).to_be_visible()

    def test_battle_royale_practice_desktop(self):
        def entered():
            expect(self.page.get_by_role("button", name="Change build")).to_be_visible()
            self.page.locator("#world").focus()

        def walk():
            self.page.keyboard.down("KeyW")
            self.page.wait_for_timeout(5_000)
            self.page.keyboard.up("KeyW")

        self.practice_fight(
            entered, walk, lambda: self.page.keyboard.press("Tab"),
            # A left click on open ground swings where the character faces (a click on a unit selects it).
            lambda: self.page.mouse.click(720, 200),
            lambda: self.page.keyboard.press("F9"),
            lambda: self.page.get_by_role("button", name="Change build").click())

    def test_battle_royale_practice_touch(self):
        self.context.close()
        self.context = self.browser.new_context(viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True, device_scale_factor=1)
        self.page = self.context.new_page()
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        session = self.context.new_cdp_session(self.page)

        def drag(start, end, hold_ms):
            touch = lambda x, y: [{"x": x, "y": y, "id": 1}]
            session.send("Input.dispatchTouchEvent", {"type": "touchStart", "touchPoints": touch(*start)})
            for step in range(1, 6):
                session.send("Input.dispatchTouchEvent", {"type": "touchMove", "touchPoints": touch(
                    start[0] + (end[0] - start[0]) * step / 5, start[1] + (end[1] - start[1]) * step / 5)})
            self.page.wait_for_timeout(hold_ms)
            session.send("Input.dispatchTouchEvent", {"type": "touchEnd", "touchPoints": []})

        def entered():
            # #551: the arena offers target and attack from the first moment, and no essential
            # control, frame, status line or the hotbar overlaps another at phone size.
            expect(self.page.get_by_role("button", name="Attack", exact=True)).to_be_visible()
            expect(self.page.get_by_role("button", name="Target", exact=True)).to_be_visible()
            overlaps = self.page.evaluate("""() => {
              const selectors = ['.unit-frames', '.hud', '.minimap', '.touch-top', '.action-dock', '.touch-context',
                '.touch-jump', '.touch-faces'];
              const boxes = selectors.map(selector => [selector, document.querySelector(selector).getBoundingClientRect()]);
              const unitFrames = [...document.querySelectorAll('.unit-frame')].filter(frame => frame.offsetParent)
                .map(frame => frame.getBoundingClientRect());
              const frames = ['.unit-frames', {left: Math.min(...unitFrames.map(b => b.left)), right: Math.max(...unitFrames.map(b => b.right)),
                top: Math.min(...unitFrames.map(b => b.top)), bottom: Math.max(...unitFrames.map(b => b.bottom))}];
              const shown = [frames, ...boxes.slice(1)].filter(([, box]) => box.right > box.left && box.bottom > box.top);
              const found = [];
              for (const [i, [a, first]] of shown.entries()) for (const [b, second] of shown.slice(i + 1))
                if (first.left < second.right && second.left < first.right && first.top < second.bottom && second.top < first.bottom) found.push(a + ' / ' + b);
              const wide = document.documentElement.scrollWidth > window.innerWidth;
              return wide ? [...found, 'horizontal scroll'] : found; }""")
            self.assertEqual(overlaps, [], "Phone-size practice controls must not overlap")
            self.page.screenshot(path=str(ARTIFACTS / f"{self._testMethodName}-entered.png"))

        def change_build():
            self.page.get_by_role("button", name="Menu", exact=True).tap()
            self.page.get_by_role("menuitem", name="Change build").tap()

        def reset():
            # The thumb on open ground (right of the stick) turns the camera, then the menu resets.
            drag((300, 380), (220, 380), 200)
            self.page.get_by_role("button", name="Menu", exact=True).tap()
            self.page.get_by_role("menuitem", name="Reset practice").tap()

        self.practice_fight(
            entered,
            # The thumbstick pushed up walks forward.
            lambda: drag((60, 720), (60, 650), 5_000),
            lambda: self.page.get_by_role("button", name="Target", exact=True).tap(),
            lambda: self.page.get_by_role("button", name="Attack", exact=True).tap(),
            reset, change_build, touch=True)
        session.detach()

    def fortress_walkthrough(self, touch=False):
        """#552: the shared Battle Royale fortress in practice, walked through the real WASM zone
        (mmorpg-core and Rapier): courtyard -> keep door -> corridor -> interior ramp -> keep roof
        -> central tower lift -> tower top. Returns each stop's feet position in metres."""
        self.page.goto(self.url + "?activity=battle-royale&debug")
        start = self.page.get_by_role("button", name=re.compile("^Start practice"))
        expect(start).to_be_enabled(timeout=30_000)
        start.tap() if touch else start.click()
        expect(self.page.locator("#character-select")).to_be_hidden()
        self.frames(6)
        # Drive the zone tick by tick from here, as the click smokes do; the page still renders.
        self.page.evaluate("""() => {
          const source = window.__valeDebug.worldSource();
          const advance = source.advance;
          window.__fortressQueue = [];
          source.advance = () => window.__fortressQueue.splice(0);
          const feet = () => {
            const projection = source.latestProjection();
            const self = projection.entities.find(e => e.kind === 'player' && e.entityId === projection.viewerId);
            return [self.position[0] / 100, (self.position[1] - 90) / 100, self.position[2] / 100];
          };
          const tick = command => {
            if (command) source.sendCommand(command);
            window.__fortressQueue.push(...advance.call(source, 1 / 30));
          };
          // Walks toward (x, z) metres, or until `until(feet)`; throws when it gets stuck.
          window.__fortressWalk = (x, z, ticks = 900, until = null) => {
            for (let i = 0; i < ticks; i++) {
              const [fx, fy, fz] = feet();
              if (until ? until([fx, fy, fz]) : Math.hypot(x - fx, z - fz) < 0.25) {
                for (let j = 0; j < 15; j++) tick(j === 0 ? {kind: 'move', forward: 0, strafe: 0, facing: 0} : null);
                return feet();
              }
              const facing = Math.round(Math.atan2(x - fx, z - fz) / (2 * Math.PI) * 65536);
              tick({kind: 'move', forward: 1, strafe: 0, facing: ((facing % 65536) + 65536) % 65536});
            }
            throw Error(`stuck at ${feet()} on the way to ${x}, ${z}`);
          };
        }""")
        stops = {}

        def walk(name, x, z, until=None, ticks=900):
            stops[name] = self.page.evaluate(f"([x, z, t]) => window.__fortressWalk(x, z, t{', ' + until if until else ''})", [x, z, ticks])
            self.frames(6)

        def shot(name):
            self.page.screenshot(path=str(ARTIFACTS / f"{self._testMethodName}-{name}.png"))

        shot("courtyard")
        walk("door", 4.5, -14)
        walk("corridor", 4.5, -9.9)
        shot("interior")
        walk("ramp-foot", 2, -9.9)
        walk("roof", -12, -9.9)
        shot("roof")
        # The central tower lift: push onto its pillar (a wall while it is up) until it carries
        # the character up to the tower top.
        walk("lift", -4, -7.05, until="([x, y, z]) => y > 15.5", ticks=1_500)
        walk("tower", -4, -4)
        shot("tower")
        return stops

    def assert_fortress_route(self, stops):
        for name, height in [("door", 0), ("corridor", 0), ("roof", 8), ("lift", 16), ("tower", 16)]:
            self.assertAlmostEqual(stops[name][1], height, delta=0.15, msg=f"{name} stands at {height} m: {stops[name]}")

    def test_battle_royale_fortress_walkthrough_desktop(self):
        stops = self.fortress_walkthrough()
        self.assert_fortress_route(stops)
        self.evidence.append({"test": self._testMethodName, "stops": stops})

    def test_battle_royale_fortress_walkthrough_phone(self):
        self.context.close()
        self.context = self.browser.new_context(viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True, device_scale_factor=1)
        self.page = self.context.new_page()
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        stops = self.fortress_walkthrough(touch=True)
        self.assert_fortress_route(stops)
        # The touch controls stay usable over the fortress.
        expect(self.page.get_by_role("button", name="Attack", exact=True)).to_be_visible()


if __name__ == "__main__":
    unittest.main(verbosity=2)
