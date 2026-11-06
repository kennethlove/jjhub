# syntax=docker/dockerfile:1
# story: e02s01
FROM rust:1.99-bookworm AS builder
WORKDIR /app
# WHY: lockfile pins the dep layer so source edits do not float versions
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# WHY: cache mounts — registry downloads and dep builds survive across
# builds on this builder; without them every deploy recompiles the world.
# The binary is copied out of the cached target dir before the stage ends.
RUN --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --bin server \
    && cp target/release/server /server-binary

FROM debian:bookworm-slim
# story: e18s01 — curl feeds the /healthz healthcheck below
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /server-binary /usr/local/bin/server
ENV JJHUB_STORAGE_ROOT=/var/lib/jjhub/repos
VOLUME /var/lib/jjhub
EXPOSE 3000
# story: e18s01 — honest HTTP healthcheck against the real /healthz route
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD curl -fsS http://localhost:3000/healthz
CMD ["server"]
