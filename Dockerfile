########## Stage 1: compile the Rust binary ##########
FROM rust:1.91-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33 AS builder
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

FROM builder AS maintenance-builder
RUN cargo build --release --locked \
    --bin billing_ledger_audit \
    --bin monthly_quota_rebase \
    --bin mcp_search_billing_repair \
    --bin mcp_request_log_retry_repair \
    --bin observability_sidecar_migrate \
    --bin observability_lock_holder \
    --bin db_compaction_once \
    --bin request_logs_gc_once \
    --bin ha_outbox_cleanup_once \
    --bin ha_trigger_repair_once

FROM maintenance-builder AS app-builder
ARG APP_EFFECTIVE_VERSION
RUN test -n "${APP_EFFECTIVE_VERSION}" \
    && APP_EFFECTIVE_VERSION="${APP_EFFECTIVE_VERSION}" cargo build --release --locked --bin tavily-hikari

########## Stage 1b: audit the allowlisted build context ##########
FROM builder AS context-audit
COPY . /context
RUN test ! -e /context/.env \
    && test ! -e /context/.env.local \
    && test -z "$(find /context -type d -name node_modules -print -quit)" \
    && test -z "$(find /context -type f \( -name '*.db' -o -name '*.db-*' \) -print -quit)" \
    && test -z "$(find /context -mindepth 1 -print | sed 's#^/context/##' | while IFS= read -r path; do \
      case "${path}" in \
        Cargo.toml|Cargo.lock|build.rs|rust-toolchain.toml|src|src/*|scripts|scripts/docker-entrypoint.sh|scripts/docker-healthcheck.sh|web|web/dist|web/dist/*) ;; \
        *) printf '%s\n' "${path}" ;; \
      esac; \
    done)"

########## Stage 2: import the official Xray runtime ##########
FROM ghcr.io/xtls/xray-core:26.2.6@sha256:c6daec5244a2110490ec2049d4c6588cbef544a8bcb4b32c5e4da16e15b7f98e AS xray-downloader

########## Stage 3: normalize every copied payload before final image COPY ##########
FROM builder AS payload-normalizer
ARG SOURCE_DATE_EPOCH=0

RUN mkdir -p \
    /payload/bin \
    /payload/runtime-scripts \
    /payload/web/app \
    /payload/web/app/pwa \
    /payload/web/meta \
    /payload/web/pwa \
    /payload/xray/bin \
    /payload/xray/share

COPY --from=xray-downloader /usr/local/bin/xray /payload/xray/bin/xray
COPY --from=xray-downloader /usr/local/share/xray/ /payload/xray/share/

COPY --from=maintenance-builder /app/target/release/billing_ledger_audit /payload/bin/billing_ledger_audit
COPY --from=maintenance-builder /app/target/release/monthly_quota_rebase /payload/bin/monthly_quota_rebase
COPY --from=maintenance-builder /app/target/release/mcp_search_billing_repair /payload/bin/mcp_search_billing_repair
COPY --from=maintenance-builder /app/target/release/mcp_request_log_retry_repair /payload/bin/mcp_request_log_retry_repair
COPY --from=maintenance-builder /app/target/release/observability_sidecar_migrate /payload/bin/observability_sidecar_migrate
COPY --from=maintenance-builder /app/target/release/observability_lock_holder /payload/bin/observability_lock_holder
COPY --from=maintenance-builder /app/target/release/db_compaction_once /payload/bin/db_compaction_once
COPY --from=maintenance-builder /app/target/release/request_logs_gc_once /payload/bin/request_logs_gc_once
COPY --from=maintenance-builder /app/target/release/ha_outbox_cleanup_once /payload/bin/ha_outbox_cleanup_once
COPY --from=maintenance-builder /app/target/release/ha_trigger_repair_once /payload/bin/ha_trigger_repair_once
COPY --from=app-builder /app/target/release/tavily-hikari /payload/bin/tavily-hikari

COPY scripts/docker-entrypoint.sh /payload/runtime-scripts/docker-entrypoint.sh
COPY scripts/docker-healthcheck.sh /payload/runtime-scripts/docker-healthcheck.sh

