FROM node:20-alpine AS frontend-builder

WORKDIR /app/build/frontend/workspace

RUN corepack enable && corepack prepare pnpm@9.15.0 --activate

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY src/frontend ./src/frontend
COPY src/backend/version.txt ./src/backend/version.txt

RUN pnpm install --frozen-lockfile && pnpm --dir /app/build/frontend/workspace/src/frontend build && mkdir -p /app/dist/frontend && cp -R /app/build/frontend/workspace/src/frontend/dist/. /app/dist/frontend/

FROM rust:1-alpine AS backend-builder

WORKDIR /app

RUN apk add --no-cache musl-dev

COPY src/backend ./src/backend
COPY src/common ./src/common

RUN CARGO_TARGET_DIR=/app/build/backend/target cargo build --release --manifest-path /app/src/backend/Cargo.toml && mkdir -p /app/dist/backend && cp /app/build/backend/target/release/momento-api /app/dist/backend/momento-api

FROM alpine:latest

ARG SOURCE_REPOSITORY=https://github.com/yzard/momento

LABEL org.opencontainers.image.source="${SOURCE_REPOSITORY}" \
      org.opencontainers.image.description="Momento self-hosted photo management service"

WORKDIR /app

RUN apk add --no-cache \
    ffmpeg \
    imagemagick \
    exiftool \
    su-exec \
    tzdata \
    libheif

COPY --from=backend-builder /app/dist/backend/momento-api /app/momento-api
COPY --from=frontend-builder /app/dist/frontend ./static

COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

RUN mkdir -p /data

ENV PUID=1000 \
    PGID=1000 \
    UMASK=022 \
    TZ=UTC \
    RUST_BACKTRACE=full

EXPOSE 8000

# The server reads no environment variables: server.data_dir (/data) and
# server.static_dir (/app/static) come from the config file the entrypoint generates.
ENTRYPOINT ["/entrypoint.sh"]
CMD ["/app/momento-api", "-c", "/data/config.toml"]
