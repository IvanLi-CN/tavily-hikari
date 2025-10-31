# Stage 1: compile the Rust binary
FROM rust:1.91 AS builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
RUN cargo fetch

COPY src ./src
RUN cargo build --release --locked

# Stage 2: create a slim runtime image
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /srv/app

COPY --from=builder /app/target/release/tavily-hikari /usr/local/bin/tavily-hikari

ENV PROXY_DB_PATH=/srv/app/data/tavily_proxy.db \
    PROXY_BIND=0.0.0.0 \
    PROXY_PORT=8787

VOLUME ["/srv/app/data"]
EXPOSE 8787

ENTRYPOINT ["tavily-hikari"]
CMD []
