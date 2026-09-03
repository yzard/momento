# Momento

[![Open Source](https://img.shields.io/badge/open%20source-yes-3da639.svg)](https://github.com/yzard/momento)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Docker Pulls](https://img.shields.io/docker/pulls/zhuoyin/momento.svg)](https://hub.docker.com/r/zhuoyin/momento)
[![Backend: Rust](https://img.shields.io/badge/backend-Rust-b7410e.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Frontend: React](https://img.shields.io/badge/frontend-React-149eca.svg?logo=react&logoColor=white)](https://react.dev/)

Momento is a self-hosted photo and video management service with optional AI features. It is designed for personal, family, and other small installations. It is not designed for a large number of users making concurrent requests.

Momento stores its application data in SQLite and keeps original media, thumbnails, previews, and AI inputs on the local filesystem.

## Key Points

- Self-hosted photo and video library.
- Uses SQLite instead of an external database server.
- Best suited to a small number of users.
- Imports local files and WebDAV uploads, absorbing exact byte-for-byte duplicates into one media record while granting access to each importing user.
- Understands Google Photos supplemental metadata sidecar files, keeps them beside the canonical original, and uses their values as authoritative capture time, location, and description metadata when available.
- Keeps AI processing in a separate service named `llm-service`.
- `llm-service` may run on the same machine as Momento or on another machine reachable over the network.
- All model runtimes run locally beside `llm-service`; remote model providers are not supported.

## Android Client

The Android client connects to a Momento server for library browsing, timeline, albums, places,
faces, search, media viewing, duplicate review, and device backup. It supports photo and video
backup from the device media library; backups default to Wi-Fi only and run on a daily schedule.
Grant the requested photo/video media permission to back up content. Android 13 and later use the
system photo/video permissions; older Android versions use storage-read permission. Notifications
are requested for visible foreground backup progress.

On first run, enter the complete Momento server origin and authenticate. Signed release APKs require
HTTPS and Android also blocks release cleartext traffic at the platform level. Debug builds permit
HTTP after an explicit warning so an emulator can reach `http://10.0.2.2:<port>` and a development
device can reach a trusted local server. An HTTPS reverse proxy may use HTTP on its private upstream
connection because the Android client connects only to the proxy's HTTPS origin.

All Android compilation, debugging, and tests run in Docker. The host needs only Docker:

```bash
./build_android_client.sh verify
./build_android_client.sh assemble-debug
./build_android_client.sh shell
```

`verify` compiles the debug variant, runs JVM unit tests, and runs Android lint. `shell` opens the
containerized Java 17, Gradle, Android SDK 36, and ADB environment. A debug APK is written to
`dist/android/debug/`.

Instrumented tests use a headless Android emulator inside a separate Docker target. They require a
Linux Docker host with KVM exposed as `/dev/kvm`:

```bash
./build_android_client.sh instrumented-test
```

Create a signed release with a directory containing exactly one direct `.jks` file and a
`password.txt` file containing exactly one non-empty line. The password is used for both the
keystore and key, and the keystore must contain exactly one private-key alias:

```bash
./build_android_client.sh release --keystore-dir /secure/path/to/keystore-directory
```

The release script requires Docker only; Java, Gradle, and Android SDK 36 run inside the pinned
`docker/Dockerfile.android` builder image. The keystore directory is mounted read-only, and signing
values remain inside the temporary build container rather than appearing in Docker command arguments.
The script stages Gradle state below `build/android/` and writes the final signed artifacts only to
`dist/android/<keystore-stem>-<android-version>.apk` and
`dist/android/<keystore-stem>-<android-version>.aab`. The Android version is owned by
`src/android/version.txt`; it starts at `1.0.0` and is independent from the Momento server version.
Run `./build_android_client.sh --help` for every command, option, requirement, and output path.

## Docker Compose

The canonical deployment definition is [`docker-compose.yaml`](docker-compose.yaml).
It runs `momento-api` and `momento-llm-service` on one private bridge network. Only Momento publishes
port 8000; the LLM service is reachable only from Momento at `momento-llm-service:8100`.

Published images are available from Docker Hub:

- [`zhuoyin/momento`](https://hub.docker.com/r/zhuoyin/momento)
- [`zhuoyin/momento-llm-service`](https://hub.docker.com/r/zhuoyin/momento-llm-service)

The following is the complete content of the root `docker-compose.yaml`:

```yaml
services:
  momento-api:
    container_name: momento-api
    image: zhuoyin/momento:latest
    restart: unless-stopped
    environment:
      PUID: "${PUID:-1000}"
      PGID: "${PGID:-1000}"
      UMASK: "${UMASK:-022}"
      TZ: "${TZ:-UTC}"
      RESET_ADMIN_PASSWORD: "${RESET_ADMIN_PASSWORD:-false}"
      SECRET_KEY: "${SECRET_KEY:-playground-only-change-this-secret-before-exposing-the-server}"
      LLM_SERVICE_API_KEY: "${LLM_SERVICE_API_KEY:-change-me-llm-service-key}"
      LLM_SERVICE_ADDRESS: "momento-llm-service:8100"
    ports:
      - "8000:8000"
    volumes:
      - "${MOMENTO_DATA_DIR:-./data}:/data"
    networks:
      - momento
    depends_on:
      momento-llm-service:
        condition: service_started

  momento-llm-service:
    container_name: momento-llm-service
    image: zhuoyin/momento-llm-service:latest
    restart: unless-stopped
    environment:
      PUID: "${PUID:-1000}"
      PGID: "${PGID:-1000}"
      UMASK: "${UMASK:-022}"
      TZ: "${TZ:-UTC}"
      LLM_SERVICE_API_KEY: "${LLM_SERVICE_API_KEY:-change-me-llm-service-key}"
    volumes:
      - "${MOMENTO_DATA_DIR:-./data}:/data"
    networks:
      - momento
    gpus: all
    shm_size: "8gb"

networks:
  momento:
```

Each container has one `/data` mount backed by the same host directory, which defaults to `./data`
relative to the repository root. Their entrypoints create
`/data/config.toml` and `/data/config_llm.toml` on first startup. Momento resolves
`${LLM_SERVICE_ADDRESS}` into `[llm].server_address`; the value contains only the private service
host and port. Momento owns the WebSocket scheme and path.

Start the stack and open `http://localhost:8000`:

```bash
docker compose up -d
```

Build both images locally with the tags used by Compose, or explicitly publish them:

```bash
./build_docker.sh /secure/path/to/keystore-directory
./build_docker.sh publish docker zhuoyin /secure/path/to/keystore-directory
```

The build calls `build_android_client.sh release`, creates and verifies the signed Android release
first, requires exactly one non-empty APK, and embeds it in the Momento image. It does not run Android
development or test commands. Signed-in users can download that APK from the Android download action
in the web sidebar or account settings; both serve `/momento-android.apk` from the same Momento
instance. Android may require the user to allow installs from the browser used for the download.

Compose supplies one `LLM_SERVICE_API_KEY` to both services, `SECRET_KEY` to Momento, and
`RESET_ADMIN_PASSWORD=false` by default. Set strong `LLM_SERVICE_API_KEY` and `SECRET_KEY` environment values
before exposing the service. A new database creates `admin` / `admin` and requires an immediate
password change.

To recover administrator access, set `RESET_ADMIN_PASSWORD=true` for one Momento startup. For that
server process only, Momento accepts `admin` / `admin` for the existing administrator account. The
stored password is not replaced until the forced password-change form succeeds. Remove the
environment override after startup so a later restart does not request recovery again.

The LLM image requires an NVIDIA GPU, a compatible host driver, and NVIDIA Container Toolkit. It
is large because all model runtimes and weights are included for offline activation.

## PhotoSync WebDAV

WebDAV is always available at `/webdav`. Configure PhotoSync with the complete server URL, such
as `https://photos.example.com/webdav/`, and use an active Momento user's username and password
with WebDAV/Basic authentication. Use HTTPS whenever the service is reachable outside a trusted
local network.

PhotoSync may create directories, upload to a hidden temporary filename, and rename the completed
file into place. Momento supports that OPTIONS, PROPFIND, MKCOL, PUT, and MOVE sequence. Completed
files are staged below `/data/webdav/<username>/`, imported for that user after the configured
stability interval, and removed from the staging directory after a successful import. PUT accepts
either a declared content length or chunked transfer encoding; chunked uploads are bounded while
streaming and do not need to be buffered in memory. PATCH must declare its chunk size. Every upload
is limited by `webdav.max_upload_bytes`, and oversized partial files are removed.

Momento records a file as import-ready only after a complete PUT finishes, a ranged PATCH reaches
its declared total, or a MOVE/COPY finalizes the destination. The importer exclusively claims that
closed file before calculating SHA-256, so an active or paused partial transfer is never hashed or
queued. Read-only WebDAV requests do not pause importing. The generated defaults check every second,
apply a two-second settling delay, and process up to four completed files concurrently.

Local and WebDAV imports calculate a SHA-256 content hash before allocating a media record. An
exact duplicate reuses the existing media ID and canonical original, grants the importing user
access, and keeps the earliest imported file modification time as the record's creation fallback.
If the duplicate includes a supplemental metadata sidecar, Momento moves it beside the canonical
original and regenerates metadata so newly supplied values replace older values while fields not
present in the sidecar continue to come from the original media.

## Features

### Category

Momento supports photos and videos. Optional local classifiers identify screenshots and document
photos. Classification is photo-only, and the two categories are independent, so one photo may
appear in both the Screenshots and Documents Timeline views. Each result stores the classifier's
boolean decision and confidence. Receipt-specific detection remains planned.

### Search

Search currently supports:

- Text extracted with OCR
- AI-generated object tags

### Faces

Momento detects faces and groups images containing similar people. Administrators can manually combine face groups when separate groups belong to the same person. The face runtime filters detections using the face service's `minimum_face_likelihood` and `minimum_face_resolution_pixels`; resolution is the minimum detected face-box width and height in the prepared input. `face_detection_size` selects the square SCRFD input and accepts only `640`, `960`, or `1280`. Detection uses `processing_concurrency` and `model_concurrency`, while accepted faces are aligned to the Buffalo L model's fixed 112x112 ArcFace input and combined across requests according to `recognition_batch_size` and `recognition_batch_wait_milliseconds`. One CUDA BiSeNet ResNet18 ONNX session dynamically batches those aligned faces at 512x512 and adds facial-feature visibility and clarity scores. Persisted 256x256 portrait crops retain their existing dimensions but are centered on the midpoint between the detected eyes. Momento groups embeddings when their cosine similarity reaches `[face_group].similarity_threshold`; the default is `0.50`, lower values merge less-similar faces, and higher values require a closer match. After automatic grouping or a manual merge, the group thumbnail representative uses the configurable confidence, face size, center proximity, frontality, visibility, and feature clarity weights in `[face_group]`; changed weights are applied to existing groups at the next startup.

### Places

Places groups accessible media by the exact reverse-geocoded city, state or province, and country tuple. A missing state remains distinct from a named state. Place covers use aspect-preserving prepared images and prefer completed `image_aesthetics` results: 40% LAION aesthetic score, 25% CLIP scenic suitability, 20% visual simplicity, 10% landscape composition, and 5% technical quality, with OCR-clutter and dominant-face penalties. When aesthetics have not run, Momento uses a deterministic landscape/date/media-ID fallback. Momento does not persist a selected place cover: every thumbnail request ranks the requesting user's currently accessible place media from live database state, so additions, deletions, access changes, and new aesthetic results take effect on the next request.

### Utility

**Deduplicate:** Momento generates image embeddings and perceptual hashes to find identical and near-identical images. This is useful for reviewing duplicate media before cleanup.

**Spend Audit:** Planned functionality will use receipt OCR and analysis to help review spending from receipt photos.

## Docker Playground

The playground uses the same [`docker-compose.yaml`](docker-compose.yaml) as normal
deployments and builds exactly two containers: `momento-api` and `momento-llm-service`.
Model runtimes are processes inside `momento-llm-service`; the deployment does not mount the Docker socket
or create model containers. The CUDA 12.9 llm-service image contains isolated environments and
baked model weights for Unlimited-OCR, RAM++, DINOv2-base, CLIP ViT-B/32 with the LAION aesthetic
head, InsightFace `buffalo_l`, BiSeNet ResNet18 face parsing, and PP-OCRv6-small. Screenshot and document detection share a
PaddleOCR-assisted runtime design. Each active detector owns one CUDA pipeline and combines
concurrent requests into bounded GPU micro-batches. Task activation performs no package
installation or model download.

Requirements:

- Docker with the Compose plugin.
- An NVIDIA GPU, compatible host driver, and NVIDIA Container Toolkit.
- Sufficient disk space for the large model image and at least 8 GiB shared memory for vLLM.
- Review of the bundled model licenses before redistributing the llm-service image.

Run from any working directory:

```bash
./run_playground.sh /secure/path/to/keystore-directory
```

The script builds the signed Android release and both images, mounts `playground/` as `/data` in both containers, passes the
invoking UID/GID, starts the stack on the private Compose network, and removes the containers on
exit. Open `http://localhost:8000`.

Momento resolves `LLM_SERVICE_ADDRESS=momento-llm-service:8100` in its checked-in playground
configuration and maintains an authenticated WebSocket to that address. Submissions,
cancellations, acknowledgements, and
results share that connection, so llm-service does not need a Momento address. The checked-in
playground configs resolve `RESET_ADMIN_PASSWORD`, `SECRET_KEY`, and the shared `LLM_SERVICE_API_KEY` from
Compose and identify the Momento connection as `playground`. Production deployments must replace
the default secrets and persist the complete data directory.

## Data

The playground mounts one data root into both containers for convenience. Production deployments
may place Momento and llm-service on separate machines with unrelated storage. llm-service never
opens a Momento path: it receives source bytes over the authenticated WebSocket and stores its own
queue, cache, temporary files, and logs below its configured `/data` volume. The playground layout is:

```text
/data/
├── config.toml
├── config_llm.toml
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
└── llm/
    ├── cache/
    └── queue/
```

Back up the complete data directory, not only `database.sqlite`.

Log locations are fixed: both services write plain daily files named
`<service>.YYYY-MM-DD.log` under `<data_dir>/logs/`. Their event lines omit the service name and
process ID, while console output remains colorized by level. llm-service keeps its durable queue
and runtime cache under `<data_dir>/llm/`; deployments may persist that subtree independently.

The generated `[llm]` section contains one global `enabled` switch and an independent five-field
cron schedule for each AI feature. Schedules use the host or container system timezone; Momento
does not configure a separate cron timezone. When global LLM processing is enabled, OCR, image
tagging, deduplication, face detection, image aesthetics, screenshot detection, and document
detection are all active; there are no per-feature enablement switches.
`deduplicate_cron` intentionally keeps its feature name because it starts the complete
deduplication pipeline; `image_clustering` is only that pipeline's inference stage.
Administrators can edit these schedules from the AI section in either the web or Android client.
The AI work-status table shows the latest job for each media item and feature. Successful reruns
therefore replace older failures in the displayed totals while the historical jobs remain stored,
and valid empty results such as an image with no detected faces count as completed.
Momento validates the complete prospective configuration, atomically updates the original
`config.toml` while preserving comments and unrelated values, and reschedules the affected cron
loop immediately without a service restart.

Momento API uses one scheduler thread, a fixed two-thread network runtime, and three bounded FIFO
executor domains configured by `[thread_pool].cpu_workers`, `[thread_pool].io_workers`, and
`[thread_pool].sqlite_workers`. HTTP/WebDAV orchestration stays on the network runtime; parsing,
hashing, encoding, and supervised media tools use CPU workers; filesystem and log operations use I/O
workers; and SQL uses SQLite workers. Metadata, LLM submission, and LLM result workers drain their
durable queues and then wait for versioned work signals; they have no poll-interval settings. WebDAV
and Android backup imports are event-driven, while local import starts only from an administrator
request. There are no per-feature concurrency settings.
r2d2 separately owns SQLite connections and its connection-maintenance thread. Each SQL operation is
still executed by a configured SQLite executor worker after checking out a connection; r2d2 does not
form another business-work queue.

GPS coordinates are reverse geocoded entirely on-device with a pinned GeoNames `cities500`
snapshot embedded in the Momento API binary. No external geocoding service or runtime network
request is used. GeoNames data is provided under CC BY 4.0; attribution and source checksums are
recorded in `src/backend/assets/geonames/SOURCE.md`.

## AI Architecture

Momento's AI features run locally inside `llm-service`. The name covers several kinds of machine
learning runtime, not only large language models. Model packages and weights are baked into the
container image, and normal activation does not install packages, download weights, or call a
hosted inference API.

### Models and tasks

| Task | Model or method | Compute | Output | Why this approach |
| --- | --- | --- | --- | --- |
| `ocr` | Baidu Unlimited-OCR served by vLLM | GPU | Recognized text and Markdown | Unlimited-OCR is Momento's full OCR model for photos and representative video frames. It is used when the recognized content itself is required. |
| `image_tagging` | RAM++ with the Swin-Large checkpoint | GPU | Searchable semantic tags | RAM++ is a large-vocabulary image-tagging model. It recognizes visual concepts directly and does not use Unlimited-OCR. |
| `image_clustering` | Meta DINOv2-base | GPU | Normalized 768-dimensional embedding, perceptual hash, and image-quality score | CPU workers preserve the full-image perceptual hash while bounding quality analysis to 512 pixels, then one CUDA model instance dynamically batches prepared DINOv2 tensors across requests. |
| `image_aesthetics` | OpenAI CLIP ViT-B/32 with a LAION aesthetic linear head | GPU | Aesthetic, scenic, simplicity, landscape, and technical-quality scores | CPU workers share grayscale and edge analysis for the image measurements, then one CUDA model instance dynamically batches prepared CLIP tensors across requests. |
| `face_detection` | InsightFace `buffalo_l` + BiSeNet ResNet18 | GPU | Face boxes, landmarks, normalized 512-dimensional identity embeddings, face size, frontality, visibility, and facial-feature clarity | One copy of each model remains on CUDA; recognition and face parsing dynamically batch aligned faces across requests, while configurable weights select group representatives. |
| `screenshot_detection` | PP-OCRv6-small plus visual heuristics | GPU | Boolean decision and confidence | CPU workers letterbox images onto a fixed 1280x1280 canvas so one detector instance can dynamically batch differently shaped requests on CUDA. A separate recognition instance batches only top status-region crops, while CPU preparation, layout parsing, and classification overlap through a bounded pipeline. The classifier combines these OCR regions with mobile aspect ratio, compact UI components, edge geometry, and flat-color structure. |
| `document_detection` | PP-OCRv6-small detection plus visual heuristics | GPU | Boolean decision and confidence | CPU workers letterbox images onto a fixed 1280x1280 canvas so one detector instance can dynamically batch differently shaped requests on CUDA. Document classification uses text geometry and confidence without loading or running text recognition; CPU preparation, layout parsing, and classification overlap through a bounded pipeline. The classifier combines these regions with paper-like color and photographic-content penalties. |

| OCR choice | Best use | Layout data | Hardware | Role in Momento |
| --- | --- | --- | --- | --- |
| Unlimited-OCR | High-quality text recognition | Returns text/Markdown rather than the normalized word regions required by the classifiers | GPU | Primary `ocr` task |
| PP-OCRv6-small | Fast spatial text hints | Returns text boxes and recognition confidence directly | GPU | Internal batched signal for screenshot and document classification; not a replacement for primary OCR |

| Classifier behavior | Screenshot detection | Document detection |
| --- | --- | --- |
| Eligible media | Photos only | Photos only |
| Video inputs | Never created | Never created |
| Can overlap | Yes; a photo may also be a document | Yes; a photo may also be a screenshot |

### Input preparation and scheduling

Momento owns every AI input reference. Photo jobs stream the immutable full-resolution canonical
original; video-capable jobs stream one shared lossless full-resolution representative frame.
Screenshot and document detection remain photo-only. UI thumbnails and browser previews are never
AI inputs. Each request contains ordered descriptors, and llm-service asks Momento to transmit only
content hashes it does not already hold. Identical inputs for concurrent task types are hard-linked
into their job directories, so a large original is stored once in the llm-service mounted volume.
Camera RAW inputs (including DNG, CR2/CR3, NEF/NRW, ARW, RW2, ORF, RAF, PEF, SRW, and generic RAW)
are decoded at full resolution by LibRaw inside llm-service; the resulting TIFF is cached by source
content hash and shared across AI task types. Neither service uses a shared filesystem or `/tmp` for
runtime task data. The Momento API image includes ImageMagick's LibRaw module as a separate dependency
so metadata generation and browser thumbnails use the same original RAW files without an encoded-file
size cutoff.

`llm-service` keeps only one AI model loaded at a time. Jobs are grouped by type, and the scheduler
keeps a bounded rolling window for one task until its queue is exhausted. It then unloads that
model, loads the model required by the next task, and continues processing. This allows systems
with about 16 GB of GPU memory to use several AI features without loading every model together.

Model concurrency is enforced only by the active model subservice. Momento and the llm-service
scheduler do not apply model-specific concurrency limits.

### Durable execution

Accepted jobs are streamed into a durable disk queue before Momento receives an acknowledgement.
The queue keeps a content-addressed source store and links each job to it; a durable receipt from
Momento removes the job and removes cached source/RAW-normalized data when the final job reference is
gone.
The scheduler keeps bounded job metadata in memory and sends only validated descriptors to the
active same-container model process. The model opens inputs from `queue/processing`; image bytes
are never retransmitted over the local HTTP boundary. `[scheduler].max_queue_bytes` limits unique
source bytes retained by the durable queue, while `working_space_reserve_bytes` preserves filesystem
headroom for RAW normalization, inference results, queue metadata, and logs. If capacity is full,
llm-service defers admission before any payload bytes are sent and Momento retries the same queued
job without consuming an attempt. Inputs larger than the configured queue budget are rejected
permanently. Operators should still monitor disk usage because model output, normalized RAW data,
metadata, and logs are not source bytes counted by `max_queue_bytes`.

AI cancellation is durable across machines. Momento records an all-task or task-specific scope
plus exact job IDs in an outbox and retries until llm-service acknowledges it. Matching inference
already running finishes locally and is discarded without result delivery. Replacement jobs for a
pending cancellation scope remain queued until acknowledgement, preventing reset work from being
cancelled by its own delayed request.
