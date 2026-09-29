# syntax=docker/dockerfile:1.7
#
# The Venture server, ready to host: the server, the web client it serves,
# the content, and a world baked from `world_authoring/` (see
# `docs/building-and-running.md`, "Hosting with Docker").
#
#     docker buildx build --platform linux/amd64 -t venture-server .
#     docker run -p 8080:8080 -v venture-data:/srv/venture/server_data venture-server
#
# Build it from a checkout with its Git LFS files pulled (`git lfs pull`):
# the content's textures and models are LFS files.
#
# It builds for any platform from any platform, at full speed: the build
# stage runs on the building machine's own platform, and only the server is
# compiled for the platform the image is for. (The web client is
# WebAssembly, and the world is data, so they're the same everywhere.)

# ---- Building: the server, the web client, and the world ----
FROM --platform=$BUILDPLATFORM rust:1.98-trixie AS build

ARG TARGETARCH
ARG TRUNK_VERSION=0.21.14

# The Rust target for the image's platform, and a linker for it (the
# server is pure Rust, so a linker is all it needs from C).
RUN set -eux; \
    case "$TARGETARCH" in \
        amd64) triple=x86_64-unknown-linux-gnu; gnu=x86-64-linux-gnu; libc=amd64 ;; \
        arm64) triple=aarch64-unknown-linux-gnu; gnu=aarch64-linux-gnu; libc=arm64 ;; \
        *) echo "unsupported platform: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$triple" > /target-triple; \
    linker="$(echo "$triple" | sed 's/-unknown//')-gcc"; \
    if ! command -v "$linker" > /dev/null; then \
        apt-get update; \
        apt-get install -y --no-install-recommends "gcc-$gnu" "libc6-dev-$libc-cross"; \
        rm -rf /var/lib/apt/lists/*; \
    fi; \
    rustup target add "$triple" wasm32-unknown-unknown; \
    curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-$(uname -m)-unknown-linux-gnu.tar.gz" \
        | tar -xz -C /usr/local/bin trunk

ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

WORKDIR /venture
COPY . .

# (The caches keep Cargo's downloads and builds between image builds, so a
# change rebuilds in minutes. Everything the image needs is copied out to
# /out, since the caches aren't part of the image.)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/root/.cache/trunk \
    --mount=type=cache,target=/venture/target \
    set -eux; \
    triple="$(cat /target-triple)"; \
    cargo build --release --locked --target "$triple" -p venture_server --bin venture_server; \
    cargo build --release --locked -p venture_worldgen --bin bake; \
    mkdir -p /out/server_data/profiles; \
    cp "target/$triple/release/venture_server" /out/; \
    (cd crates/client && trunk build --release --cargo-profile wasm-release --dist /out/client); \
    target/release/bake /out/world

# ---- Running ----
FROM debian:trixie-slim

# (Nothing here runs a command in the image, so building it for another
# platform needs no emulation.)
WORKDIR /srv/venture
# The same layout as a checkout, so the repository's `server.ron` works as it is.
COPY --from=build /out/venture_server /usr/local/bin/venture_server
COPY --from=build /out/world world/
COPY --from=build /out/client crates/client/dist/
COPY content content/
COPY server.ron server.ron
# Players' profiles: the only thing the server writes. Keep them in a
# volume, so they outlive the container.
COPY --from=build --chown=10001:10001 /out/server_data server_data/
VOLUME /srv/venture/server_data

# (Not root.)
USER 10001:10001
EXPOSE 8080
# The server saves everyone's profile when it's stopped with Ctrl-C (SIGINT).
STOPSIGNAL SIGINT
CMD ["venture_server"]
