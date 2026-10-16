#!/usr/bin/env python3
"""Build and exercise a native client against a disposable local zone host.

Requires OpenSSL and a working GPU backend (a software Vulkan adapter also works).
--window additionally opens a native window for 120 frames. No external services.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def free_port(kind):
    with socket.socket(socket.AF_INET, kind) as reservation:
        reservation.bind(("127.0.0.1", 0))
        return reservation.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--window", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    subprocess.run(["cargo", "build", "--locked", "-p", "mmorpg-client", "-p", "mmorpg-game-server"], cwd=root, check=True, timeout=600)
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"], cwd=root, timeout=30))
    suffix = ".exe" if os.name == "nt" else ""
    binaries = Path(metadata["target_directory"]) / "debug"
    with tempfile.TemporaryDirectory(prefix="mmorpg-native-") as temporary:
        directory = Path(temporary)
        certificate = directory / "cert.pem"
        key = directory / "key.pem"
        subprocess.run([
            "openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
            "-keyout", str(key), "-out", str(certificate), "-sha256", "-days", "10", "-nodes",
            "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1",
        ], check=True, capture_output=True, timeout=15)
        key.chmod(0o600)
        port = free_port(socket.SOCK_DGRAM)
        status_port = free_port(socket.SOCK_STREAM)
        # Do not inherit an existing recovery directory or custom route/zone set.
        environment = {key: value for key, value in os.environ.items() if not key.startswith("MMORPG_")}
        environment.update(MMORPG_ZONE_IDS="1", MMORPG_PORT=str(port), MMORPG_STATUS_PORT=str(status_port),
                           MMORPG_CERT_PEM=str(certificate), MMORPG_KEY_PEM=str(key), MMORPG_DRAIN_GRACE_MS="20")
        with (directory / "server.log").open("w+") as log:
            server = subprocess.Popen([str(binaries / f"mmorpg-zone-host{suffix}")], cwd=root, env=environment, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 10
                opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
                while True:
                    if server.poll() is not None:
                        raise RuntimeError("zone host exited before readiness")
                    try:
                        with opener.open(f"http://127.0.0.1:{status_port}/readyz", timeout=0.5) as response:
                            if response.status == 200:
                                break
                    except (urllib.error.URLError, TimeoutError):
                        pass  # A refused/not-ready endpoint is expected during bounded startup.
                    if time.monotonic() >= deadline:
                        raise TimeoutError("zone host never became ready")
                    time.sleep(0.05)
                command = [str(binaries / f"mmorpg-client{suffix}"), "--url", f"https://localhost:{port}/game/matches/zone-1", "--certificate", str(certificate)]
                subprocess.run([*command, "--smoke"], cwd=root, check=True, timeout=30)
                subprocess.run([
                    "cargo", "test", "--locked", "-p", "mmorpg-client", "--lib",
                    "projected_health_bars_change_gpu_pixels", "--", "--ignored", "--nocapture",
                ], cwd=root, check=True, timeout=120)
                subprocess.run([
                    "cargo", "test", "--locked", "-p", "mmorpg-client", "--lib",
                    "the_projected_combat_hud_changes_gpu_pixels", "--", "--ignored", "--nocapture",
                ], cwd=root, check=True, timeout=120)
                subprocess.run([
                    "cargo", "test", "--locked", "-p", "mmorpg-client", "--lib",
                    "authored_outpost_relief_approaches_render_on_the_native_gpu", "--", "--ignored", "--nocapture",
                ], cwd=root, check=True, timeout=120)
                if args.window:
                    subprocess.run([*command, "--frames", "120"], cwd=root, check=True, timeout=30)
            except BaseException:
                log.flush()
                log.seek(0)
                print(log.read())
                raise
            finally:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=5)


if __name__ == "__main__":
    main()
