#!/usr/bin/env python3
"""Own the native development build, disposable host, client and source watch."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import json

ROOT = Path(__file__).resolve().parents[2]
LAUNCHER = ROOT / "scripts/dev-native.sh"
TLS = ROOT / "target/dev-tls"
HOST_LOG = ROOT / "target/dev-native/zone-host.log"
POLL_SECONDS = 0.25
LAUNCHER_INPUTS = (
    "scripts/dev-native.sh", "scripts/lib/dev-native.py", "scripts/lib/dev-tls.sh",
)
NATIVE_ASSET_INPUTS = (
    "web/assets/medieval-character-kit/archer.obj",
    "web/assets/medieval-character-kit/manifest.json",
    "web/assets/medieval-character-kit/materials.json",
)


class LaunchError(Exception):
    pass


def say(message):
    print(message, flush=True)


def stop(process):
    if process is None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait()
        return
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        pass
    # A parent can exit before its workers. Retire its whole owned group.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def run(arguments, timeout, *, capture=False, env=None, merge_stderr=True):
    process = subprocess.Popen(
        arguments, cwd=ROOT, env=env, start_new_session=True, text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.STDOUT if capture and merge_stderr else None,
    )
    try:
        output, _ = process.communicate(timeout=timeout)
        return subprocess.CompletedProcess(arguments, process.returncode, output or "")
    except subprocess.TimeoutExpired as error:
        raise LaunchError(f"{arguments[0]} exceeded its {timeout}s timeout") from error
    finally:
        stop(process)


def git(*arguments):
    result = run(["git", *arguments], 30, capture=True)
    if result.returncode:
        raise LaunchError(result.stdout.strip())
    return result.stdout.strip()


def relaunch():
    # Source refresh may have replaced this running module or its shell entrypoint.
    arguments = sys.argv[1:]
    if "--no-update" not in arguments:
        arguments = [*arguments, "--no-update"]
    os.execv(str(LAUNCHER), [str(LAUNCHER), *arguments])


def update_source():
    say("Fetching the latest merged source from origin's default branch…")
    environment = dict(os.environ, GIT_TERMINAL_PROMPT="0")
    try:
        fetched = run(["git", "fetch", "--no-tags", "origin", "HEAD"], 30,
                      capture=True, env=environment)
        if fetched.returncode:
            raise LaunchError(fetched.stdout.strip())
        revision = git("rev-parse", "--verify", "FETCH_HEAD^{commit}")
        included = run(["git", "merge-base", "--is-ancestor", revision, "HEAD"],
                       30, capture=True)
        if included.returncode == 0:
            say(f"Checkout already includes the latest merged revision {revision[:12]}.")
            return
        if included.returncode != 1:
            raise LaunchError(included.stdout.strip())
        if git("status", "--porcelain", "--untracked-files=normal"):
            raise LaunchError("Local changes must be preserved; the checkout needs an update.")
        forward = run(["git", "merge-base", "--is-ancestor", "HEAD", revision],
                      30, capture=True)
        if forward.returncode:
            raise LaunchError("The local branch diverges from the latest merged source.")
        git("merge", "--ff-only", revision)
        say(f"Updated to merged revision {revision[:12]}.")
        relaunch()
    except LaunchError as error:
        raise LaunchError(f"Cannot safely update: {error}\n"
                          "Integrate the latest source yourself, or use --no-update "
                          "to run the current local checkout.") from error


def inputs():
    paths = [ROOT / name for name in (
        "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain",
        *LAUNCHER_INPUTS, *NATIVE_ASSET_INPUTS,
    )]
    for directory in (ROOT / "crates", ROOT / ".cargo"):
        for current, directories, files in os.walk(directory):
            directories[:] = [name for name in directories
                              if name not in {"target", ".git", "__pycache__", "node_modules"}]
            paths.extend(Path(current) / name for name in files)
    result = {}
    for path in paths:
        try:
            result[str(path.relative_to(ROOT))] = hashlib.sha256(path.read_bytes()).digest()
        except FileNotFoundError:
            # Atomic saves and deletions are reconciled by the next snapshot.
            continue
    return result


def wait_for_edit(previous):
    while inputs() == previous:
        time.sleep(POLL_SECONDS)


def build():
    say("Building the current native source…")
    result = run([
        "cargo", "build", "--locked", "-p", "mmorpg-client", "-p", "mmorpg-game-server",
        "--bins", "--message-format=json-render-diagnostics",
    ], 1800, capture=True, merge_stderr=False)
    binaries = {}
    for line in result.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            raise LaunchError(f"Cargo emitted invalid build output: {line}") from error
        if message.get("reason") == "compiler-message":
            rendered = message.get("message", {}).get("rendered")
            if rendered:
                print(rendered, file=sys.stderr, end="", flush=True)
        target = message.get("target", {})
        if (message.get("reason") == "compiler-artifact" and "bin" in target.get("kind", [])
                and target.get("name") in {"mmorpg-client", "mmorpg-zone-host"}
                and message.get("executable")):
            binaries[target["name"]] = message["executable"]
    if result.returncode:
        raise LaunchError("Native build failed.")
    if set(binaries) != {"mmorpg-client", "mmorpg-zone-host"}:
        raise LaunchError("Cargo did not report both native executable artifacts.")
    return binaries


def host_failure(host):
    if host.poll() is not None:
        raise LaunchError(f"Zone host exited ({host.returncode}); see {HOST_LOG}.")


def serve(binaries, smoke, watch, source):
    host = client = None
    environment = dict(os.environ)
    environment.pop("MMORPG_RECOVERY_DIR", None)
    environment.update({
        "MMORPG_ZONE_IDS": "1", "MMORPG_PORT": "4433", "MMORPG_STATUS_PORT": "8080",
        "MMORPG_CERT_PEM": str(TLS / "cert.pem"), "MMORPG_KEY_PEM": str(TLS / "key.pem"),
        "MMORPG_ROUTE_PREFIX": "/game", "MMORPG_RECONNECT_GRACE_TICKS": "600",
        "MMORPG_DRAIN_GRACE_MS": "500",
    })
    try:
        with HOST_LOG.open("w") as log:
            host = subprocess.Popen([binaries["mmorpg-zone-host"]], cwd=ROOT, env=environment,
                                    stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        deadline = time.monotonic() + 10
        while True:
            host_failure(host)
            ready = run(["curl", "--fail", "--silent", "--max-time", "1",
                         "http://127.0.0.1:8080/readyz"], 2, capture=True)
            if ready.returncode == 0:
                host_failure(host)
                break
            if time.monotonic() >= deadline:
                raise LaunchError("Zone host did not become ready within 10 seconds.")
            time.sleep(0.1)
        arguments = [binaries["mmorpg-client"], "--url", "https://localhost:4433/game/matches/zone-1",
                     "--zone", "1", "--certificate", str(TLS / "cert.pem")]
        if smoke:
            arguments.append("--smoke")
        client = subprocess.Popen(arguments, cwd=ROOT, start_new_session=True)
        say("Native client started. Local source changes rebuild and restart the disposable zone."
            if watch else "Native client started.")
        while True:
            if client.poll() is not None:
                return client.returncode
            host_failure(host)
            if watch and inputs() != source:
                say("Source changed; stopping the previous build before rebuilding.")
                return None
            time.sleep(POLL_SECONDS)
    except LaunchError:
        if HOST_LOG.exists():
            print(HOST_LOG.read_text()[-12000:], file=sys.stderr)
        raise
    finally:
        stop(client)
        stop(host)


def main():
    parser = argparse.ArgumentParser(prog="./scripts/dev-native.sh",
        description="Update, build and watch the local native host and client.")
    parser.add_argument("--smoke", action="store_true", help="verify once without a persistent window")
    parser.add_argument("--no-update", action="store_true", help="use the local checkout without fetching Git")
    parser.add_argument("--no-watch", action="store_true", help="build and launch once without watching edits")
    args = parser.parse_args()
    for command in ("cargo", "curl", "openssl", "bash", *([] if args.no_update else ["git"])):
        if shutil.which(command) is None:
            raise LaunchError(f"Required command not found: {command}")
    if not args.no_update:
        update_source()
    HOST_LOG.parent.mkdir(parents=True, exist_ok=True)
    certificate = run(["bash", "-euo", "pipefail", "-c",
                       'source "$1"; ensure_dev_certificate "$2"',
                       "dev-native", str(ROOT / "scripts/lib/dev-tls.sh"), str(TLS)], 30)
    if certificate.returncode:
        raise LaunchError("Could not prepare development TLS credentials.")
    watch = not (args.no_watch or args.smoke)
    while True:
        source = inputs()
        try:
            binaries = build()
        except LaunchError as error:
            if not watch:
                raise
            print(f"{error} Fix the source; watching for the next edit.", file=sys.stderr, flush=True)
            wait_for_edit(source)
            continue
        current = inputs()
        if any(current.get(name) != source.get(name) for name in LAUNCHER_INPUTS):
            relaunch()
        if current != source:
            say("Source changed during the build; rebuilding before starting anything.")
            continue
        result = serve(binaries, args.smoke, watch, source)
        if result is not None:
            return result
        current = inputs()
        if any(current.get(name) != source.get(name) for name in LAUNCHER_INPUTS):
            relaunch()


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda number, _frame: sys.exit(128 + number))
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        say("Stopped native development processes.")
        sys.exit(130)
    except (LaunchError, OSError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
