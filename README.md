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
├── logs/
└── llm/
    ├── cache/
    └── queue/
```

Back up the complete data directory, not only `database.sqlite`.

Log locations are fixed: both services write plain daily files named
`<service>.YYYY-MM-DD.log` under `<data_dir>/logs/`. Their event lines omit the service name and
process ID, while console output remains colorized by level. llm-service keeps its durable queue
and runtime cache under `<data_dir>/llm/`; deployments may persist that subtree independently.
