#!/usr/bin/env python3
"""Real two-page online acceptance: Chromium pages join one real zone host over WebTransport.

Builds and starts `mmorpg-zone-host` with a disposable ECDSA P-256 development
certificate (valid 10 days, as WebTransport certificate hashes require at most
14) on OS-selected ports, serves the production bundle, and opens two pages
with `?server=<admission route>&certHash=<SHA-256>`. One character runs; the
other page must see it move through its own decoded projections. Closing the
running page must remove its unit from the other page's projections once the
host's reconnect grace (one second here) has passed. Nothing is mocked: the
pages use the production bundle, browser WebTransport and the real host.

Prerequisites: those of scripts/smoke-browser.py (a built web/dist, Playwright
Chromium) plus cargo and OpenSSL. Run from the repository root:

    python3 scripts/smoke-browser-online.py

Evidence (screenshots, positions, host log) goes to artifacts/browser-online/.
"""
from __future__ import annotations

import base64
import functools
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.parse
import urllib.request

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "artifacts" / "browser-online"
ZONE_ID = 1
GRACE_TICKS = 30
TICK_HZ = 30
READY_TIMEOUT_S = 60
# Units per metre; the run must carry the character well past its neighbours on the spawn grid.
MIN_RUN_UNITS = 300


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/mmorpg/"):
            self.path = self.path[len("/mmorpg"):]
        super().do_GET()

    def log_message(self, *_args):
        pass


def free_port(kind: int) -> int:
    """An OS-selected port for a disposable process (TEST-016)."""
    with socket.socket(socket.AF_INET, kind) as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def build_zone_host() -> Path:
    result = subprocess.run(
        ["cargo", "build", "--locked", "-p", "mmorpg-game-server", "--bin", "mmorpg-zone-host", "--message-format=json-render-diagnostics"],
        cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True, timeout=1800,
    )
    for line in result.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == "mmorpg-zone-host" and message.get("executable"):
            return Path(message["executable"])
    raise RuntimeError("cargo did not report the mmorpg-zone-host executable")


def development_certificate(directory: Path) -> tuple[Path, Path, str]:
    certificate, key = directory / "cert.pem", directory / "key.pem"
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
         "-keyout", str(key), "-out", str(certificate), "-sha256", "-days", "10", "-nodes",
         "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1"],
        check=True, capture_output=True, timeout=60,
    )
    der = ssl.PEM_cert_to_DER_cert(certificate.read_text())
    return certificate, key, base64.b64encode(hashlib.sha256(der).digest()).decode()


