# Multi-stage build for the headless douyin-recorder server.
#
# The server crate depends only on `core`; Tauri/GTK/WebKit are not involved in
# any build or runtime stage.
#
#   docker build -t douyin-recorder-server .
#   docker run -d --name douyin-recorder \
#     -p 8787:8787 \
#     -e DOUYIN_RECORDER_TOKEN=change-me \
#     -v /path/on/host:/data \
#     douyin-recorder-server
#
# The web UI is served at http://<host>:8787 (append ?token=<token> on first
# visit when a token is configured).

# ── Stage 1: build the web frontend ──────────────────────────────────────────
FROM node:22-bookworm-slim AS web
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY tsconfig.json tsconfig.node.json vite.config.ts index.html ./
COPY src ./src
RUN pnpm build

# ── Stage 2: build the headless server binary ────────────────────────────────
FROM rust:1-slim-bookworm AS rust
# The headless build has no Tauri dependency, so no GTK/WebKit dev packages.
RUN apt-get update && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# Workspace manifests first so dependency compilation is cached. The workspace
# also lists the desktop crate; a stub manifest keeps server-only images slim.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src-tauri/src && printf '[package]\nname = "douyin-recorder"\nversion = "0.2.1"\nedition = "2021"\n' > src-tauri/Cargo.toml \
    && printf 'fn main() {}\n' > src-tauri/src/main.rs
COPY core/Cargo.toml core/build.rs ./core/
COPY core/proto ./core/proto
COPY server/Cargo.toml server/build.rs ./server/

# The server build embeds the frontend from ../dist.
COPY --from=web /app/dist /app/dist
RUN mkdir -p core/src server/src \
    && echo "" > core/src/lib.rs && echo "" > server/src/main.rs \
    && cargo build --release -p douyin-recorder-server || true
COPY core/src ./core/src
COPY server/src ./server/src
RUN touch core/src/lib.rs server/src/main.rs \
    && cargo build --release -p douyin-recorder-server

# ── Stage 3: runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim
# The headless binary only needs ffmpeg (recording engine) and CA roots.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ffmpeg ca-certificates tzdata curl \
      fonts-wqy-zenhei fontconfig \
    && rm -rf /var/lib/apt/lists/*
# Danmaku subtitles use the desktop font name; map it to the bundled CJK font
# so burning renders Chinese correctly on Linux.
RUN printf '%s\n' \
      '<?xml version="1.0"?>' \
      '<!DOCTYPE fontconfig SYSTEM "fonts.dtd">' \
      '<fontconfig>' \
      '  <alias><family>Microsoft YaHei</family><prefer><family>WenQuanYi Zen Hei</family></prefer></alias>' \
      '</fontconfig>' \
      > /etc/fonts/conf.d/99-danmaku-fallback.conf

# FFmpeg sidecar resolution falls back to PATH, which apt satisfies.
COPY --from=rust /app/target/release/douyin-recorder-server /usr/local/bin/douyin-recorder-server

# Run as an unprivileged user so files in the bind-mounted /data directory are
# owned by the host user (compose passes PUID/PGID, defaulting to 1000).
RUN groupadd -g 1000 app && useradd -u 1000 -g 1000 -M -s /usr/sbin/nologin app \
    && mkdir -p /data && chown -R 1000:1000 /data

ENV DOUYIN_RECORDER_HOME=/data \
    DOUYIN_RECORDER_BIND=0.0.0.0:8787 \
    TZ=Asia/Shanghai \
    XDG_CACHE_HOME=/tmp/.cache

VOLUME ["/data"]
EXPOSE 8787
USER app

ENTRYPOINT ["/usr/local/bin/douyin-recorder-server"]
