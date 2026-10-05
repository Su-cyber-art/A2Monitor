ARG NODE_IMAGE=node:24-slim
ARG RUST_IMAGE=rust:1.98-slim
ARG RUNTIME_IMAGE=debian:trixie-slim

FROM ${NODE_IMAGE} AS web
WORKDIR /source/web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM ${NODE_IMAGE} AS emerald
RUN npm install --global bun@1.3.14
WORKDIR /source/themes/emerald
COPY themes/emerald/package.json themes/emerald/bun.lock ./
RUN bun install --frozen-lockfile
COPY themes/emerald/ ./
RUN bun run build

FROM ${RUST_IMAGE} AS rust
ARG CARGO_BUILD_JOBS=2
RUN apt-get update && apt-get install -y --no-install-recommends build-essential pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY scripts/ scripts/
RUN cargo build --locked --release --workspace -j ${CARGO_BUILD_JOBS}

FROM ${RUNTIME_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tzdata && rm -rf /var/lib/apt/lists/* && groupadd -g 10001 monitor && useradd -u 10001 -g monitor -M -s /usr/sbin/nologin monitor && mkdir /data && chown monitor:monitor /data
WORKDIR /app
COPY --from=rust /source/target/release/monitor-hub /source/target/release/monitor-agent ./
COPY --from=web /source/web/dist ./web/dist
COPY --from=emerald /source/themes/emerald/dist ./web/themes/emerald/dist
COPY themes/emerald/komari-theme.json themes/emerald/LICENSE ./web/themes/emerald/
USER monitor
ENV MONITOR_DATA=/data MONITOR_FRONTEND=/app/web/dist MONITOR_LISTEN=0.0.0.0:28080 RUST_LOG=info TZ=Asia/Taipei
VOLUME /data
EXPOSE 28080
STOPSIGNAL SIGINT
CMD ["/app/monitor-hub"]
