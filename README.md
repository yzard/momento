# Momento

Momento is a self-hosted photo management application designed to give you full control over your media library. Similar to Google Photos, it provides a powerful interface for browsing, organizing, and sharing your photos and videos while keeping your data on your own hardware.

<!-- Screenshots: Add application screenshots here -->

## Features

- **Timeline View**: Browse your entire library chronologically with smart grouping by day, week, month, or year.
- **Map View**: Visualize your travels and memories on an interactive map using embedded GPS metadata.
- **Albums & Tags**: Organize your media into custom albums or use tags for quick categorization.
- **Public Sharing**: Create password-protected, expiring share links for individual photos or entire albums.
- **Trash System**: Secure soft-delete with a 30-day retention period for easy recovery.
- **Smart Imports**: Import media from local directories or via WebDAV with automated background processing.
- **Metadata Extraction**: Automatic extraction of EXIF data, including camera settings, timestamps, and location.
- **Optimized Previews**: High-performance thumbnail and preview generation for a smooth browsing experience.
- **Multi-User Support**: Full user management system with administrative controls.

## Quick Start (Docker)

The fastest way to get Momento running is using Docker Compose.

Start the application from the repository root:

```bash
docker compose -f docker/docker-compose.yml up --build -d
```

Access the web interface at `http://localhost:8000`.

## Installation from Source

### Prerequisites

- Node.js >= 20
- pnpm 9.15.0
- Rust (stable toolchain)
- System dependencies:
  - `ffmpeg` - video processing and thumbnail extraction
  - `imagemagick` - image processing
  - `exiftool` - metadata extraction
  - `libheif` - HEIC/HEIF image support

**Ubuntu/Debian:**
```bash
sudo apt install ffmpeg imagemagick libimage-exiftool-perl libheif-dev
```

**macOS:**
```bash
brew install ffmpeg imagemagick exiftool libheif
```

**Arch Linux:**
```bash
sudo pacman -S ffmpeg imagemagick perl-image-exiftool libheif
```

### Build & Run

**1. Clone the repository:**
```bash
git clone https://github.com/yourusername/momento.git
cd momento
```

**2. Build and run the full local playground:**
```bash
./run_playground.sh
```

The script builds artifacts under `build/`, runs them from `dist/`, and starts both
services with `playground/config.toml` and `playground/config_llm.toml`.

### Image OCR Service

Momento sends inference work to the standalone Rust LLM service in `src/backend_llm`.
`run_playground.sh` builds and starts both services together.

Momento submits manifest-first multipart jobs to `POST /api/v1/jobs/submit`. The LLM
service stores raw input bytes in its durable queue and returns task results through
Momento's configured internal callback endpoint.

The service supports `baidu` and `local` providers in `config_llm.toml`. The
playground uses the local provider and starts
`vllm/vllm-openai:unlimited-ocr` through Docker, so Docker with the NVIDIA
runtime and a GPU are required. On Linux, verify the NVIDIA Container Toolkit
is configured for Docker before running the playground:

```bash
docker run --rm --gpus all nvidia/cuda:12.9.1-base-ubuntu24.04 nvidia-smi
```

Momento's `llm.enabled` setting controls whether imported images are submitted
for OCR and image tagging. Image tagging uses a RAM++ adapter configured in
`playground/config_llm.toml`.

### Playground

Run the local playground with its E2E configuration and data:

```bash
./run_playground.sh
```

The playground reads `playground/config.toml` and `playground/config_llm.toml`
directly. Runtime data remains alongside the configs under `playground`; the runner does not
copy configuration files into that directory.

## Configuration

Momento is configured via a `config.toml` file located in your data directory (`/data` in Docker, or the current directory when running from source). A default configuration is generated on first run.

```toml
[server]
host = "0.0.0.0"
port = 8000
debug = false

[logging]
file_path = "/data/logs/momento-api.log"

[security]
secret_key = "change-me-in-production-use-openssl-rand-hex-32"
access_token_expire_minutes = 30
refresh_token_expire_days = 7

[admin]
username = "admin"
password = "admin"

[thumbnails]
max_size = 400
quality = 90

[reverse_geocoding]
enabled = true

[webdav]
enabled = false
mount_path = "/webdav"
```

**Important:** Change the `secret_key` to a secure random value in production. Generate one with:
```bash
openssl rand -hex 32
```

## Environment Variables

The server itself reads no environment variables — it is configured entirely by the
config file passed with `-c`. The variables below are consumed by the container's
entrypoint script, not by the application:

| Variable | Default | Description |
|----------|---------|-------------|
| `PUID` | 1000 | User ID for file permissions |
| `PGID` | 1000 | Group ID for file permissions |
| `UMASK` | 022 | Umask for created files |
| `TZ` | UTC | System timezone (e.g., `America/New_York`) |

Storage locations are set in the config file instead:

```toml
[storage]
data_dir = "/data"          # database and all media directories
static_dir = "/app/static"  # built frontend served as a fallback
```

## Default Credentials

- **Username:** `admin`
- **Password:** `admin`

You will be required to change the admin password immediately after your first login.

## Data Storage

All application data is stored within the data directory, organized as follows:

```
/data
├── config.toml      # Application configuration
├── database.sqlite  # SQLite database
├── logs/             # Application and model-service logs
├── originals/       # Original unmodified media files
├── thumbnails/      # Generated thumbnails for gallery views
├── previews/        # Web-optimized preview images
├── imports/         # Temporary directory for processing uploads
├── albums/          # Album cover images
└── trash/           # Soft-deleted files pending permanent removal
```

## License

This project is licensed under the MIT License.
