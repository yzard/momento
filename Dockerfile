FROM node:20-alpine AS frontend-builder

WORKDIR /app/build/web/workspace

RUN corepack enable && corepack prepare pnpm@9.15.0 --activate

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY src/web ./src/web

RUN pnpm install --frozen-lockfile && pnpm --dir /app/build/web/workspace/src/web build && mkdir -p /app/dist/web && cp -R /app/build/web/workspace/src/web/dist/. /app/dist/web/

FROM rust:1-alpine AS backend-builder

WORKDIR /app

RUN apk add --no-cache musl-dev

COPY src/api ./src/api

RUN CARGO_TARGET_DIR=/app/build/api/target cargo build --release --manifest-path /app/src/api/Cargo.toml && mkdir -p /app/dist/api && cp /app/build/api/target/release/momento-api /app/dist/api/momento-api

FROM alpine:latest

WORKDIR /app

RUN apk add --no-cache \
    ffmpeg \
    imagemagick \
    exiftool \
    su-exec \
    tzdata \
    libheif

COPY --from=backend-builder /app/dist/api/momento-api /app/momento-api
COPY --from=frontend-builder /app/dist/web ./static

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

RUN mkdir -p /data

ENV PUID=1000 \
    PGID=1000 \
    UMASK=022 \
    TZ=UTC \
    RUST_BACKTRACE=full \
    MOMENTO_DATA_DIR=/data \
    MOMENTO_STATIC_DIR=/app/static

EXPOSE 8000

ENTRYPOINT ["/entrypoint.sh"]
CMD ["/app/momento-api"]
