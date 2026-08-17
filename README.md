# Momento

Momento is a self-hosted photo and video management service with optional AI features. It is designed for personal, family, and other small installations. It is not designed for a large number of users making concurrent requests.

Momento stores its application data in SQLite and keeps original media, thumbnails, previews, and AI inputs on the local filesystem.

## Key Points

- Self-hosted photo and video library.
- Uses SQLite instead of an external database server.
- Best suited to a small number of users.
- Imports local files and WebDAV uploads.
- Understands Google Photos supplemental metadata sidecar files and uses them to restore information such as capture time and location when available.
- Keeps AI processing in a separate service named `llm-service`.
- `llm-service` may run on the same machine as Momento or on another machine reachable over the network.
- All model runtimes run locally beside `llm-service`; remote model providers are not supported.

## AI Architecture

Momento prepares every AI input itself. For a photo it sends a prepared image; for a video it currently sends a prepared representative frame. Each request contains the raw input bytes and its descriptor, while the transport preserves multiple ordered inputs for future preparation strategies. `llm-service` does not need access to Momento's filesystem.

`llm-service` is designed to keep only one AI model loaded at a time. This allows systems with about 16 GB of GPU memory to use several different AI features without loading every model together.

Jobs are grouped by type. The scheduler keeps a bounded rolling window for one task and replaces each completed job immediately until that task's queue is exhausted. It then unloads that model, loads the model required by the next task, and continues processing.

Model concurrency is enforced only by the active model subservice. Momento and the llm-service scheduler do not apply model-specific concurrency limits.

Accepted jobs are streamed into a durable disk queue before Momento receives an acknowledgement. The scheduler keeps bounded job metadata in memory and sends only validated job/input descriptors to the active same-container model process. The model opens inputs from `queue/processing`; image bytes are never retransmitted over the local HTTP boundary. Queue job count is not configured, so operators must monitor disk capacity.

AI cancellation is durable across machines. Momento records an all-task or task-specific scope plus exact job IDs in an outbox and retries the authenticated cancellation message until llm-service acknowledges it. llm-service removes matching staged, queued, result-pending, and failed copies; matching inference already running finishes locally and is then discarded without result delivery.

## Features

### Category

Momento supports photos and videos.

Planned category detection will identify special media such as:

- Screenshots
- Document photos
- Receipt photos

### Search

Search currently supports:

- Text extracted with OCR
- AI-generated object tags

### Faces

Momento detects faces and groups images containing similar people. Administrators can manually combine face groups when separate groups belong to the same person. The face runtime filters detections using the face service's `minimum_face_likelihood` and `minimum_face_resolution_pixels`; resolution is the minimum detected face-box width and height in the prepared input. Persisted 256x256 portrait crops retain their existing dimensions but are centered on the midpoint between the detected eyes. Momento groups embeddings when their cosine similarity reaches `[llm].face_group_similarity_threshold`; the default `0.55` preserves the previous behavior, lower values merge less-similar faces, and higher values require a closer match. After automatic grouping or a manual merge, the group thumbnail representative is selected first by the face-box center's distance from the media center, then by a five-landmark frontality score, quality, confidence, and stable face ID.

### Utility

**Deduplicate:** Momento generates image embeddings and perceptual hashes to find identical and near-identical images. This is useful for reviewing duplicate media before cleanup.

**Spend Audit:** Planned functionality will use receipt OCR and analysis to help review spending from receipt photos.

## Docker Compose

Published images are available from Docker Hub:

