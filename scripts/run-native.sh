#!/usr/bin/env bash
# Build the current checkout as an optimised, standalone native game and launch it.
#
# No source watching, no Git update and no disposable development host: the `dist` Cargo
# profile (release, fat LTO, one codegen unit, stripped) builds `mmorpg-client` and
# `mmorpg-zone-host`, `scripts/package-native.py` installs them side by side with the host's
# optional art in a self-contained folder, and the game starts from that folder. Choose a
# character and Host World there: the client starts its sibling zone host itself (see
# docs/NATIVE_CLIENT.md#host-world). The folder runs without this repository.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT_DIR

usage() {
  cat <<EOF
usage: $0 [--out DIR] [--no-fetch] [--build-only] [-- CLIENT_ARGS...]

  --out DIR      install the packaged game here (default: dist/mmorpg-native)
  --no-fetch     skip scripts/fetch-bundles.sh and embed the bundles already on disk
  --build-only   build and package, but do not launch
  CLIENT_ARGS    passed to mmorpg-client
EOF
}

out="$ROOT_DIR/dist/mmorpg-native"
fetch=1
launch=1
client_args=()
while (($#)); do
  case "$1" in
    --out)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      out="$(realpath -m -- "$2")"
      shift 2
      ;;
    --no-fetch) fetch=0; shift ;;
    --build-only) launch=0; shift ;;
    -h | --help) usage; exit 0 ;;
    --) shift; client_args=("$@"); break ;;
    *) usage >&2; exit 2 ;;
  esac
done

cd -- "$ROOT_DIR"
for command in cargo python3; do
  command -v "$command" >/dev/null || { echo "Required command not found: $command" >&2; exit 1; }
done

if ((fetch)); then
  echo "Fetching the asset bundles the native client embeds…"
  scripts/fetch-bundles.sh
fi

echo "Building the optimised native game (profile dist; the first build takes a while)…"
cargo build --locked --profile dist -p mmorpg-client -p mmorpg-game-server --bins

# package-native.py never overwrites an installation: package next to it, then swap it in.
staging="$out.new"
rm -rf -- "$staging"
python3 scripts/package-native.py "$staging" --bin-dir "$ROOT_DIR/target/dist" >/dev/null
rm -rf -- "$out"
mv -- "$staging" "$out"
echo "Packaged game: $out/mmorpg-client"

if ((launch)); then
  cd -- "$out"
  exec ./mmorpg-client "${client_args[@]}"
fi
