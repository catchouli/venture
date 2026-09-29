# syntax=docker/dockerfile:1.7
#
# The Venture server, ready to host: the server, the web client it serves,
# the content, and a world baked from `world_authoring/` (see
# `docs/building-and-running.md`, "Hosting with Docker").
#
#     docker build -t venture-server .
#     docker run -p 8080:8080 -v venture-data:/srv/venture/server_data venture-server
#
# Build it from a checkout with its Git LFS files pulled (`git lfs pull`):
# the content's textures and models are LFS files.

# ---- Building: the server, the bake tool, the web client, and the world ----
FROM rust:1.98-trixie AS build

ARG TRUNK_VERSION=0.21.14
RUN rustup target add wasm32-unknown-unknown \
    && curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-$(uname -m)-unknown-linux-gnu.tar.gz" \
    | tar -xz -C /usr/local/bin trunk

WORKDIR /venture
COPY . .

# (The caches keep Cargo's downloads and builds between image builds, so
# a change rebuilds in minutes. Everything the image needs is copied out
# to /out, since the caches aren't part of the image.)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/root/.cache/trunk \
    --mount=type=cache,target=/venture/target \
    set -eux; \
    cargo build --release --locked -p venture_server --bin venture_server; \
    cargo build --release --locked -p venture_worldgen --bin bake; \
    mkdir -p /out; \
    cp target/release/venture_server /out/; \
    (cd crates/client && trunk build --release --cargo-profile wasm-release --dist /out/client); \
    target/release/bake /out/world

# ---- Running ----
FROM debian:trixie-slim

RUN useradd --system --home-dir /srv/venture --shell /usr/sbin/nologin venture
WORKDIR /srv/venture

# The same layout as a checkout, so the repository's `server.ron` works as it is.
COPY --from=build /out/venture_server /usr/local/bin/venture_server
COPY --from=build /out/world world/
COPY --from=build /out/client crates/client/dist/
COPY content content/
COPY server.ron server.ron
RUN mkdir -p server_data/profiles && chown -R venture:venture server_data

USER venture
# Players' profiles: keep them in a volume, so they outlive the container.
VOLUME /srv/venture/server_data
EXPOSE 8080
# The server saves everyone's profile when it's stopped with Ctrl-C (SIGINT).
STOPSIGNAL SIGINT
CMD ["venture_server"]
