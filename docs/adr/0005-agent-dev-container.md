# Optional Dev Container for coding agents; CI stays on Ubuntu runners

Status: accepted; resolves #269 (split from #148). CI stays on conventional GitHub-hosted Ubuntu runners and Nix is not pursued (owner, 2026-10-18).

## Decision

`.devcontainer/` provides a prebuilt environment for fresh, ephemeral coding agents (cloud agent sandboxes, devcontainer-aware runners). It is optional. Required CI does not use it, and the image is not published to a registry.

The image is `ubuntu:24.04` pinned by digest plus the tools CI installs: Rust from `rust-toolchain.toml` (stable, clippy, rustfmt, `wasm32-unknown-unknown`), `libudev-dev`, cargo-nextest (latest, as CI), the `wasm-bindgen` CLI at the crate's exact pin and Bun at `web/package.json`'s `packageManager`. The Dockerfile reads those versions from their files at build time, so it adds no second version authority. Asset bundles are per-checkout inputs: `postCreateCommand` runs `scripts/fetch-bundles.sh`. Playwright/Chromium, GPU and native-window workflows are not in the image; they stay opt-in on the host or in the Pages job.

## Measurements (2026-10-24, revision 4e08253)

Local, cold (no Docker layer, Cargo registry or target cache), 1 run each:

| Step | Time |
| --- | --- |
| Provision from `ubuntu:24.04` (apt + rustup + nextest + wasm-bindgen + Bun) = `docker build --no-cache` | 80 s |
| Pull a comparable public image instead (`rust:1-bookworm`, 2.2 GB) | 25 s |
| Prebuilt image size (uncompressed / gzip) | 1.9 GB / 0.49 GB |
| In the image: `fetch-bundles.sh` + `prepare-ranger-native.sh` | 19 s |
| In the image: first useful test (`cargo nextest run -p mmorpg-protocol`, cold compile) | 115 s |
| In the image: then `cargo check --workspace --locked` | 49 s |
| In the image: `bun install --frozen-lockfile` | 2 s |

GitHub-hosted `rust / validate` job, last 10 Rust-changing runs: Cargo cache restore 11–37 s, `Install system packages` (only `libudev-dev`) **16–247 s** (half of the runs above 200 s), pre-validation (bundles, Ranger native prep, nextest) 14–22 s, Test 21–959 s.

## Reading

- Cold agent start is dominated by Cargo compilation, which a toolchain image does not remove. A prebuilt image saves about a minute of provisioning per fresh agent, and only when it is pulled rather than built; building it locally costs the same as provisioning.
- Reused workspaces (the owner's machine, persistent agent checkouts) already have the toolchain and warm targets; the container gives them nothing.
- In CI the one large, variable environment cost is `apt-get` for `libudev-dev` (up to ~4 min), not the toolchain. A job container would hide it but adds an image publish pipeline, registry storage and a pull on every job, and does not remove the Cargo cache restore. Fixing the apt step in the reusable workflow (for example skipping a full `apt-get update` or caching the package) is the cheaper remedy and stays with `reusable-workflows`.
- Not measured: registry pull of this exact image on GitHub runners (no image is published), repeated-task success/retry rates, runner-minutes across many agent tasks.

Required CI is therefore not switched to containers. Revisit only if ephemeral-agent startup becomes a bottleneck; then publish the image with a digest and measure its pull on a runner.
