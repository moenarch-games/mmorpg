#!/usr/bin/env python3
"""Two-host handoff smoke (#145): the native client walks and teleports between zone hosts.

Starts a disposable PostgreSQL 17 container (or uses --database-url), two trusted-LAN
handoff hosts for Seam Test West (901) and East (902) sharing it, and runs the native
client's --autopilot against West: the character walks through the seam to East and
teleports back to West by waystone, each time following the redirect notice to the
other host. Requires Docker (unless --database-url), OpenSSL and the client's bundles;
no GPU.
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

POSTGRES_IMAGE = "postgres:17"


def free_port(kind):
    with socket.socket(socket.AF_INET, kind) as reservation:
        reservation.bind(("127.0.0.1", 0))
        return reservation.getsockname()[1]


def wait_ready(process, status_port, name):
    deadline = time.monotonic() + 60
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    while True:
        if process.poll() is not None:
            raise RuntimeError(f"{name} exited before readiness")
        try:
            with opener.open(f"http://127.0.0.1:{status_port}/readyz", timeout=0.5) as response:
                if response.status == 200:
                    return
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            pass  # Not ready yet during bounded startup.
        if time.monotonic() >= deadline:
            raise TimeoutError(f"{name} never became ready")
        time.sleep(0.1)


def start_postgres():
    """A throwaway PostgreSQL 17 container; returns its URL and container ID."""
    port = free_port(socket.SOCK_STREAM)
    container = subprocess.check_output([
        "docker", "run", "-d", "--rm", "-e", "POSTGRES_PASSWORD=smoke", "-e", "POSTGRES_DB=mmorpg",
        "-p", f"127.0.0.1:{port}:5432", POSTGRES_IMAGE,
    ], text=True, timeout=300).strip()
    deadline = time.monotonic() + 60
    while subprocess.run(
        ["docker", "exec", container, "pg_isready", "-U", "postgres", "-d", "mmorpg", "-h", "127.0.0.1"],
        capture_output=True, timeout=10,
    ).returncode != 0:
        if time.monotonic() >= deadline:
            raise TimeoutError("PostgreSQL never became ready")
        time.sleep(0.2)
    return f"postgres://postgres:smoke@127.0.0.1:{port}/mmorpg", container


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database-url", help="an existing PostgreSQL database the hosts may migrate")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    subprocess.run([str(root / "scripts" / "fetch-bundles.sh")], cwd=root, check=True, timeout=600)
    subprocess.run([str(root / "scripts" / "prepare-ranger-native.sh")], cwd=root, check=True, timeout=600)
    subprocess.run([
        "cargo", "build", "--locked", "-p", "mmorpg-client", "-p", "mmorpg-game-server",
        "--features", "mmorpg-game-server/postgres",
    ], cwd=root, check=True, timeout=1800)
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"], cwd=root, timeout=30))
    suffix = ".exe" if os.name == "nt" else ""
    binaries = Path(metadata["target_directory"]) / "debug"
    container = None
    processes = []
    with tempfile.TemporaryDirectory(prefix="mmorpg-handoff-") as temporary:
        directory = Path(temporary)
        try:
            url = args.database_url
            if url is None:
                url, container = start_postgres()
            certificate = directory / "cert.pem"
            key = directory / "key.pem"
            subprocess.run([
                "openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
                "-keyout", str(key), "-out", str(certificate), "-sha256", "-days", "10", "-nodes",
                "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1",
            ], check=True, capture_output=True, timeout=15)
            key.chmod(0o600)
            zones = {"west": 901, "east": 902}
            ports = {name: free_port(socket.SOCK_DGRAM) for name in zones}
            links = {name: free_port(socket.SOCK_STREAM) for name in zones}
            status = {name: free_port(socket.SOCK_STREAM) for name in zones}

            def route(name):
                return f"https://localhost:{ports[name]}/game/matches/zone-{zones[name]}"

            base = {key: value for key, value in os.environ.items() if not key.startswith("MMORPG_")}
            for name, peer in (("west", "east"), ("east", "west")):
                environment = dict(base)
                environment.update(
                    MMORPG_HANDOFF_DATABASE_URL=url, MMORPG_HOST_ID=f"smoke-{name}",
                    MMORPG_ZONE_IDS=str(zones[name]), MMORPG_PORT=str(ports[name]),
                    MMORPG_STATUS_PORT=str(status[name]), MMORPG_DRAIN_GRACE_MS="0",
                    MMORPG_CERT_PEM=str(certificate), MMORPG_KEY_PEM=str(key),
                    MMORPG_TRANSFER_LINK=f"127.0.0.1:{links[name]}",
                    MMORPG_ZONE_ROUTES=f"{zones[peer]}=127.0.0.1:{links[peer]}@{route(peer)}",
                )
                log = (directory / f"{name}.log").open("w+")
                process = subprocess.Popen([str(binaries / f"mmorpg-zone-host{suffix}")], cwd=root, env=environment, stdout=log, stderr=log)
                processes.append((name, process, log))
            for name, process, _ in processes:
                wait_ready(process, status[name], f"{name} host")
            character = f"seam-smoke-{time.time_ns()}"
            subprocess.run([
                str(binaries / f"mmorpg-client{suffix}"), "--url", route("west"), "--zone", "901",
                "--certificate", str(certificate), "--class", "warden", "--character-id", character,
                "--autopilot",
            ], cwd=root, check=True, timeout=180)
        except BaseException:
            for name, _, log in processes:
                log.flush()
                log.seek(0)
                print(f"--- {name} host ---\n{log.read()}")
            raise
        finally:
            for _, process, log in processes:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
                log.close()
            if container is not None:
                subprocess.run(["docker", "stop", container], capture_output=True, timeout=60)


if __name__ == "__main__":
    main()