class OnlineAcceptance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        dist = ROOT / "web" / "dist"
        if not (dist / "index.html").is_file():
            raise RuntimeError("Build the production web bundle (cd web && bun run build) before online acceptance.")
        ARTIFACTS.mkdir(parents=True, exist_ok=True)
        cls.cleanups = []
        try:
            cls.start(dist)
        except BaseException:
            cls.tearDownClass()
            raise

    @classmethod
    def start(cls, dist: Path):
        executable = build_zone_host()
        temporary = tempfile.TemporaryDirectory(prefix="mmorpg-online-smoke-")
        cls.cleanups.append(temporary.cleanup)
        certificate, key, cls.certificate_hash = development_certificate(Path(temporary.name))
        transport_port, status_port = free_port(socket.SOCK_DGRAM), free_port(socket.SOCK_STREAM)
        cls.route = f"https://127.0.0.1:{transport_port}/game/matches/zone-{ZONE_ID}"
        environment = {key_: value for key_, value in os.environ.items() if key_ != "MMORPG_RECOVERY_DIR"}
        environment.update({
            "MMORPG_ZONE_IDS": str(ZONE_ID),
            "MMORPG_PORT": str(transport_port),
            "MMORPG_STATUS_PORT": str(status_port),
            "MMORPG_CERT_PEM": str(certificate),
            "MMORPG_KEY_PEM": str(key),
            "MMORPG_ROUTE_PREFIX": "/game",
            "MMORPG_RECONNECT_GRACE_TICKS": str(GRACE_TICKS),
            "MMORPG_DRAIN_GRACE_MS": "200",
        })
        cls.host_log = ARTIFACTS / "zone-host.log"
        log = cls.host_log.open("w")
        cls.cleanups.append(log.close)
        cls.host = subprocess.Popen([str(executable)], cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT)
        cls.cleanups.append(cls.stop_host)
        deadline = time.monotonic() + READY_TIMEOUT_S
        while True:
            if cls.host.poll() is not None:
                raise RuntimeError(f"The zone host exited with {cls.host.returncode}:\n{cls.host_log.read_text()}")
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{status_port}/readyz", timeout=1):
                    break
            except (urllib.error.URLError, OSError):
                if time.monotonic() > deadline:
                    raise RuntimeError("The zone host did not become ready.") from None
                time.sleep(0.1)

        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(dist)))
        thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        thread.start()
        cls.cleanups.append(lambda: (cls.server.shutdown(), cls.server.server_close(), thread.join()))
        cls.url = f"http://127.0.0.1:{cls.server.server_port}/mmorpg/?" + urllib.parse.urlencode(
            {"debug": "", "server": cls.route, "certHash": cls.certificate_hash})

        cls.playwright = sync_playwright().start()
        cls.cleanups.append(cls.playwright.stop)
        cls.browser = cls.playwright.chromium.launch(args=["--use-angle=swiftshader", "--enable-unsafe-swiftshader"])
        cls.cleanups.append(cls.browser.close)

    @classmethod
    def stop_host(cls):
        if cls.host.poll() is None:
            cls.host.terminate()
            try:
                cls.host.wait(timeout=10)
            except subprocess.TimeoutExpired:
                cls.host.kill()
                cls.host.wait()

    @classmethod
    def tearDownClass(cls):
        for cleanup in reversed(cls.cleanups):
            try:
                cleanup()
            except Exception as error:  # Keep cleaning up; the first failure is already reported.
                print(f"cleanup failed: {error}")

    def open_player(self, name: str, *, animated=False, class_label=None):
        context = self.browser.new_context(viewport={"width": 1280, "height": 800}, reduced_motion="no-preference" if animated else "reduce")
        context.add_init_script("""(() => {
          const Original = window.WebTransport;
          window.__acceptanceNodes = identity => window.__valeDebug.unitNodes(identity).map(({id,transform}) => ({id,transform}));
          window.__actualTransports = [];
          window.__transportKinds = [];
          window.WebTransport = new Proxy(Original, {construct(Target, args) {
            const transport = Reflect.construct(Target, args, Target);
            window.__actualTransports.push(transport);
            window.__transportKinds.push(new URL(args[0]).pathname.includes('/reconnect/'));
            return transport;
          }});
        })();""")
        page = context.new_page()
        errors: list[str] = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto(self.url)
        enter = page.get_by_role("button", name=re.compile("^Enter World"))
        expect(enter).to_be_enabled(timeout=30_000)
        expect(page.get_by_role("checkbox", name=re.compile("Play online"))).to_be_checked()
        expect(page.locator("#enter-world-note")).to_contain_text("Join 127.0.0.1:")
        page.screenshot(path=str(ARTIFACTS / f"{name}-selection.png"))
        if class_label:
            page.get_by_role("button", name="Create character", exact=True).click()
            page.get_by_label("Name").fill(name)
            page.get_by_role("radio", name=re.compile(f"^{class_label}")).check()
            page.get_by_role("button", name="Create character", exact=True).filter(visible=True).click()
        enter.click()
        expect(page.locator("#character-select")).to_be_hidden(timeout=15_000)
        expect(page.locator("#world-mode")).to_contain_text("online")
        page.locator("#world").focus()
        self.wait_for(page, "window.__valeDebug.projection() !== null")
        self.players.append({"name": name, "context": context, "page": page, "errors": errors})
        return page

    def frames(self, page, count=3):
        page.evaluate("""count => new Promise(resolve => {
          function next() { if (--count <= 0) resolve(); else requestAnimationFrame(next); }
          requestAnimationFrame(next);
        })""", count)

    def wait_for(self, page, expression: str, argument=None, timeout_ms=15_000):
        page.wait_for_function(expression, arg=argument, timeout=timeout_ms)

    @staticmethod
    def projection(page):
        return page.evaluate("window.__valeDebug.projection()")

    # Projections also carry creatures and NPCs, whose IDs overlap player IDs: match players by kind too.

    def seen(self, page, player_id: int):
        entities = self.projection(page)["entities"]
        return next((entity for entity in entities if entity["kind"] == "player" and entity["id"] == player_id), None)

    def setUp(self):
        self.players = []
        self.evidence = {}

    def tearDown(self):
        evidence = []
        for player in self.players:
            if not player["page"].is_closed():
                player["page"].screenshot(path=str(ARTIFACTS / f"{player['name']}.png"))
            evidence.append({"page": player["name"], "pageErrors": player["errors"], "projection":self.projection(player["page"]) if not player["page"].is_closed() else None, "actionSamples":player["page"].evaluate("window.__actionSamples ?? []") if not player["page"].is_closed() else []})
            player["context"].close()
        self.evidence["pages"] = evidence
        self.evidence["route"] = self.route
        (ARTIFACTS / "online-evidence.json").write_text(json.dumps(self.evidence, indent=2))
        for player in self.players:
            self.assertEqual(player["errors"], [], f"{player['name']} raised an uncaught error")

    def face_online_waypoint(self, page, x, z):
        """Right-drag turns the real controller; held input remains owned by production."""
        box = page.locator("#world").bounding_box()
        cx, cy = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
        # Align once: subsequent gestures track the camera yaw changed by our pointer.
        if not hasattr(self, "route_camera_yaw"):
            self.route_camera_yaw = {}
        if page not in self.route_camera_yaw:
            tick = self.projection(page)["tick"]
            page.keyboard.down("KeyW")
            self.wait_for(page, "tick => window.__valeDebug.projection().tick >= tick", tick + 3)
            page.keyboard.up("KeyW")
            page.mouse.move(cx, cy)
            page.mouse.down(button="right")
            page.mouse.move(cx, cy + 8)
            tick = self.projection(page)["tick"]
            self.wait_for(page, "tick => window.__valeDebug.projection().tick >= tick", tick + 12)
            self.route_camera_yaw[page] = self.projection(page)["entities"][0]["facing"]
            page.mouse.up(button="right")
        unit = self.projection(page)["entities"][0]
        page.mouse.move(cx, cy)
        page.mouse.down(button="right")
        page.mouse.move(cx, cy + 8)
        import math
        desired = round(math.atan2(x-unit["position"][0], z-unit["position"][2])*65536/(2*math.pi)) % 65536
        delta = (desired-self.route_camera_yaw[page]+32768) % 65536-32768
        page.mouse.move(cx-delta*(2*math.pi/65536)/0.006, cy+8, steps=4)
        self.route_camera_yaw[page] = desired
        self.frames(page, 2)
        page.mouse.up(button="right")

    def walk_online_waypoints(self, pages, points):
        route = []
        self.evidence["routeDiagnostics"] = diagnostics = []
        for x, z in points:
            pending = list(pages)
            deadline = time.monotonic() + 25
            try:
                while pending:
                    legs = []
                    for page in list(pending):
                        projection = self.projection(page)
                        unit = projection["entities"][0]
                        at = unit["position"]
                        distance = ((at[0]-x)**2 + (at[2]-z)**2)**0.5
                        diagnostics.append({"goal":[x,z], "player":projection["viewerId"],
                                            "tick":projection["tick"], "at":at,
                                            "facing":unit["facing"], "distance":distance})
                        if distance < 100:
                            pending.remove(page)
                            route.append({"goal":[x,z],"player":projection["viewerId"],"at":at})
                            continue
                        self.face_online_waypoint(page, x, z)
                        # Stop short, then re-aim from the actual authoritative position.
                        ticks = max(1, min(60, int((distance-70)/21)))
                        legs.append((page, ticks))
                    moving = []
                    starts = {}
                    for page, ticks in legs:
                        start = self.projection(page)
                        starts[page] = start
                        page.keyboard.down("KeyW")
                        moving.append((page, start["tick"] + ticks))
                    while moving:
                        for page, until in list(moving):
                            projection = self.projection(page)
                            at = projection["entities"][0]["position"]
                            if projection["tick"] >= until or (at[0]-x)**2+(at[2]-z)**2 < 100**2:
                                page.keyboard.up("KeyW")
                                start = starts[page]
                                diagnostics.append({"goal":[x,z], "player":projection["viewerId"],
                                                    "startTick":start["tick"], "endTick":projection["tick"],
                                                    "startAt":start["entities"][0]["position"], "endAt":at,
                                                    "facing":projection["entities"][0]["facing"],
                                                    "requestedTicks":until-start["tick"]})
                                moving.remove((page, until))
                        if time.monotonic() > deadline:
                            self.fail(f"Held-input route did not reach {(x,z)}: {diagnostics[-4:]}")
                        time.sleep(0.02)
                    if time.monotonic() > deadline:
                        self.fail(f"Held-input route did not reach {(x,z)}: {diagnostics[-4:]}")
            finally:
                for page in pages:
                    page.keyboard.up("KeyW")
        return route

    def test_remote_procedural_attack_cast_impact_and_departure(self):
        """Two real sessions observe ordinary attacks, accepted casts and confirmed impacts.

        Both travel the existing Greyhaven route via ordinary movement commands.
        The watcher never attacks: no fake projection, teleport or client resolver.
        """
        actor = self.open_player("ActorMage", animated=True, class_label="Arcanist")
        watcher = self.open_player("Observer", animated=True)
        actor_id = self.projection(actor)["viewerId"]
        for page in (actor, watcher):
            page.evaluate("""() => {
              window.__actionSamples = [];
              const source = window.__valeDebug.worldSource();
              const advance = source.advance.bind(source);
              const observed = [];
              source.advance = (...args) => {
                const projections = advance(...args);
                observed.push(...projections);
                return projections;
              };
              const record = () => {
                const d = window.__valeDebug, p = d.worldSource().latestProjection();
                if (p) window.__actionSamples.push({
                  tick: Number(p.tick), viewerId: p.viewerId, health: p.viewer.health,
                  inCombat: p.viewer.inCombat, events: observed.splice(0).flatMap(snapshot => snapshot.events.map(e => ({...e}))),
                  nodes: window.__acceptanceNodes(`unit-player-${window.__observedActor ?? p.viewerId}`),
                  effects: d.presentationFrame().effectNodes,
                  combatText: [...document.querySelectorAll('.combat-text-entry')].map(e => e.textContent),
                });
                if (window.__actionSamples.length > 256) window.__actionSamples.shift();
                requestAnimationFrame(record);
              };
              requestAnimationFrame(record);
            }""")
        watcher.evaluate("id => window.__observedActor = id", actor_id)
        self.evidence["normalInputRoute"] = self.walk_online_waypoints((actor, watcher), [(-1550,2000),(-3350,2000),(-3350,1800),(-8450,1800),(-8450,1376)])
        for page in (actor, watcher):
            self.wait_for(page, "window.__valeDebug.projectedUnits().some(e => e.kind === 'creature' && e.id === 108)")
        actor.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'select-target',target:{kind:'creature',id:108}})")
        # Digit1 is the ordinary production ability binding; the host resolves it.
        actor.keyboard.press("Digit1")
        self.wait_for(watcher, """id => window.__actionSamples.some(s => s.events.some(e => e.kind === 'cast-started' && e.source.kind === 'player' && e.source.id === id))""", actor_id)
        self.wait_for(watcher, """id => window.__actionSamples.some(s => s.events.some(e => e.kind === 'damage-observed' && e.source.kind === 'player' && e.source.id === id && e.amount > 0))""", actor_id)
        actor.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'start-attack'})")
        self.wait_for(watcher, """id => window.__actionSamples.some(s => s.events.some(e => e.kind === 'attack-performed' && e.source.kind === 'player' && e.source.id === id))""", actor_id, timeout_ms=20_000)
        samples = watcher.evaluate("window.__actionSamples")
        relevant = [sample for sample in samples if any(event.get("source") == {"kind": "player", "id": actor_id} for event in sample["events"])]
        self.assertTrue(relevant)
        self.assertTrue(any(sample["effects"] for sample in relevant), "Real observer frames draw confirmed effects")
        self.assertGreater(len({json.dumps(sample["nodes"], sort_keys=True) for sample in relevant}), 1, "Remote model actually plays through changing nodes")
        for sample in relevant:
            self.assertFalse(any(event["kind"] == "damage-dealt" and event.get("source", {}).get("id") == actor_id for event in sample["events"]), "Watcher is not credited with another actor's damage")
        watcher.screenshot(path=str(ARTIFACTS / "remote-procedural-action.png"))
        actor.evaluate("window.__valeDebug.worldSource().sendCommand({kind:'stop-attack'})")
        before_resume = self.projection(watcher)
        watcher_id = before_resume["viewerId"]
        # A normal accepted Firebolt supplies a live effect immediately before interruption.
        actor.keyboard.press("Digit1")
        self.wait_for(watcher, """() => {
          const d = window.__valeDebug, effects = d.presentationFrame().effectNodes;
          if (!effects.length) return false;
          window.__resumeBefore = effects.map(e => e.id);
          window.__resumeDuring = [];
          const observe = () => {
            if (d.worldSource().linkState() === 'reconnecting') {
              window.__resumeDuring.push(d.presentationFrame().effectNodes.map(e => e.id));
            }
            if (window.__actualTransports.length < 2 || d.worldSource().linkState() !== 'connected') requestAnimationFrame(observe);
          };
          requestAnimationFrame(observe);
          window.__actualTransports.at(-1).close({closeCode:0,reason:'acceptance path interruption'});
          return true;
        }""", timeout_ms=10_000)
        self.wait_for(watcher, "window.__actualTransports.length === 2 && window.__valeDebug.worldSource().linkState() === 'connected'", timeout_ms=15_000)
        self.wait_for(watcher, "tick => window.__valeDebug.projection().tick > tick", before_resume["tick"])
        self.assertEqual(self.projection(watcher)["viewerId"], watcher_id, "Real resume retains the same player")
        self.assertEqual(watcher.evaluate("window.__transportKinds"), [False, True], "Second connection uses the real reconnect route")
        cleanup = watcher.evaluate("""() => ({before:window.__resumeBefore, during:window.__resumeDuring,
          after:window.__valeDebug.presentationFrame().effectNodes.map(e => e.id)})""")
        self.assertTrue(cleanup["before"], "Actual transient geometry was active at interruption")
        self.assertTrue(cleanup["during"], "Observed a real reconnecting frame")
        self.assertTrue(all(not set(ids).intersection(cleanup["before"]) for ids in cleanup["during"]), "Reconnecting frames clear prior effect identities")
        self.assertFalse(set(cleanup["after"]).intersection(cleanup["before"]), "Resume does not replay prior effects; new authoritative effects are allowed")
        resume = {"beforeTick":before_resume["tick"], "after":self.projection(watcher), "transportKinds":watcher.evaluate("window.__transportKinds"), "cleanup":cleanup}
        self.evidence["realReconnect"] = resume
        watcher.screenshot(path=str(ARTIFACTS / "remote-procedural-resumed.png"))
        actor.close()
        self.wait_for(watcher, "id => !window.__valeDebug.projection().entities.some(e => e.kind === 'player' && e.id === id)", actor_id)
        self.wait_for(watcher, "id => window.__valeDebug.unitNodes(`unit-player-${id}`).length === 0", actor_id)
        self.evidence.update({"actorId": actor_id, "remoteActionSamples": relevant, "departedActorNodes": watcher.evaluate("id => window.__valeDebug.unitNodes(`unit-player-${id}`)", actor_id)})

    def test_two_pages_share_one_zone_host(self):
        runner = self.open_player("runner")
        watcher = self.open_player("watcher")
        runner_id = self.projection(runner)["viewerId"]
        watcher_id = self.projection(watcher)["viewerId"]
        self.assertNotEqual(runner_id, watcher_id, "Each page is admitted as its own player")

        # Both players stand on the spawn plaza, inside each other's interest radius.
        self.wait_for(watcher, "id => window.__valeDebug.projection().entities.some(e => e.kind === 'player' && e.id === id)", runner_id)
        self.wait_for(runner, "id => window.__valeDebug.projection().entities.some(e => e.kind === 'player' && e.id === id)", watcher_id)
        start = self.seen(watcher, runner_id)["position"]
        own_start = self.projection(runner)["entities"][0]["position"]
        self.assertEqual(self.projection(runner)["entities"][0]["id"], runner_id)

        # W runs the runner forward; the watcher sees it through its own projections.
        runner.keyboard.down("KeyW")
        self.frames(runner, 30)
        runner.keyboard.up("KeyW")
        self.frames(runner, 4)
        self.wait_for(watcher, """([id, start, distance]) => {
          const unit = window.__valeDebug.projection().entities.find(e => e.kind === 'player' && e.id === id);
          return unit && Math.hypot(unit.position[0] - start[0], unit.position[2] - start[2]) > distance;
        }""", [runner_id, start, MIN_RUN_UNITS])
        moved = self.seen(watcher, runner_id)["position"]
        own_moved = self.projection(runner)["entities"][0]["position"]
        self.assertGreater(abs(own_moved[0] - own_start[0]) + abs(own_moved[2] - own_start[2]), MIN_RUN_UNITS)
        # The watcher did not move: only the runner's intent reached the host.
        self.assertEqual(self.projection(watcher)["entities"][0]["id"], watcher_id)
        expect(watcher.locator("#connection-status")).to_have_text("")
        runner.screenshot(path=str(ARTIFACTS / "runner-moved.png"))
        watcher.screenshot(path=str(ARTIFACTS / "watcher-sees-runner.png"))

        # Closing the runner's page ends its session; the host removes the unit after its grace.
        self.players[0]["page"].close()
        closed_at = time.monotonic()
        self.wait_for(watcher, "id => !window.__valeDebug.projection().entities.some(e => e.kind === 'player' && e.id === id)", runner_id,
                      timeout_ms=15_000)
        gone_after = time.monotonic() - closed_at
        expect(watcher.locator("#character-select")).to_be_hidden()
        self.evidence.update({
            "runnerId": runner_id,
            "watcherId": watcher_id,
            "runnerStartSeenByWatcher": start,
            "runnerMovedSeenByWatcher": moved,
            "runnerOwnStart": own_start,
            "runnerOwnMoved": own_moved,
            "graceSeconds": GRACE_TICKS / TICK_HZ,
            "runnerGoneFromWatcherAfterSeconds": round(gone_after, 2),
            "watcherTick": self.projection(watcher)["tick"],
        })


if __name__ == "__main__":
    unittest.main(verbosity=2)
