#!/usr/bin/env bash
# Run the existing browser client against a local Battle Royale host with two rounds.
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT_DIR
readonly MODE_ROOT="${BATTLE_ROYALE_ROOT:-$ROOT_DIR/../battle-royale}"
readonly TRANSPORT_PORT="${MMORPG_DEV_TRANSPORT_PORT:-4433}"
readonly STATUS_PORT="${MMORPG_DEV_STATUS_PORT:-8080}"
readonly WEB_PORT="${MMORPG_DEV_WEB_PORT:-5173}"
readonly TLS_DIR="$ROOT_DIR/target/dev-tls"
readonly LOG_DIR="$ROOT_DIR/target/dev-browser-battle-royale"
if [[ ! -f "$MODE_ROOT/crates/battle-royale-game-server/Cargo.toml" ]]; then
  printf '%s\n' 'Set BATTLE_ROYALE_ROOT to the compatible battle-royale checkout documented in docs/BATTLE_ROYALE.md.' >&2
  exit 1
fi
# shellcheck source-path=SCRIPTDIR source=lib/dev-tls.sh
source "$ROOT_DIR/scripts/lib/dev-tls.sh"
ensure_dev_certificate "$TLS_DIR"
mkdir -p "$LOG_DIR"
certificate_hash="$(dev_certificate_hash "$TLS_DIR/cert.pem" | tr '+/' '-_' | tr -d '=')"
readonly certificate_hash
cargo build --manifest-path "$MODE_ROOT/Cargo.toml" --locked -p battle-royale-game-server --bin battle-royale-host
host_pid=''
web_pid=''
cleanup() {
  local pid
  for pid in "$web_pid" "$host_pid"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT
trap 'exit 130' INT TERM
setsid env -u BATTLE_ROYALE_RECOVERY_DIR BATTLE_ROYALE_CERT_PEM="$TLS_DIR/cert.pem" BATTLE_ROYALE_KEY_PEM="$TLS_DIR/key.pem" \
  BATTLE_ROYALE_MATCH_IDS="${BATTLE_ROYALE_MATCH_IDS:-1,2,3,4}" BATTLE_ROYALE_PORT="$TRANSPORT_PORT" \
  BATTLE_ROYALE_STATUS_PORT="$STATUS_PORT" \
  cargo run --manifest-path "$MODE_ROOT/Cargo.toml" --locked -p battle-royale-game-server --bin battle-royale-host \
  >"$LOG_DIR/host.log" 2>&1 &
host_pid=$!
(cd "$ROOT_DIR/web" && exec setsid bun run dev --host 127.0.0.1 --port "$WEB_PORT") >"$LOG_DIR/web.log" 2>&1 &
web_pid=$!
printf 'Open in two tabs:\nhttp://127.0.0.1:%s/mmorpg/?activity=battle-royale&arenaServer=https://127.0.0.1:%s/game/matches/br-0000000000000001&certHash=%s\n' \
  "$WEB_PORT" "$TRANSPORT_PORT" "$certificate_hash"
printf 'Logs: %s\n' "$LOG_DIR"
wait -n "$host_pid" "$web_pid"
