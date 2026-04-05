FROM node:20-alpine AS frontend-builder

WORKDIR /app

RUN corepack enable && corepack prepare pnpm@9.15.0 --activate

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY src/web/package.json ./src/web/
COPY packages/shared/package.json ./packages/shared/

RUN pnpm install --frozen-lockfile

COPY src/web ./src/web
COPY packages/shared ./packages/shared

WORKDIR /app/src/web
RUN pnpm build

FROM rust:1-alpine AS backend-builder

WORKDIR /app

RUN apk add --no-cache musl-dev

COPY src/api/Cargo.toml src/api/Cargo.lock ./src/api/
COPY src/api ./src/api

WORKDIR /app/src/api
RUN cargo build --release

FROM alpine:latest

WORKDIR /app

RUN apk add --no-cache \
    ffmpeg \
    imagemagick \
    exiftool \
    su-exec \
    tzdata \
    libheif

COPY --from=backend-builder /app/src/api/target/release/momento-api /app/momento-api

COPY --from=frontend-builder /app/src/web/dist ./static

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
