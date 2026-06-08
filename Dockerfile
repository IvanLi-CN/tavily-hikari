ARG XRAY_CORE_VERSION=26.2.6

########## Stage 1: compile the Rust binary ##########
FROM rust:1.91-bookworm AS builder
ARG APP_EFFECTIVE_VERSION
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock build.rs ./
# Prepare a temporary stub target so `cargo fetch` doesn't fail on CI builders
# that require at least one target in the manifest resolution phase.
RUN mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo fetch

COPY src ./src
ENV APP_EFFECTIVE_VERSION=${APP_EFFECTIVE_VERSION}
RUN cargo build --release --locked \
    --bin tavily-hikari \
    --bin billing_ledger_audit \
    --bin monthly_quota_rebase \
    --bin mcp_search_billing_repair \
    --bin mcp_request_log_retry_repair \
    --bin db_compaction_once \
    --bin request_logs_gc_once

########## Stage 2: import the official Xray runtime ##########
FROM ghcr.io/xtls/xray-core:${XRAY_CORE_VERSION} AS xray-downloader

FROM debian:bookworm-slim AS runtime
ARG APP_EFFECTIVE_VERSION

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /srv/app

COPY --from=builder /app/target/release/tavily-hikari /usr/local/bin/tavily-hikari
COPY --from=builder /app/target/release/billing_ledger_audit /usr/local/bin/billing_ledger_audit
COPY --from=builder /app/target/release/monthly_quota_rebase /usr/local/bin/monthly_quota_rebase
COPY --from=builder /app/target/release/mcp_search_billing_repair /usr/local/bin/mcp_search_billing_repair
COPY --from=builder /app/target/release/mcp_request_log_retry_repair /usr/local/bin/mcp_request_log_retry_repair
COPY --from=builder /app/target/release/db_compaction_once /usr/local/bin/db_compaction_once
COPY --from=builder /app/target/release/request_logs_gc_once /usr/local/bin/request_logs_gc_once
COPY --from=xray-downloader /usr/local/bin/xray /usr/local/bin/xray
COPY --from=xray-downloader /usr/local/share/xray /usr/local/share/xray
# Copy prebuilt web assets (produced by CI before Docker build)
COPY web/dist /srv/app/web

ENV PROXY_DB_PATH=/srv/app/data/tavily_proxy.db \
    PROXY_BIND=0.0.0.0 \
    PROXY_PORT=8787 \
    WEB_STATIC_DIR=/srv/app/web \
    XRAY_RUNTIME_DIR=/srv/app/data/xray-runtime \
    APP_EFFECTIVE_VERSION=${APP_EFFECTIVE_VERSION}

LABEL org.opencontainers.image.version=${APP_EFFECTIVE_VERSION}

VOLUME ["/srv/app/data"]
EXPOSE 8787

HEALTHCHECK --interval=10s --timeout=5s --start-period=60s --retries=18 CMD curl --fail --silent http://127.0.0.1:8787/health || exit 1

ENTRYPOINT ["tavily-hikari"]
CMD []
