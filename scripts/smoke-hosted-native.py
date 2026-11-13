#!/usr/bin/env python3
"""Opt-in stage-5 native session/process check with a real local setup service.

Requires coturn (turnserver), or MMORPG_TEST_TURNSERVER pointing to its executable.

Exercises one authoritative world, owner approval, private guest identity, native
WebRTC movement/reconnect, client-independent hosting, removal and durable resume.
This does not verify native window interaction, missing assets or Internet NAT/TURN.
"""
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
    root = Path(__file__).resolve().parent.parent
    setup = root.parent / "multiplayer-setup-service"
    turnserver = os.environ.get("MMORPG_TEST_TURNSERVER") or shutil.which("turnserver")
    if not turnserver:
        raise RuntimeError("Install coturn or set MMORPG_TEST_TURNSERVER to turnserver for this opt-in check.")
    temporary_root = root / "target" / "host-world-tmp"
    temporary_root.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, RUSTC_WRAPPER="", TMPDIR=str(temporary_root))
    subprocess.run(["cargo", "build", "--locked", "-p", "mmorpg-game-server", "-p", "mmorpg-client", "--example", "hosted-client-probe", "--bin", "mmorpg-zone-host"], cwd=root, env=environment, check=True, timeout=900)
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
    processes = []
    logs = []
    success = False

    def spawn(arguments, env, name, capture=True):
        log = (directory / f"{name}.log").open("w")
        logs.append(log)
        process = subprocess.Popen(arguments, cwd=root, env=env, stdout=subprocess.PIPE if capture else log, stderr=log, text=True)
        processes.append(process)
        return process

    def event(process, expected, timeout=120):
        deadline = time.monotonic() + timeout
        with selectors.DefaultSelector() as ready:
            ready.register(process.stdout, selectors.EVENT_READ)
            while time.monotonic() < deadline:
                if not ready.select(0.2):
                    if process.poll() is not None:
                        raise RuntimeError(f"process exited before {expected}; see {directory}")
                    continue
                line = process.stdout.readline()
                if not line:
                    raise RuntimeError(f"process closed output before {expected}; see {directory}")
                value = json.loads(line)
                if value.get("event") == expected:
                    return value
        raise TimeoutError(f"timed out waiting for {expected}; see {directory}")

    def control(mode):
        return subprocess.check_output([str(probe), mode], cwd=root, env=owner_env, text=True, timeout=45)

    def entitlement():
        entries = json.loads((directory / "owner/worlds/default/guest-access.json").read_text())
        assert len(entries) == 1
        return next(iter(entries.values()))["character"]

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
        service_env = dict(base, BIND_ADDR=f"127.0.0.1:{port}", ALLOWED_ORIGINS="https://moenarch.com", ROOM_TTL_SECONDS="60", TURN_URLS=f"turn:127.0.0.1:{turn_port}", TURN_SHARED_SECRET=turn_secret)
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
            control("stop")
        except (subprocess.SubprocessError, OSError):
            pass
        for process in processes:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        for log in logs:
            log.close()
        if success:
            shutil.rmtree(directory)
        else:
            print(f"Focused hosting failure retained at {directory}")


if __name__ == "__main__":
    main()