- [`zhuoyin/momento`](https://hub.docker.com/r/zhuoyin/momento)
- [`zhuoyin/momento-llm-service`](https://hub.docker.com/r/zhuoyin/momento-llm-service)

No data subdirectories or configuration files need to be created manually. Docker and the two
container entrypoints create them on first startup, including `data/config.toml` and
`data/llm-config/config_llm.toml`. The generated `[llm].api_key` and `[server].api_key` match.
Replace that shared key, the security secret, and the administrator password in both generated
files before exposing the service, then restart the containers.

```yaml
services:
  llm-service:
    image: zhuoyin/momento-llm-service:latest
    environment:
      PUID: "${PUID:-1000}"
      PGID: "${PGID:-1000}"
      UMASK: "${UMASK:-022}"
      TZ: "${TZ:-UTC}"
    volumes:
      - ./data/llm:/data/llm
      - ./data/logs:/data/logs
      - ./data/llm-config:/config
    expose:
      - "8100"
    gpus: all
    shm_size: "8gb"
    healthcheck:
      test: ["CMD", "curl", "--fail", "http://127.0.0.1:8100/ready"]
      interval: 5s
      timeout: 3s
      retries: 20
      start_period: 10s
    stop_grace_period: 45s
    restart: unless-stopped

  momento-api:
    image: zhuoyin/momento:latest
    environment:
      PUID: "${PUID:-1000}"
      PGID: "${PGID:-1000}"
      UMASK: "${UMASK:-022}"
      TZ: "${TZ:-UTC}"
    volumes:
      - ./data:/data
    ports:
      - "127.0.0.1:8000:8000"
    depends_on:
      llm-service:
        condition: service_healthy
    init: true
    stop_grace_period: 30s
    restart: unless-stopped
```

Start the stack and open `http://localhost:8000`:

```bash
docker compose up -d
```

The llm-service image requires an NVIDIA GPU, a compatible host driver, and NVIDIA Container
Toolkit. It is large because all model runtimes and weights are included for offline activation.
The generated `[cronjob]` section contains independent schedules for OCR, image tagging,
deduplication, and face detection. `deduplicate_cron` intentionally keeps its feature name because
it starts the complete deduplication pipeline; `image_clustering` is only that pipeline's inference
stage.

## Docker Playground

The playground builds and starts exactly two containers: `momento-api` and `llm-service`.
Model runtimes are processes inside `llm-service`; the deployment does not mount the Docker socket
or create model containers. The CUDA 12.9 llm-service image contains isolated environments and
baked model weights for Unlimited-OCR, RAM++, DINOv2-small, and InsightFace `buffalo_l`, so task
activation performs no package installation or model download.

Requirements:

- Docker with the Compose plugin.
- An NVIDIA GPU, compatible host driver, and NVIDIA Container Toolkit.
- Sufficient disk space for the large model image and at least 8 GiB shared memory for vLLM.
- Review of the bundled model licenses before redistributing the llm-service image.

Run from any working directory:

```bash
./run_playground.sh
```

The script builds both images, passes the invoking UID/GID, starts the stack on a private Compose
network, and removes the containers on exit. Open `http://localhost:8000`.

Momento maintains an authenticated WebSocket to
`ws://llm-service:8100/api/v1/llm/connect`. Submissions, cancellations, acknowledgements, and
results share that connection, so llm-service does not need a Momento address. The checked-in
playground configs share one API key and identify the Momento connection as `playground`.
Production deployments must replace that key and persist Momento data and the llm-service queue
independently.

## PhotoSync WebDAV

WebDAV is always available at `/webdav`. Configure PhotoSync with the complete server URL, such
as `https://photos.example.com/webdav/`, and use an active Momento user's username and password
with WebDAV/Basic authentication. Use HTTPS whenever the service is reachable outside a trusted
local network.

PhotoSync may create directories, upload to a hidden temporary filename, and rename the completed
file into place. Momento supports that OPTIONS, PROPFIND, MKCOL, PUT, and MOVE sequence. Completed
files are staged below `/data/webdav/<username>/`, imported for that user after the configured
stability interval, and removed from the staging directory after a successful import. PUT and
PATCH requests must declare their byte size and cannot exceed `webdav.max_upload_bytes`.

## Data

The Momento data directory contains the SQLite database and media files:

```text
/data/
├── config.toml
├── database.sqlite
├── albums/
├── originals/
├── thumbnails/
├── thumbnails_tiny/
├── previews/
├── imports/
├── webdav/
├── trash/
├── logs/
├── llm-config/
│   └── config_llm.toml
└── llm/
    ├── cache/
    └── queue/
```

Back up the complete data directory, not only `database.sqlite`.

Log locations are fixed: both services write plain daily files named
`<service>.YYYY-MM-DD.log` under `<data_dir>/logs/`. Their event lines omit the service name and
process ID, while console output remains colorized by level. llm-service keeps its durable queue
and runtime cache under `<data_dir>/llm/`; deployments may persist that subtree independently.
