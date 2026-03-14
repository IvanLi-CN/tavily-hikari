########## Stage 1: compile the Rust binary ##########
FROM rust:1.91-bookworm AS builder
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
FROM debian:bookworm-slim AS xray-downloader
ARG XRAY_CORE_VERSION=26.2.6
ARG TARGETARCH

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl unzip \
    && rm -rf /var/lib/apt/lists/* \
    && ARCH="${TARGETARCH:-$(dpkg --print-architecture)}" \
    && case "${ARCH}" in \
        amd64) XRAY_ZIP="Xray-linux-64.zip" ;; \
        arm64) XRAY_ZIP="Xray-linux-arm64-v8a.zip" ;; \
        *) echo "Unsupported TARGETARCH=${TARGETARCH} resolved_arch=${ARCH} for Xray-core" >&2; exit 1 ;; \
      esac \
    && curl -fsSL -o /tmp/xray.zip "https://github.com/XTLS/Xray-core/releases/download/v${XRAY_CORE_VERSION}/${XRAY_ZIP}" \
    && unzip -q /tmp/xray.zip -d /tmp/xray \
    && install -m 0755 /tmp/xray/xray /usr/local/bin/xray \
    && install -d /usr/local/share/licenses/xray-core \
    && install -m 0644 /tmp/xray/LICENSE /usr/local/share/licenses/xray-core/LICENSE \
    && rm -rf /tmp/xray /tmp/xray.zip

FROM debian:bookworm-slim AS runtime
ARG APP_EFFECTIVE_VERSION

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /srv/app

COPY --from=builder /app/target/release/tavily-hikari /usr/local/bin/tavily-hikari
COPY --from=xray-downloader /usr/local/bin/xray /usr/local/bin/xray
COPY --from=xray-downloader /usr/local/share/licenses/xray-core/LICENSE /usr/local/share/licenses/xray-core/LICENSE
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

HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=6 CMD curl --fail --silent http://127.0.0.1:8787/health || exit 1

ENTRYPOINT ["tavily-hikari"]
CMD []