COPY web/dist/assets/ /payload/web/app/assets/
COPY web/dist/index.html web/dist/admin.html web/dist/console.html web/dist/login.html web/dist/registration-paused.html web/dist/sw-public.js web/dist/sw-admin.js /payload/web/app/
COPY web/dist/pwa/ /payload/web/pwa/
COPY web/dist/favicon.svg web/dist/manifest.webmanifest web/dist/manifest-admin.webmanifest /payload/web/meta/
RUN mv /payload/web/pwa/asset-graphs.json /payload/web/app/pwa/asset-graphs.json

RUN set -eux; \
    find /payload -type d -exec chown 0:0 {} +; \
    find /payload -type f -exec chown 0:0 {} +; \
    find /payload -type d -exec chmod 0755 {} +; \
    find /payload -type f -exec chmod 0644 {} +; \
    chmod 0755 \
      /payload/bin/tavily-hikari \
      /payload/bin/billing_ledger_audit \
      /payload/bin/monthly_quota_rebase \
      /payload/bin/mcp_search_billing_repair \
      /payload/bin/mcp_request_log_retry_repair \
      /payload/bin/observability_sidecar_migrate \
      /payload/bin/observability_lock_holder \
      /payload/bin/db_compaction_once \
      /payload/bin/request_logs_gc_once \
      /payload/bin/ha_outbox_cleanup_once \
      /payload/bin/ha_trigger_repair_once \
      /payload/runtime-scripts/docker-entrypoint.sh \
      /payload/runtime-scripts/docker-healthcheck.sh \
      /payload/xray/bin/xray; \
    find /payload -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

########## Stage 4: final runtime image ##########
FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /srv/app

COPY --from=payload-normalizer /payload/xray/bin/xray /usr/local/bin/xray
COPY --from=payload-normalizer /payload/xray/share/ /usr/local/share/xray/

COPY --from=payload-normalizer /payload/bin/tavily-hikari /usr/local/bin/tavily-hikari
COPY --from=payload-normalizer /payload/bin/billing_ledger_audit /usr/local/bin/billing_ledger_audit
COPY --from=payload-normalizer /payload/bin/monthly_quota_rebase /usr/local/bin/monthly_quota_rebase
COPY --from=payload-normalizer /payload/bin/mcp_search_billing_repair /usr/local/bin/mcp_search_billing_repair
COPY --from=payload-normalizer /payload/bin/mcp_request_log_retry_repair /usr/local/bin/mcp_request_log_retry_repair
COPY --from=payload-normalizer /payload/bin/observability_sidecar_migrate /usr/local/bin/observability_sidecar_migrate
COPY --from=payload-normalizer /payload/bin/observability_lock_holder /usr/local/bin/observability_lock_holder
COPY --from=payload-normalizer /payload/bin/db_compaction_once /usr/local/bin/db_compaction_once
COPY --from=payload-normalizer /payload/bin/request_logs_gc_once /usr/local/bin/request_logs_gc_once
COPY --from=payload-normalizer /payload/bin/ha_outbox_cleanup_once /usr/local/bin/ha_outbox_cleanup_once
COPY --from=payload-normalizer /payload/bin/ha_trigger_repair_once /usr/local/bin/ha_trigger_repair_once
COPY --from=payload-normalizer /payload/runtime-scripts/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
COPY --from=payload-normalizer /payload/runtime-scripts/docker-healthcheck.sh /usr/local/bin/docker-healthcheck.sh

# The application package layer contains the versioned JavaScript bundle and shells.
COPY --from=payload-normalizer /payload/web/app/ /srv/app/web/
COPY --from=payload-normalizer /payload/web/pwa/ /srv/app/web/pwa/
COPY --from=payload-normalizer /payload/web/meta/favicon.svg /payload/web/meta/manifest.webmanifest /payload/web/meta/manifest-admin.webmanifest /srv/app/web/

VOLUME ["/srv/app/data"]
EXPOSE 8787

HEALTHCHECK --interval=5s --timeout=5s --start-period=20s --retries=18 CMD ["/usr/local/bin/docker-healthcheck.sh"]

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD []

ENV PROXY_DB_PATH=/srv/app/data/tavily_proxy.db \
    PROXY_BIND=0.0.0.0 \
    PROXY_PORT=8787 \
    WEB_STATIC_DIR=/srv/app/web \
    XRAY_RUNTIME_DIR=/srv/app/data/xray-runtime

ARG APP_EFFECTIVE_VERSION
LABEL org.opencontainers.image.version=${APP_EFFECTIVE_VERSION}
