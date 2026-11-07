# Optional prebuilt environment for coding agents (#269, docs/adr/0005); required CI does not use it.
# The tools are whatever mise.toml lists (latest stable), installed by mise as the image's global tool set;
# rustup installs the toolchain rust-toolchain.toml names (channel, components, wasm32 target). The
# container's postCreateCommand runs `mise run setup` for the project's Python virtualenv, Playwright and the
# asset bundles.
FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac

# libudev-dev: the native client's gamepad backend (gilrs); libasound2-dev: its audio (kira on cpal). The rest: builds, fetch scripts, git.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential bzip2 ca-certificates curl git git-lfs libasound2-dev libudev-dev pkg-config python3 sudo unzip xz-utils \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --shell /bin/bash --uid 1001 agent \
 && echo 'agent ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/agent

USER agent
ENV CARGO_HOME=/home/agent/.cargo RUSTUP_HOME=/home/agent/.rustup \
    PATH=/home/agent/.local/share/mise/shims:/home/agent/.local/bin:/home/agent/.cargo/bin:$PATH
WORKDIR /home/agent

# mise.toml's [tools] (everything before its [env]) become the global set; its [env] and [tasks] apply
# inside the checkout.
COPY --chown=agent mise.toml /tmp/mise/
COPY --chown=agent rust-toolchain.toml /tmp/toolchain/
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none \
 && curl -fsSL https://mise.run | sh \
 && mkdir -p /home/agent/.config/mise \
 && sed '/^\[env\]/,$d' /tmp/mise/mise.toml > /home/agent/.config/mise/config.toml \
 && mise trust --yes /home/agent/.config/mise/config.toml \
 && mise install --yes \
 && cd /tmp/toolchain && rustup toolchain install && rustup default "$(rustup show active-toolchain | cut -d' ' -f1)" \
 && rm -rf /tmp/mise /tmp/toolchain "$RUSTUP_HOME/downloads" "$RUSTUP_HOME/tmp"
# Chromium's system libraries (Playwright runs apt through sudo), so `mise run setup` only fetches the browser.
RUN uvx --from playwright playwright install-deps chromium
