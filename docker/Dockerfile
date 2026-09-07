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

FROM alpine:3.24 AS raw-decoder-builder
RUN apk add --no-cache build-base cmake ninja git curl unzip autoconf automake libtool pkgconf jpeg-dev libjxl-dev zlib-dev lcms2-dev expat-dev
COPY docker/build_raw_decoder.sh docker/libraw-dng-host.patch docker/dng-sdk-portability.patch /decoder/
RUN sh /decoder/build_raw_decoder.sh /build/raw-decoder /opt/raw-decoder system
RUN --mount=type=bind,source=tests/docker/build_raw_decoder.sh,target=/decoder-test.sh \
    sh /decoder-test.sh /opt/raw-decoder/bin/dcraw_emu /build/raw-decoder/dng_sdk_1_7_1/sample_files /build/decoder-check

FROM alpine:3.24 AS runtime-base

ARG SOURCE_REPOSITORY=https://github.com/yzard/momento

LABEL org.opencontainers.image.source="${SOURCE_REPOSITORY}" \
      org.opencontainers.image.description="Momento self-hosted photo management service"

WORKDIR /app

RUN apk add --no-cache \
    ffmpeg \
    imagemagick \
    imagemagick-heic \
    imagemagick-raw \
    imagemagick-tiff \
    imagemagick-webp \
    libjxl \
    exiftool \
    su-exec \
    tini \
    tzdata

COPY docker/imagemagick-policy.xml /etc/ImageMagick-7/policy.xml
COPY --from=raw-decoder-builder /opt/raw-decoder/lib/libraw_r.so* /usr/lib/
COPY --from=raw-decoder-builder /opt/raw-decoder/share/licenses/ /usr/share/licenses/momento-raw-decoder/
RUN --mount=from=raw-decoder-builder,source=/build/raw-decoder/dng_sdk_1_7_1/sample_files,target=/fixtures \
    --mount=type=bind,source=tests/docker/libraw-dng-host.sh,target=/decoder-test.sh \
    sh /decoder-test.sh /fixtures /data/decoder-check && rm -rf /data/decoder-check

RUN for format in AVIF DNG GIF HEIC QOI TIFF WEBP; do \
        magick -list format | awk -v required_format="$format" '($1 == required_format || $1 == required_format"*") && $3 ~ /r/ { found = 1 } END { exit !found }'; \
    done

FROM runtime-base

COPY --from=backend-builder /app/dist/backend/momento-api /app/momento-api
COPY --from=frontend-builder /app/dist/frontend ./static
COPY dist/android/*.apk /app/android/
RUN set -- /app/android/*.apk; \
    [ "$#" -eq 1 ]; \
    cp "$1" /app/static/momento-android.apk; \
    rm -rf /app/android

COPY docker/entrypoint.sh /entrypoint.sh
COPY docker/entrypoint_common.sh /entrypoint_common.sh
RUN chmod +x /entrypoint.sh

RUN mkdir -p /data

ENV PUID=1000 \
    PGID=1000 \
    UMASK=022 \
    TZ=UTC \
    RUST_BACKTRACE=full \
    TMPDIR=/data/tmp \
    TEMP=/data/tmp \
    TMP=/data/tmp

EXPOSE 8000

# The server reads no environment variables: server.data_dir (/data) and
# server.static_dir (/app/static) come from the config file the entrypoint generates.
ENTRYPOINT ["/sbin/tini", "--", "/entrypoint.sh"]
CMD ["/app/momento-api", "-c", "/data/config.toml"]
