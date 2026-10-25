# Optional prebuilt environment for coding agents (#269, docs/adr/0005).
# Mirrors the tools GitHub-hosted Ubuntu CI provisions; required CI does not use it.
# Versions come from the existing authorities, read at build time:
# rust-toolchain.toml (Rust channel, components, wasm32 target),
# crates/mmorpg-wasm/Cargo.toml (exact wasm-bindgen pin) and web/package.json
# packageManager (Bun). cargo-nextest is latest, as in CI.
FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac

# libudev-dev: the native client's gamepad backend (gilrs); libasound2-dev: its audio (kira on cpal). The rest: builds, fetch scripts, git.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential bzip2 ca-certificates curl git git-lfs libasound2-dev libudev-dev pkg-config python3 sudo unzip xz-utils \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --shell /bin/bash --uid 1001 agent \
 && echo 'agent ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/agent

USER agent
ENV CARGO_HOME=/home/agent/.cargo RUSTUP_HOME=/home/agent/.rustup PATH=/home/agent/.cargo/bin:/home/agent/.bun/bin:$PATH
WORKDIR /home/agent

COPY --chown=agent rust-toolchain.toml crates/mmorpg-wasm/Cargo.toml web/package.json /tmp/toolchain/
RUN WASM_BINDGEN_VERSION="$(sed -n 's/^wasm-bindgen = "=\(.*\)"$/\1/p' /tmp/toolchain/Cargo.toml)" \
 && BUN_VERSION="$(sed -n 's/.*"packageManager": "bun@\(.*\)".*/\1/p' /tmp/toolchain/package.json)" \
 && test -n "$WASM_BINDGEN_VERSION" && test -n "$BUN_VERSION" \
 && curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none \
 && cd /tmp/toolchain && rustup toolchain install && rustup default "$(rustup show active-toolchain | cut -d' ' -f1)" \
 && curl -fsSL https://get.nexte.st/latest/linux | tar zxf - -C "$CARGO_HOME/bin" \
 && curl -fsSL "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${WASM_BINDGEN_VERSION}/wasm-bindgen-${WASM_BINDGEN_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
    | tar zxf - --strip-components=1 -C "$CARGO_HOME/bin" "wasm-bindgen-${WASM_BINDGEN_VERSION}-x86_64-unknown-linux-musl/wasm-bindgen" \
 && curl -fsSL https://bun.sh/install | bash -s "bun-v${BUN_VERSION}" \
 && rm -rf /tmp/toolchain "$RUSTUP_HOME/downloads" "$RUSTUP_HOME/tmp"
