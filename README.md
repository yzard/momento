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

Momento prepares every AI input itself. For a photo it sends a prepared image; for a video it sends prepared frames. Each request contains the raw input bytes and their descriptors, so `llm-service` does not need access to Momento's filesystem.

`llm-service` is designed to keep only one AI model loaded at a time. This allows systems with about 16 GB of GPU memory to use several different AI features without loading every model together.

Jobs are grouped by type. The scheduler keeps a bounded rolling window for one task and replaces each completed job immediately until that task's queue is exhausted. It then unloads that model, loads the model required by the next task, and continues processing.

Model concurrency is enforced only by the active model subservice. Momento and the llm-service scheduler do not apply model-specific concurrency limits.

Accepted jobs are streamed into a durable disk queue before Momento receives an acknowledgement. The scheduler keeps bounded job metadata in memory and sends only validated job/input descriptors to the active same-machine model. Model containers read inputs from the queue's read-only `processing/` mount; image bytes are never retransmitted over the local HTTP boundary. Queue job count is not configured, so operators must monitor disk capacity.

AI cancellation is durable across machines. Momento records an all-task or task-specific scope plus exact job IDs in an outbox and retries the authenticated cancellation request until llm-service acknowledges it. llm-service removes matching staged, queued, callback-pending, and failed copies; matching inference already running finishes locally and is then discarded without callback delivery.

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

Momento detects faces and groups images containing similar people. Administrators can manually combine face groups when separate groups belong to the same person. The face runtime filters detections using the face service's `minimum_face_likelihood` and `minimum_face_resolution_pixels`; resolution is the minimum detected face-box width and height in the prepared input. Persisted 256x256 portrait crops retain their existing dimensions but are centered on the midpoint between the detected eyes. Momento groups embeddings when their cosine similarity reaches `[llm].face_group_similarity_threshold`; the default `0.55` preserves the previous behavior, lower values merge less-similar faces, and higher values require a closer match.

### Utility

**Deduplicate:** Momento generates image embeddings and perceptual hashes to find identical and near-identical images. This is useful for reviewing duplicate media before cleanup.

**Spend Audit:** Planned functionality will use receipt OCR and analysis to help review spending from receipt photos.

## Docker Compose Example

The following `docker/docker-compose.yml` example builds both services from this repository. It uses host networking on Linux because the default local AI configuration starts model runtimes as separate Docker containers.

```yaml
services:
  llm-service:
    build:
      context: ..
      dockerfile: docker/Dockerfile.llm
    network_mode: host
    command: ["/app/llm-service", "-c", "/data/config_llm.toml"]
    environment:
      PUID: "1000"
      PGID: "1000"
      UMASK: "022"
      TZ: "UTC"
    volumes:
      - /srv/momento/llm:/data
      - /var/run/docker.sock:/var/run/docker.sock
    restart: unless-stopped

  momento:
    build:
      context: ..
      dockerfile: docker/Dockerfile
    network_mode: host
    command: ["/app/momento-api", "-c", "/data/config.toml"]
    environment:
      PUID: "1000"
      PGID: "1000"
      UMASK: "022"
      TZ: "UTC"
    volumes:
      - /srv/momento/data:/data
    depends_on:
      - llm-service
    restart: unless-stopped
```

Before starting:

1. Create `/srv/momento/llm/config_llm.toml`. `playground/config_llm.toml` is an example; set `storage.queue_dir = "/data/queue"`, `storage.runtime_mount_source = "/srv/momento/llm/queue/processing"`, and `storage.runtime_mount_target = "/momento-inputs"`. The source uses the Docker host path because the llm-service starts sibling model containers through the host Docker socket.
2. Start Momento once to generate `/srv/momento/data/config.toml`, then configure its `[llm]` section.
3. Use the same API key in Momento's `llm.api_key` and llm-service's `general.api_key`.
4. Use the same callback key in Momento's `llm.callback_key` and llm-service's `callback.key`.
5. When both services use host networking, set `llm.service_url` to `http://127.0.0.1:8100` and `llm.callback_url` to `http://127.0.0.1:8000/api/v1/internal/llm/callback`.
6. Local AI models require an NVIDIA GPU, the NVIDIA Container Toolkit, and permission for `llm-service` to use the Docker socket.

Start the stack from the repository root:

```bash
docker compose -f docker/docker-compose.yml up --build -d
```

Open `http://localhost:8000`.

For development, the repository also provides:

```bash
./run_playground.sh
```

## Data

The Momento data directory contains the SQLite database and media files:

```text
/data/
├── config.toml
├── database.sqlite
├── originals/
├── thumbnails/
├── thumbnails_tiny/
├── previews/
├── imports/
├── webdav/
└── logs/
```

Back up the complete data directory, not only `database.sqlite`.

Log locations are fixed: each service writes plain daily files named
`<service>.YYYY-MM-DD.log` under its own `<data_dir>/logs/` directory. Console output remains
colorized by level.
