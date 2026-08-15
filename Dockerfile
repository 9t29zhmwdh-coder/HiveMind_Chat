# syntax=docker/dockerfile:1

# The web UI is built first so the Rust stage can copy the finished bundle.
FROM node:26-bookworm-slim@sha256:cd565714d4da3e84bfd341e31448f81d47c6362198f152345297c9c1154e6341 AS web
WORKDIR /build
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1-bookworm@sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97 AS server
WORKDIR /build
# Dependency manifests are copied first so a source-only change reuses the
# cached dependency build.
COPY Cargo.toml Cargo.lock ./
COPY crates/hive-core/Cargo.toml crates/hive-core/
COPY crates/hive-server/Cargo.toml crates/hive-server/
COPY crates/hive-cli/Cargo.toml crates/hive-cli/
RUN mkdir -p crates/hive-core/src crates/hive-server/src crates/hive-cli/src \
    && echo "" > crates/hive-core/src/lib.rs \
    && echo "fn main() {}" > crates/hive-server/src/main.rs \
    && echo "fn main() {}" > crates/hive-cli/src/main.rs \
    && cargo build --release --locked \
    && rm -rf crates/*/src
COPY crates/ crates/
# Cargo skips a rebuild when only mtimes look stale, so the stub artefacts are
# removed explicitly before the real build.
RUN touch crates/*/src/*.rs \
    && cargo build --release --locked --bin hivemind-server --bin hive

FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 hivemind
WORKDIR /app
COPY --from=server /build/target/release/hivemind-server /usr/local/bin/
COPY --from=server /build/target/release/hive /usr/local/bin/
COPY --from=web /build/dist /app/web
RUN mkdir -p /data && chown hivemind:hivemind /data

USER hivemind
VOLUME ["/data"]
EXPOSE 8750

ENV HIVEMIND_BIND=0.0.0.0:8750 \
    HIVEMIND_DATABASE=/data/hivemind.db \
    HIVEMIND_CONFIG=/data/hivemind.toml \
    HIVEMIND_WEB_ROOT=/app/web

HEALTHCHECK --interval=30s --timeout=4s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/hive", "--database", "/data/hivemind.db", "rooms"]

ENTRYPOINT ["/usr/local/bin/hivemind-server"]
