#!/usr/bin/env python3
"""Opt-in native hosted-world journey with a real local setup service.

Requires coturn (turnserver), or MMORPG_TEST_TURNSERVER pointing to its executable.

Exercises one authoritative world, owner approval, private guest identity, native
WebRTC movement/reconnect, client-independent hosting, removal and durable resume.
--assets also exercises real window/clipboard actions and missing-asset rendering.
Separate-machine Internet NAT/TURN acceptance remains an explicit second phase.
"""
import argparse
import hashlib
import mmap
import json
import os
from pathlib import Path
import secrets
import selectors
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request


def target(directory):
    result = subprocess.check_output(["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"], cwd=directory, timeout=30)
    return Path(json.loads(result)["target_directory"]) / "debug"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", action="store_true", help="exercise native Host/Copy/paste/Join/Approve windows with absent optional art")
    options = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    setup = root.parent / "multiplayer-setup-service"
    turnserver = os.environ.get("MMORPG_TEST_TURNSERVER") or shutil.which("turnserver")
    if not turnserver:
        raise RuntimeError("Install coturn or set MMORPG_TEST_TURNSERVER to turnserver for this opt-in check.")
    temporary_root = root / "target" / "host-world-tmp"
    temporary_root.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, RUSTC_WRAPPER="", TMPDIR=str(temporary_root))
    subprocess.run(["cargo", "build", "--locked", "-p", "mmorpg-game-server", "-p", "mmorpg-client", "--example", "hosted-client-probe", "--bin", "mmorpg-zone-host", "--bin", "mmorpg-client"], cwd=root, env=environment, check=True, timeout=900)
    subprocess.run(["cargo", "build", "--locked", "--bin", "multiplayer-setup-service"], cwd=setup, env=environment, check=True, timeout=300)
    probe = target(root) / "examples" / "hosted-client-probe"
    api = target(setup) / "multiplayer-setup-service"
    directory = Path(tempfile.mkdtemp(prefix="native-hosting-", dir=temporary_root))
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    endpoint = f"http://127.0.0.1:{port}"
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        turn_port = reservation.getsockname()[1]
    base = {key: value for key, value in environment.items() if not key.startswith("MMORPG_")}
    base.update(MMORPG_TEST_SETUP_ENDPOINT=endpoint, MMORPG_WEBRTC_BIND_IP="127.0.0.1")
    owner_env = dict(base, MMORPG_CONFIG_DIR=str(directory / "owner"))
    guest_env = dict(base, MMORPG_CONFIG_DIR=str(directory / "guest"))
    if options.assets:
        owner_install = directory / "owner-install"
        owner_install.mkdir()
        for name in ("mmorpg-client", "mmorpg-zone-host"):
            os.link(target(root) / name, owner_install / name)
        shutil.copytree(root / "web/public/scenery/greyhaven.ambient-animals", owner_install / "assets/ambient")
        guest_install = directory / "guest-install"
        guest_install.mkdir()
        client = guest_install / "mmorpg-client"
        os.link(target(root) / "mmorpg-client", client)
        assert not (guest_install / "assets").exists()
        bundle = json.loads((root / "web/public/scenery/greyhaven.ambient-animals/manifest.json").read_text())
        cat = next(asset for asset in bundle["assets"] if asset["key"] == "greyhaven.ambient.cat" and asset["variant"] == "default")
        resource = root / "web/public/scenery/greyhaven.ambient-animals" / cat["resources"][0]["path"]
        raw = resource.read_bytes()
        assert len(raw) == 146384 and hashlib.sha256(raw).hexdigest() == cat["resources"][0]["sha256"]
        with client.open("rb") as executable, mmap.mmap(executable.fileno(), 0, access=mmap.ACCESS_READ) as image:
            assert image.find(raw) < 0, "guest binary embeds the supposedly missing cat"
        assert not (directory / "guest/asset-cache").exists()

    processes = []
    logs = []
    event_logs = {}
    success = False

    def spawn(arguments, env, name, capture=True):
        log = (directory / f"{name}.log").open("w")
        logs.append(log)
        process = subprocess.Popen(arguments, cwd=root, env=env, stdout=subprocess.PIPE if capture else log, stderr=log, text=True)
        processes.append(process)
        if capture:
            events = (directory / f"{name}-events.jsonl").open("w")
            logs.append(events)
            event_logs[process.pid] = events
        return process

    pending_events = {}
    output_buffers = {}

    def event(process, expected, timeout=120):
        deadline = time.monotonic() + timeout
        pending = pending_events.setdefault(process.pid, [])
        buffered = output_buffers.setdefault(process.pid, bytearray())
        with selectors.DefaultSelector() as ready:
            ready.register(process.stdout, selectors.EVENT_READ)
            while time.monotonic() < deadline:
                for index, value in enumerate(pending):
                    if value.get("event") == expected:
                        return pending.pop(index)
                if not ready.select(0.2):
                    if process.poll() is not None:
                        raise RuntimeError(f"process exited before {expected}; see {directory}")
                    continue
                chunk = os.read(process.stdout.fileno(), 65536)
                if not chunk:
                    raise RuntimeError(f"process closed output before {expected}; see {directory}")
                event_logs[process.pid].write(chunk.decode(errors="replace"))
                event_logs[process.pid].flush()
                buffered.extend(chunk)
                while b"\n" in buffered:
                    line, _, rest = buffered.partition(b"\n")
                    buffered[:] = rest
                    pending.append(json.loads(line))
        raise TimeoutError(f"timed out waiting for {expected}; see {directory}")

    def wait_for(predicate, timeout, description):
        deadline = time.monotonic() + timeout
        while True:
            value = predicate()
            if value:
                return value
            if time.monotonic() >= deadline:
                raise TimeoutError(f"timed out waiting for {description}; see {directory}")
            time.sleep(0.1)

    def control(mode):
        return subprocess.check_output([str(probe), mode], cwd=root, env=owner_env, text=True, timeout=45)

    def stop_world():
        # The control endpoint closes before final replay/save releases the world lock.
        # Observe that existing lock through the real stop command; never kill the writer.
        deadline = time.monotonic() + 120
        while True:
            result = subprocess.run([str(probe), "stop"], cwd=root, env=owner_env,
                                    capture_output=True, text=True, timeout=45)
            if result.returncode == 0:
                return
            if "World is still stopping" not in result.stderr and "Connection refused" not in result.stderr:
                raise RuntimeError(result.stderr)
            if time.monotonic() >= deadline:
                raise TimeoutError("host did not release its save lock after graceful stop")
            time.sleep(0.2)

    def entitlement():
        entries = json.loads((directory / "owner/worlds/default/guest-access.json").read_text())
        assert len(entries) == 1
        return next(iter(entries.values()))["character"]

    def status():
        result = subprocess.run([str(probe), "status"], cwd=root, env=owner_env,
                                capture_output=True, text=True, timeout=5)
        return json.loads(result.stdout) if result.returncode == 0 else {"invitation":None,"guests":[]}

    def asset_journey():
        # All pointer/clipboard operations stay on our private display. No semantic
        # UI bypass: Host, Copy, paste, Join and Approve use the shipped widgets.
        import sys
        sys.path.insert(0, str(root / "scripts/lib"))
        from native_window import Pointer
        xvfb = shutil.which("Xvfb")
        if not xvfb:
            raise RuntimeError("The opt-in native window journey requires Xvfb and Pillow.")
        display_number = next(n for n in range(153, 253)
                              if not Path(f"/tmp/.X11-unix/X{n}").exists()
                              and not Path(f"/tmp/.X{n}-lock").exists())
        display = f":{display_number}"
        server = spawn([xvfb, display, "-screen", "0", "1280x800x24", "-nolisten", "tcp"], base, "xvfb", False)
        wait_for(lambda: Path(f"/tmp/.X11-unix/X{display_number}").exists(), 10, "isolated display")
        if server.poll() is not None:
            raise RuntimeError("Xvfb failed; inspect xvfb.log (its keyboard compiler uses /tmp).")
        pointer = Pointer(display)
        def window_env(env, image=""):
            result = dict(env, DISPLAY=display, WINIT_UNIX_BACKEND="x11", MMORPG_SMOKE_FRAME=image)
            result.pop("WAYLAND_DISPLAY", None)
            result.pop("WAYLAND_SOCKET", None)
            return result
        def roster(env, name, class_id, sex, hat):
            config = Path(env["MMORPG_CONFIG_DIR"])
            config.mkdir(parents=True, exist_ok=True)
            path = config / "roster.json"
            if not path.exists():
                path.write_text(json.dumps({"schemaVersion":1,"characters":[{"id":"local-1","name":name,"classId":class_id,"sex":sex,"appearance":{"hat":hat}}]}))
        roster(owner_env, "Hosted Warden", "warden", "male", "ironcrest-helm")
        roster(guest_env, "Invited Ranger", "ranger", "female", "ranger-cap")
        def window(process, previous):
            event(process, "native_frontend_frame")
            found = wait_for(lambda: next((w for w, title in pointer.windows()
                                          if title.startswith("MMORPG") and w not in previous), None), 10, "native window")
            pointer.focus(found)
            pointer.click(640, 400)  # The ordinary click-to-skip splash.
            time.sleep(0.3)
            return found
        owner = spawn([str(owner_install / "mmorpg-client")], window_env(owner_env), "window-owner")
        owner_window = window(owner, set())
        pointer.click(90, 493)  # Host World; certificate/endpoint never entered.
        wait_for(lambda: status().get("invitation"), 45, "ready registered native world")
        owner_ready = event(owner, "native_world_ready", 120)
        pointer.tap("Escape")
        time.sleep(0.5)
        pointer.click(580, 387)  # Copy invitation in the native game menu.
        invitation = wait_for(pointer.clipboard, 5, "native OS clipboard invitation")
        assert invitation == status()["invitation"], "Copy did not supply the current native invitation"
        print(json.dumps({"phase":"native_host_and_copy","copied_current_invitation":True}), flush=True)
        def guest(name, invite, capture=False, paste=False):
            arguments = [str(client)] + ([] if paste else ["--join-invite", invite])
            image = str(directory / "received-cat.png") if capture else ""
            previous = {w for w, _ in pointer.windows()}
            process = spawn(arguments, window_env(guest_env, image), name)
            shown = window(process, previous)
            if paste:
                pointer.click(180, 561)
                time.sleep(0.3)
                pointer.paste()
            pointer.click(90, 595)  # Join World through the same strict parser as CLI.
            return process, shown
        cold, cold_window = guest("window-cold", invitation, capture=True, paste=True)
        wait_for(lambda: any(g["pending"] for g in status()["guests"]), 15, "signed native admission request")
        pointer.focus(owner_window)
        time.sleep(1.1)  # Existing one-second observed hosting probe populates the menu.
        pointer.click(715, 404)  # Approve the displayed guest code.
        wait_for(lambda: all(not g["pending"] for g in status()["guests"]), 10, "native owner approval")
        pointer.focus(cold_window)
        event(cold, "native_world_resident", 120)
        pointer.key("w")
        cold_ready = event(cold, "native_world_ready")
        cold_detail = event(cold, "native_detail_ready")
        pointer.key("w", False)
        assert cold_detail["files_verified"] == 2 and cold_detail["bytes_received"] == 180265
        assert cold_detail["snapshots_while_loading"] > 0 and cold_detail["moved_while_loading"]
        def positions(receipt):
            return {player["id"]:player["position"] for player in receipt["players"]}
        def settled(process, after):
            while True:
                receipt = event(process, "native_movement_settled", 60)
                if receipt["tick"] >= after:
                    return receipt
        after = cold_detail["tick"]
        first_owner = settled(owner, after)
        first_guest = settled(cold, after)
        assert positions(first_owner) == positions(first_guest), "clients disagree after guest movement settles"
        assert positions(first_guest)[cold_ready["viewer"]] != positions(cold_ready)[cold_ready["viewer"]]
        rendered = event(cold, "native_detail_rendered", 120)
        assert rendered["cat_draws"] > 0 and (directory / "received-cat.png").is_file()
        pointer.focus(owner_window)
        pointer.tap("Escape")  # Resume the world, then move its actual native owner.
        pointer.key("w")
        time.sleep(2)
        pointer.key("w", False)
        second_owner = settled(owner, max(first_owner["tick"], first_guest["tick"]) + 1)
        second_guest = settled(cold, max(first_owner["tick"], first_guest["tick"]) + 1)
        assert positions(second_owner) == positions(second_guest), "clients disagree after owner movement settles"
        assert positions(second_owner)[owner_ready["viewer"]] != positions(first_owner)[owner_ready["viewer"]]
        print(json.dumps({"phase":"cold_native_world","detail":cold_detail,"rendered":rendered,
                          "settled_agreement":second_guest}), flush=True)
        character = entitlement()
        saved_position = positions(second_guest)[cold_ready["viewer"]]
        pointer.close(cold_window)
        assert cold.wait(timeout=15) == 0
        pointer.close(owner_window)
        assert owner.wait(timeout=15) == 0
        # The owner client is gone; independently managed hosting remains authoritative.
        warm, warm_window = guest("window-warm", invitation)
        warm_ready = event(warm, "native_world_ready", 120)
        warm_detail = event(warm, "native_detail_ready")
        assert warm_detail["bytes_received"] == 0 and warm_detail["cache_hits"] == 2
        assert entitlement() == character
        assert sum(abs(a-b) for a,b in zip(positions(warm_ready)[warm_ready["viewer"]], saved_position)) < 150
        pointer.close(warm_window)
        assert warm.wait(timeout=15) == 0
        cache_file = directory / "guest/asset-cache/sha256" / cat["resources"][0]["sha256"]
        cache_file.write_bytes(b"corrupted guest cache")
        repaired, repaired_window = guest("window-repaired", invitation)
        event(repaired, "native_world_ready", 120)
        repaired_detail = event(repaired, "native_detail_ready")
        assert repaired_detail["bytes_received"] == 146384 and repaired_detail["cache_hits"] == 1
        pointer.close(repaired_window)
        assert repaired.wait(timeout=15) == 0
        # Only this test executable corrupts a normal catalogue response. Production
        # continues serving the immutable installed closure and needs no fault switch.
        corrupt_host = spawn([str(probe), "corrupt-host"], dict(owner_env, MMORPG_SHARED_ART_DIR=str(owner_install / "assets/ambient")), "corrupt-host")
        corrupt_invitation = event(corrupt_host, "corrupt_host_ready")["invitation"]
        cache_file.unlink()
        corrupt, corrupt_window = guest("window-corrupt-download", corrupt_invitation)
        event(corrupt, "native_world_ready", 120)
        failure = event(corrupt, "native_detail_failed")
        assert "hash verification" in failure["error"] and "unverified bytes were not used" in failure["error"]
        assert not cache_file.exists()
        pointer.focus(corrupt_window)
        pointer.key("w")
        time.sleep(2)
        pointer.key("w", False)
        event(corrupt, "native_movement_settled")  # Verification refusal leaves play usable.
        pointer.close(corrupt_window)
        assert corrupt.wait(timeout=15) == 0
        corrupt_host.terminate()
        corrupt_host.wait(timeout=10)
        assert status()["invitation"] == invitation
        control("remove")
        revoked, revoked_window = guest("window-revoked", invitation)
        refusal = event(revoked, "native_entry_failed", 120)
        assert "could not admit" in refusal["error"]
        pointer.close(revoked_window)
        assert revoked.wait(timeout=15) == 0
        stop_world()
        print(json.dumps({"event":"hosted_assets_passed","native_window_actions":True,
                          "warm":warm_detail,"corrupt_cache_refetch":repaired_detail,
                          "corrupt_received_hash_refused":True,"revoked_invitation":True,
                          "owner_client_exit":True,"same_durable_character":True,
                          "two_network_acceptance":False,"artifacts":str(directory)}), flush=True)

    try:
        turn_secret = secrets.token_hex(32)
        turn_config = directory / "turnserver.conf"
        turn_config.write_text("\n".join([
            "listening-ip=127.0.0.1", "relay-ip=127.0.0.1", f"listening-port={turn_port}",
            "min-port=49160", "max-port=49200", "realm=native-hosting.invalid", "fingerprint", "use-auth-secret",
            f"static-auth-secret={turn_secret}", "allow-loopback-peers", "no-multicast-peers", "no-cli", "no-tls", "no-dtls",
            "log-file=stdout", f"pidfile={directory / 'turn.pid'}", "relay-threads=1",
        ]) + "\n")
        turn_config.chmod(0o600)
        turn_env = dict(base)
        if os.environ.get("MMORPG_TEST_TURN_LIBRARY_PATH"):
            turn_env["LD_LIBRARY_PATH"] = os.environ["MMORPG_TEST_TURN_LIBRARY_PATH"]
        spawn([turnserver, "-c", str(turn_config)], turn_env, "turn", capture=False)
        service_env = dict(base, BIND_ADDR=f"127.0.0.1:{port}", ALLOWED_ORIGINS="https://moenarch.com", ROOM_TTL_SECONDS="300" if options.assets else "60", TURN_URLS=f"turn:127.0.0.1:{turn_port}", TURN_SHARED_SECRET=turn_secret)
        service = spawn([str(api)], service_env, "setup", capture=False)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        deadline = time.monotonic() + 10
        while True:
            try:
                with opener.open(f"{endpoint}/health", timeout=0.5) as response:
                    assert response.status == 200
                    break
            except OSError:
                if time.monotonic() >= deadline:
                    raise TimeoutError("local setup service did not start")
                time.sleep(0.1)
        if options.assets:
            asset_journey()
            success = True
            return
        owner = spawn([str(probe), "owner"], owner_env, "owner")
        ready = event(owner, "hosting_ready")
        guest = spawn([str(probe), "guest", ready["invitation"]], guest_env, "guest")
        first = event(guest, "guest_ready")
        assert owner.wait(timeout=10) == 0
        assert first["continuedWithoutOwner"]
        character = entitlement()
        assert character != "owner-probe"
        control("remove")
        event(guest, "revocation_passed", 30)
        assert guest.wait(timeout=5) == 0
        control("stop")
        replacement = json.loads(control("start"))["invitation"]
        control("approve")
        returning = spawn([str(probe), "resume", replacement], guest_env, "returning")
        resumed = event(returning, "guest_ready")
        assert returning.wait(timeout=10) == 0
        assert entitlement() == character
        assert resumed["tick"] >= first["tick"]
        assert sum(abs(a - b) for a, b in zip(resumed["initial"], first["position"])) < 150, "returning guest did not resume its saved position"
        control("stop")
        stale = spawn([str(probe), "stale", replacement], guest_env, "stale")
        event(stale, "stale_invitation_rejected", 30)
        assert stale.wait(timeout=5) == 0
        control("start")
        service.terminate()
        service.wait(timeout=10)
        deadline = time.monotonic() + 10
        while True:
            status = json.loads(control("status"))
            if status["invitation"] is None:
                break
            if time.monotonic() >= deadline:
                raise TimeoutError("failed signaling still advertised an invitation")
            time.sleep(0.2)
        control("stop")
        print(json.dumps({"event":"hosted_native_passed", "relayed":first["relayed"], "guestTick":first["tick"], "resumedTick":resumed["tick"], "sameDurableCharacter":True, "ownerClientExit":True, "revocation":True, "staleInvitation":True, "failedSignalingHidesInvite":True, "missingAssetsVerified":False, "twoNetworkAcceptance":False}))
        success = True
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
        try:
            stop_world()
        except (subprocess.SubprocessError, OSError, RuntimeError, TimeoutError):
            pass
        for process in processes:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        for log in logs:
            log.close()
        if success and not options.assets:
            shutil.rmtree(directory)
        else:
            outcome = "artifacts" if success else "failure"
            print(f"Focused hosting {outcome} retained at {directory}")


if __name__ == "__main__":
    main()
