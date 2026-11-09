# Momento

[![Open Source](https://img.shields.io/badge/open%20source-yes-3da639.svg)](https://github.com/yzard/momento)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Docker Pulls](https://img.shields.io/docker/pulls/zhuoyin/momento.svg)](https://hub.docker.com/r/zhuoyin/momento)
[![Backend: Rust](https://img.shields.io/badge/backend-Rust-b7410e.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Frontend: React](https://img.shields.io/badge/frontend-React-149eca.svg?logo=react&logoColor=white)](https://react.dev/)

Momento is a self-hosted photo and video management service with optional AI features. It is designed for personal, family, and other small installations. It is not designed for a large number of users making concurrent requests.

Momento stores its application data in SQLite and keeps original media, thumbnails, previews, and AI inputs on the local filesystem.

Metadata converts camera RAW, HEIC/HEIF, TIFF, QOI, and other browser-unsupported images
to JPEG previews at their original pixel dimensions (with orientation applied), then generates
normal thumbnails from those previews. JPEG, PNG, GIF, WebP, AVIF, and BMP use originals directly
and skip preview generation. Full-resolution preview conversion never falls back to a smaller
embedded image; a conversion failure fails metadata generation explicitly.
The viewer's **Download original** button downloads the unchanged source file, including RAW and
video files. ImageMagick permits up to 1 GiB of memory-mapped pixel cache per conversion,
with temporary cache files kept in the mounted data directory.
Normal thumbnails default to a 400px square crop. Tiny thumbnails default to 48px and are
generated from the normal thumbnail for both photos and videos, without decoding the original
again. Place covers reuse normal thumbnails; WebGUI and Android crop them to 3:2 at display time.

## Key Points

- Self-hosted photo and video library.
- Uses SQLite instead of an external database server.
- Best suited to a small number of users.
- Imports local files and WebDAV uploads, absorbing exact byte-for-byte duplicates into one media record while granting access to each importing user.
- Understands Google Photos supplemental metadata sidecar files, keeps them beside the canonical original, and uses their values as authoritative capture time, location, and description metadata when available.
- Keeps AI processing in a separate service named `llm-service`.
- `llm-service` may run on the same machine as Momento or on another machine reachable over the network.
- All model runtimes run locally beside `llm-service`; remote model providers are not supported.

## iOS Client

The native SwiftUI client supports iOS 26 and later, with Liquid Glass controls,
adaptive compact/expanded layouts, library management, administration, and original-file
PhotoKit backup. It uses the same API as Android and intentionally omits Check for Updates.
Background execution and camera-only selection follow iOS platform rules; see the
[iOS client guide](docs/ios-client.md) for the feature map and exact differences.

On macOS with Xcode and XcodeGen:

```bash
./build_ios.sh build
./build_ios.sh verify
./build_ios.sh device --team YOUR_TEAM_ID --device YOUR_DEVICE_UDID
```

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
# Publish a client/Web/API update with its signed APK:
./build_docker.sh publish-api docker zhuoyin /secure/path/to/keystore-directory
```

The build calls `build_android_client.sh release`, creates and verifies the signed Android release
first, requires exactly one non-empty APK, and embeds it in the Momento image. It does not run Android
development or test commands. Signed-in users can download that APK from the Android download action
in the web sidebar or account settings; both serve `/momento-android.apk` from the same Momento
instance. Android may require the user to allow installs from the browser used for the download.

`publish-api` includes the same signed Android APK and Web assets, and leaves the separate
LLM image unchanged. Its orchestration contracts are covered by
`bash tests/docker/build_docker.sh`, which uses a disposable Docker substitute and no registry.

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
baked model weights for Unlimited-OCR-2, RAM++, DINOv2-base, CLIP ViT-B/32 with the LAION aesthetic
head, InsightFace `buffalo_l`, BiSeNet ResNet18 face parsing, and PP-OCRv6-small. Screenshot and document detection share a
PaddleOCR-assisted runtime design. Each active detector owns one CUDA pipeline and combines
concurrent requests into bounded GPU micro-batches. Task activation performs no package
installation or model download.

Requirements:

- Docker with the Compose plugin.
- An NVIDIA GPU, compatible host driver, and NVIDIA Container Toolkit.
- Sufficient disk space for the large model image and at least 8 GiB of shared memory for inference runtimes.
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

Momento API uses one scheduler thread, an asynchronous network runtime configured by
`[thread_pool].network_io_workers` (minimum 2), and three bounded executor domains configured by
`[thread_pool].cpu_workers`, `[thread_pool].storage_io_workers`, and
`[thread_pool].sqlite_workers`. HTTP/WebDAV orchestration stays on the network runtime; parsing,
hashing, encoding, and supervised media tools use CPU workers; filesystem and log operations use I/O
workers; and SQL uses SQLite workers. Metadata, LLM submission, and LLM result workers drain their
durable queues and then wait for versioned work signals; they have no poll-interval settings. WebDAV
and Android backup imports are event-driven, while local import starts only from an administrator
request. There are no per-feature concurrency settings.
`storage_io_workers` counts only filesystem threads (minimum 2): one dedicated reader, one
dedicated writer, and the remaining workers alternate between ready read/write queues. Logging,
cleanup, and mixed/mutating operations use write-capable workers; network threads are separate.
Defaults preserve the previous total: 2 network threads and 6 storage threads. The old
`io_workers` configuration key is rejected; update configurations before restarting.
r2d2 separately owns SQLite connections and its connection-maintenance thread. Each SQL operation is
still executed by a configured SQLite executor worker after checking out a connection; r2d2 does not
form another business-work queue.
`sqlite_workers` is the total SQLite executor thread count (minimum 2): exactly one writer
and `sqlite_workers - 1` readers. Read and write operations have separate bounded FIFO queues,
so queued writes cannot occupy reader workers. Read operations run with SQLite `query_only`
enabled. The r2d2 pool has the same connection limit; its one maintenance thread is additional.

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

The image pins the following models. A revision identifies an exact upstream checkpoint.
Full revisions and artifact checksums live in `docker/Dockerfile.llm-runtime`.

| Model | Version or pinned revision | Task | Purpose |
| --- | --- | --- | --- |
| [Baidu Unlimited-OCR-2](https://huggingface.co/baidu/Unlimited-OCR-2) | 2, released October 27, 2026. Revision `9cf077c4b206e0e2a31879df937b0b0bcba3661c` | `ocr` | Extract text and Markdown from original photos and full-resolution video representative frames. |
| [RAM++](https://github.com/xinyu1205/recognize-anything) | Swin-Large 14M checkpoint `84d4aee3a026`; implementation `7cb804a8609e` | `image_tagging` | Generate searchable visual tags without using OCR. |
| [Meta DINOv2-base](https://huggingface.co/facebook/dinov2-base) | Revision `f9e44c814b77` | `image_clustering` | Generate normalized 768-dimensional image vectors for duplicate grouping. Perceptual hashes and quality measurements supplement these vectors. |
| [OpenAI CLIP ViT-B/32](https://github.com/openai/CLIP) | Implementation `d05afc436d78`; checkpoint SHA-256 starts with `40d365715913` | `image_aesthetics` | Encode images for aesthetic scoring. Image measurements also produce scenic, simplicity, landscape, and technical-quality scores. |
| [LAION aesthetic predictor](https://huggingface.co/LAION/aesthetic-predictor) | Revision `6d122adad522`; linear head SHA-256 starts with `c7b14cead230` | `image_aesthetics` | Convert CLIP features into an aesthetic score. |
| [InsightFace buffalo_l](https://github.com/deepinsight/insightface) | Model pack from release `v0.7`; runtime `0.7.3` | `face_detection` | Detect faces and landmarks, then generate normalized 512-dimensional identity vectors for face grouping. |
| [BiSeNet ResNet18](https://github.com/yakhyo/face-parsing) | `resnet18.onnx`, release `weights`; SHA-256 starts with `0d9bd318e469` | `face_detection` | Segment facial regions to measure visibility and facial-feature clarity. |
| [PP-OCRv6-small detection](https://huggingface.co/PaddlePaddle/PP-OCRv6_small_det) | Revision `106c97591b23`; PaddleOCR `3.7.0` | `screenshot_detection`, `document_detection` | Locate text regions for layout classification. Document detection uses this detector without running text recognition. |
| [PP-OCRv6-small recognition](https://huggingface.co/PaddlePaddle/PP-OCRv6_small_rec) | Revision `bd619643acac`; PaddleOCR `3.7.0` | `screenshot_detection` | Read selected regions to help identify mobile screenshots. This model does not produce the library's primary OCR results. |

RAM++ also uses the `bert-base-uncased` tokenizer at revision `86b5e0934494`.
The tokenizer supports label processing and does not run as a separate inference feature.
Screenshot and document decisions combine PaddleOCR signals with visual heuristics.

Unlimited-OCR-2 runs with BF16 weights through Baidu's SGLang build `0.5.20.dev10+g8d62f5fb9c`.
The image pins and checks the upstream wheel's SHA-256 before installation.
The default OCR concurrency is `8`. SGLang batches these requests on the GPU.

Set `max_concurrent_jobs` in the OCR `[[service]]` section to change this limit.
Restart llm-service after a configuration change.

The adapter acquires capacity before opening or decoding each image.
It sends a lossless PNG to the engine through the service's loopback interface.

The default output limit is 65,536 tokens per input.
Set `max_tokens = 65536` in the OCR `[[service]]` section to configure this limit.
Restart llm-service after a configuration change.
The engine supports 131,072 total tokens for image input and generated output.
For longer output, `max_tokens = 122880` leaves 8,192 tokens for image input.
The runtime disables the upstream per-page length cap to prevent an earlier forced end.
It rejects truncated output rather than replacing stored text with an incomplete result.

This integration handles one image or representative frame per input.
The runtime logs request start and completion times. SGLang logs batch size and token throughput.

Each OCR result records `unlimited-ocr-2@9cf077c4b206e0e2a31879df937b0b0bcba3661c` as its model version.
After an upgrade, the administrator's OCR start action and OCR schedule queue media with missing or older results.
Existing text remains searchable until a successful result replaces it in SQLite.
Failed inference preserves existing text. Image tags, face results, and other AI results remain independent.

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
