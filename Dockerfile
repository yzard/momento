# Stage 1: Build frontend
FROM node:20-alpine AS frontend-builder

WORKDIR /app

RUN corepack enable && corepack prepare pnpm@9.15.0 --activate

COPY package.json pnpm-lock.yaml* pnpm-workspace.yaml ./
COPY apps/web/package.json ./apps/web/
COPY packages/shared/package.json ./packages/shared/

RUN pnpm install --frozen-lockfile

COPY apps/web ./apps/web
COPY packages/shared ./packages/shared

WORKDIR /app/apps/web
RUN pnpm build

# Stage 2: Build and run API with frontend
FROM python:3.12-alpine

WORKDIR /app

# Install system dependencies
RUN apk add --no-cache \
    build-base \
    ffmpeg \
    imagemagick \
    libheif-dev \
    su-exec \
    shadow \
    tzdata

# Copy and install Python dependencies
COPY apps/api/pyproject.toml .
RUN pip install --no-cache-dir .

# Copy application code
COPY apps/api/momento_api/ ./momento_api/
COPY apps/api/schema.sql .

# Copy frontend build artifacts
COPY --from=frontend-builder /app/apps/web/dist ./static

# Copy entrypoint script
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# Create data directory
RUN mkdir -p /data

# Set default environment variables
ENV PUID=1000 \
    PGID=1000 \
    UMASK=022 \
    TZ=UTC \
    PYTHONPATH=/app \
    PYTHONUNBUFFERED=1 \
    MOMENTO_STATIC_DIR=/app/static

EXPOSE 8000

ENTRYPOINT ["/entrypoint.sh"]
CMD ["python", "-m", "uvicorn", "momento_api.main:create_application", "--factory", "--host", "0.0.0.0", "--port", "8000"]
