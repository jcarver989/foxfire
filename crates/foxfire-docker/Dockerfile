# syntax=docker/dockerfile:1.7
#
# Built by `foxfire-docker warm <project>`. The build context holds only
# `entrypoint.sh`. Two named contexts each hold one Git bundle, `repo.bundle`:
#
#   project-src  the project's upstream branch at SOURCE_SHA
#   foxfire-src  Foxfire's upstream branch at TOOLS_SHA, for foxfire-relay
#
# Neither host checkout is ever part of the build.

FROM debian:bookworm-slim AS tooling
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        bash build-essential ca-certificates curl git pkg-config \
        libfontconfig1-dev libxkbcommon-x11-dev libvulkan1 mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/*
ARG UID=1000
RUN useradd --create-home --uid "${UID}" --shell /bin/bash dev \
    && install --directory --owner dev --group dev /workspace /opt/foxfire-target
USER dev
ENV CARGO_HOME=/home/dev/.cargo \
    RUSTUP_HOME=/home/dev/.rustup \
    PATH=/home/dev/.local/bin:/home/dev/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ARG RUST_VERSION=1.100.0
RUN curl --proto '=https' --tlsv1.2 --silent --show-error --fail https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}" \
    && git config --global user.name "Foxfire agent" \
    && git config --global user.email "agent@foxfire.invalid"
ARG AETHER_VERSION=0.9.21
RUN cargo install --locked aether-agent-cli --version "${AETHER_VERSION}" --root /home/dev/.local \
    && rm -rf "${CARGO_HOME}/registry" "${CARGO_HOME}/git"

FROM tooling AS relay
ARG TOOLS_SHA
RUN --mount=type=bind,from=foxfire-src,target=/tmp/foxfire-src \
    git init --quiet /tmp/foxfire \
    && cd /tmp/foxfire \
    && git bundle unbundle /tmp/foxfire-src/repo.bundle > /dev/null \
    && git -c advice.detachedHead=false checkout --quiet "${TOOLS_SHA}" \
    && rustup toolchain install \
    && cargo build --release --locked --target-dir /tmp/relay-target -p foxfire-relay \
    && install -D --target-directory /home/dev/.local/bin /tmp/relay-target/release/foxfire-relay \
    && rm -rf /tmp/foxfire /tmp/relay-target

FROM tooling AS workspace
COPY --from=relay /home/dev/.local/bin/foxfire-relay /home/dev/.local/bin/
COPY --chmod=755 entrypoint.sh /usr/local/bin/foxfire-entrypoint
ARG SOURCE_SHA
WORKDIR /workspace
RUN --mount=type=bind,from=project-src,target=/tmp/project-src \
    git init --quiet \
    && git bundle unbundle /tmp/project-src/repo.bundle > /dev/null \
    && git -c advice.detachedHead=false checkout --quiet --detach "${SOURCE_SHA}"
ENV CARGO_TARGET_DIR=/opt/foxfire-target
RUN rustup toolchain install \
    && cargo fetch --locked \
    && cargo check --locked --workspace --all-targets --all-features
ENV FOXFIRE_RELAY_LISTEN=0.0.0.0:7777 \
    FOXFIRE_RELAY_CWD=/workspace
EXPOSE 7777
ENTRYPOINT ["foxfire-entrypoint"]
