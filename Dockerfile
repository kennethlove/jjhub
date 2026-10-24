# story: e02s01
FROM rust:1.99-bookworm AS builder
WORKDIR /app
COPY Cargo.toml ./
COPY crates ./crates
RUN cargo build --release --bin server

FROM debian:bookworm-slim
# story: e18s01 — curl feeds the /healthz healthcheck below
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/server /usr/local/bin/server
ENV JJHUB_STORAGE_ROOT=/var/lib/jjhub/repos
VOLUME /var/lib/jjhub
EXPOSE 3000
# story: e18s01 — honest HTTP healthcheck against the real /healthz route
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD curl -fsS http://localhost:3000/healthz
CMD ["server"]
