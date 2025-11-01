########## Stage 1: compile the Rust binary ##########
FROM rust:1.91 AS builder
ARG APP_EFFECTIVE_VERSION
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
# Prepare a temporary stub target so `cargo fetch` doesn't fail on CI builders
# that require at least one target in the manifest resolution phase.
RUN mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo fetch

COPY src ./src
ENV APP_EFFECTIVE_VERSION=${APP_EFFECTIVE_VERSION}
RUN cargo build --release --locked

########## Stage 2: create a slim runtime image ##########
FROM debian:bookworm-slim AS runtime
ARG APP_EFFECTIVE_VERSION

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /srv/app

COPY --from=builder /app/target/release/tavily-hikari /usr/local/bin/tavily-hikari
# Copy prebuilt web assets (produced by CI before Docker build)
COPY web/dist /srv/app/web

ENV PROXY_DB_PATH=/srv/app/data/tavily_proxy.db \
    PROXY_BIND=0.0.0.0 \
    PROXY_PORT=8787 \
    WEB_STATIC_DIR=/srv/app/web \
    APP_EFFECTIVE_VERSION=${APP_EFFECTIVE_VERSION}

LABEL org.opencontainers.image.version=${APP_EFFECTIVE_VERSION}

VOLUME ["/srv/app/data"]
EXPOSE 8787

ENTRYPOINT ["tavily-hikari"]
CMD []
