# shellcheck shell=bash
# Authenticated GitHub release downloads, sourced by the fetch and CI install scripts. A token from
# GH_TOKEN or GITHUB_TOKEN selects the REST API. Callers fall back to the public URL without one.
# The token reaches curl through a process-substitution config, never its arguments.

GITHUB_RELEASE_TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
GITHUB_RELEASE_API=https://api.github.com/repos

# Reports on stderr which path this run's release downloads take.
github_release_report_mode() {
  if [[ -n "${GITHUB_RELEASE_TOKEN}" ]]; then
    echo "Release downloads: authenticated GitHub API" >&2
  else
    echo "Release downloads: anonymous public URLs" >&2
  fi
}

# curl with the token and bounded retries of transient failures.
github_release_curl() {
  curl --proto '=https' --tlsv1.2 -fsSL --retry 5 --retry-delay 5 --retry-connrefused \
    --config <(printf 'header = "Authorization: Bearer %s"\n' "${GITHUB_RELEASE_TOKEN}") "$@"
}

# Writes the JSON at REST `path` (below repos/) to `out`.
github_release_json() {
  local path="$1" out="$2"
  github_release_curl --max-time 60 -H 'Accept: application/vnd.github+json' -o "${out}" "${GITHUB_RELEASE_API}/${path}"
}

# Downloads asset `name` of the release described by `json` in `repository` to `out`.
github_release_asset() {
  local repository="$1" json="$2" name="$3" out="$4" max_time="$5" id
  id="$(python3 -I -c 'import json, sys
print(next(asset["id"] for asset in json.load(open(sys.argv[1]))["assets"] if asset["name"] == sys.argv[2]))' "${json}" "${name}")" || return 1
  github_release_curl --max-time "${max_time}" -H 'Accept: application/octet-stream' -o "${out}" \
    "${GITHUB_RELEASE_API}/${repository}/releases/assets/${id}"
}
