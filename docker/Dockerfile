FROM rust:1-alpine AS backend-builder

WORKDIR /app

RUN apk add --no-cache musl-dev

COPY src/backend ./src/backend
COPY src/common ./src/common

RUN CARGO_TARGET_DIR=/app/build/backend/target cargo build --release --manifest-path /app/src/backend/Cargo.toml && mkdir -p /app/dist/backend && cp /app/build/backend/target/release/momento-api /app/dist/backend/momento-api


FROM momento-api-runtime

COPY --from=backend-builder /app/dist/backend/momento-api /app/momento-api
COPY --from=momento-frontend /static ./static
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
