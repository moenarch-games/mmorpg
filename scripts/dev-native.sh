#!/usr/bin/env bash
# Start the local native multiplayer environment.
#
# This owns the disposable zone host it starts. Press Ctrl-C or close the client
# window to stop the host. --smoke verifies the same setup without opening a
# persistent window.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT_DIR
readonly DEV_TLS_DIR="$ROOT_DIR/target/dev-tls"
readonly CERTIFICATE_PATH="$DEV_TLS_DIR/cert.pem"
readonly PRIVATE_KEY_PATH="$DEV_TLS_DIR/key.pem"
readonly HOST_LOG_PATH="$ROOT_DIR/target/dev-native/zone-host.log"
readonly TRANSPORT_PORT=4433
readonly STATUS_PORT=8080
readonly ZONE_ID=1

smoke_mode=false

case "${1:-}" in
  "") ;;
  --smoke) smoke_mode=true ;;
  --help|-h)
    printf '%s\n' 'Usage: ./scripts/dev-native.sh [--smoke]'
    printf '%s\n' 'Builds and starts a local zone host, then connects the native client.'
    exit 0
    ;;
  *)
    printf 'Unknown option: %s\n' "$1" >&2
    printf '%s\n' 'Usage: ./scripts/dev-native.sh [--smoke]' >&2
    exit 2
    ;;
esac

for command in cargo curl openssl; do
  if ! command -v "$command" >/dev/null 2>&1; then
    printf 'Required command not found: %s\n' "$command" >&2
    exit 1
  fi
done

# shellcheck source-path=SCRIPTDIR source=lib/dev-tls.sh
source "$ROOT_DIR/scripts/lib/dev-tls.sh"
mkdir -p "$(dirname -- "$HOST_LOG_PATH")"
ensure_dev_certificate "$DEV_TLS_DIR"

cargo build --locked -p mmorpg-client -p mmorpg-game-server

host_pid=''
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ -n "$host_pid" ]] && kill -0 "$host_pid" >/dev/null 2>&1; then
    kill -TERM "$host_pid" 2>/dev/null || true
    wait "$host_pid" || true
  fi
  if (( status != 0 )) && [[ -f "$HOST_LOG_PATH" ]]; then
    printf '\nZone host log (%s):\n' "$HOST_LOG_PATH" >&2
    tail -n 100 "$HOST_LOG_PATH" >&2 || true
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

(
  unset MMORPG_RECOVERY_DIR
  export MMORPG_ZONE_IDS="$ZONE_ID"
  export MMORPG_PORT="$TRANSPORT_PORT"
  export MMORPG_STATUS_PORT="$STATUS_PORT"
  export MMORPG_CERT_PEM="$CERTIFICATE_PATH"
  export MMORPG_KEY_PEM="$PRIVATE_KEY_PATH"
  export MMORPG_ROUTE_PREFIX=/game
  export MMORPG_RECONNECT_GRACE_TICKS=600
  export MMORPG_DRAIN_GRACE_MS=500
  exec cargo run --locked -p mmorpg-game-server --bin mmorpg-zone-host
) >"$HOST_LOG_PATH" 2>&1 &
host_pid=$!

deadline=$((SECONDS + 10))
until curl --fail --silent --max-time 1 \
  "http://127.0.0.1:$STATUS_PORT/readyz" >/dev/null; do
  if ! kill -0 "$host_pid" >/dev/null 2>&1; then
    wait "$host_pid"
  fi
  if (( SECONDS >= deadline )); then
    printf 'Zone host did not become ready within 10 seconds.\n' >&2
    exit 1
  fi
  sleep 0.1
done

client_args=(
  --url "https://localhost:$TRANSPORT_PORT/game/matches/zone-$ZONE_ID"
  --zone "$ZONE_ID"
  --certificate "$CERTIFICATE_PATH"
)
if [[ "$smoke_mode" == true ]]; then
  client_args+=(--smoke)
fi

cargo run --locked -p mmorpg-client -- "${client_args[@]}"
