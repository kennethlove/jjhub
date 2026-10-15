# story: e02s01
FROM rust:1.89-bookworm AS builder
WORKDIR /app
COPY Cargo.toml ./
COPY crates ./crates
RUN cargo build --release --bin server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/server /usr/local/bin/server
ENV JJHUB_STORAGE_ROOT=/var/lib/jjhub/repos
VOLUME /var/lib/jjhub
EXPOSE 3000
CMD ["server"]
