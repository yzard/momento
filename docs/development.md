# Build and test reference

Use the relevant section when building, testing, or running Momento.

Before running host development tools, initialize repository-owned temporary storage from the repository root:

```bash
mkdir -p build/tmp
export TMPDIR="$PWD/build/tmp" TEMP="$PWD/build/tmp" TMP="$PWD/build/tmp"
```

Choose commands for the affected component. This is a reference, not a checklist to run in full.
Commands that publish images or start services require the corresponding task scope and configuration.

### Root
```bash
pnpm install              # Install all dependencies
pnpm build                # Build all packages
pnpm dev                  # Dev servers (backend + frontend)
pnpm lint                 # Lint all packages
pnpm test                 # Run all tests

./run_playground.sh /path/to/keystore                         # Build and run the playground stack
./build_docker.sh /path/to/keystore                           # Build both images locally
./build_docker.sh publish github yzard /path/to/keystore      # Build and publish both images to GHCR
./build_android_client.sh verify                              # Android compile, JVM tests, and lint in Docker
./build_android_client.sh assemble-debug                      # Android debug APK in Docker
./build_android_client.sh instrumented-test                   # Containerized emulator tests; requires /dev/kvm
./build_android_client.sh shell                               # Containerized Java/Gradle/SDK/ADB shell
./build_android_client.sh release --keystore-dir /path/to/keystore
```

`run_playground.sh`, `build_docker.sh`, and the explicitly separate Android entrypoint
`build_android_client.sh` are the supported scripts at the git root. They resolve paths from their
own location, so they work from any working directory. Android compilation, debugging, lint, and
tests must use `build_android_client.sh`; direct host Gradle, Java, Android SDK, emulator, or ADB
commands are unsupported.

### Playground containers

`run_playground.sh` uses `build_docker.sh` and the root `docker-compose.yaml` to build and start exactly two containers:
`momento-api` and `momento-llm-service`. It does not compile or run host binaries. The Momento image embeds
the built frontend, while the llm-service image embeds five isolated Python environments and all
model weights. Versioned runtime artifacts preserve model weights across builds without retaining
disposable compiler workspaces.

The script passes the invoking UID/GID and mounts `playground/` as `/data` in both containers. Each
service therefore has one volume mount. It tears down both containers on exit and never
mounts the Docker socket. Runtime model processes are spawned inside the llm-service container, so
inference never creates additional containers.

`playground/logs/` holds both services' logs. llm-service queue and runtime cache remain below
`playground/llm/`, while its config remains at `playground/config_llm.toml`. Build artifacts do not
belong in either directory.

### Android client

`build_android_client.sh` is the only Android build, debug, and test entrypoint. Its `verify`,
`assemble-debug`, `instrumented-test`, `shell`, and `release` commands all run in Docker. Only
`release` accepts `--keystore-dir`; only `instrumented-test` uses the emulator image and `/dev/kvm`.
Intermediate state belongs below `build/android/`, debug APKs below
`dist/android/debug/`, and the single release APK plus AAB directly below
`dist/android/`. Run `./build_android_client.sh --help` for the complete option contract.

`build_docker.sh` calls only `build_android_client.sh release`, then verifies and embeds the one
release APK into the momento-api image as `/app/static/momento-android.apk`. It separately builds
llm-service and never routes Android development or test commands.

Successful Android commands preserve reports and the debug signing identity in the per-project
XDG state directory described below. They remove staged projects, Gradle caches and emulator data.
Explicitly exported debug APKs remain deliverables until a release replaces them.
Unchanged release/debug inputs reuse verified APK/AAB files. Explicit `verify` and
`instrumented-test` commands always run their checks. Android SDK images remain reusable.
The wrapper downloads Gradle from `downloads.gradle.org` and compares its SHA-256 checksum with the pinned value.

Every installable APK requires a digital signature. Release builds use the supplied persistent signing key.
The Android "Allow from this source" setting authorizes the app that opens the installer.
A valid release signature does not remove this setting or the installation confirmation.
See [Android app signing](https://developer.android.com/studio/publish/app-signing) for the signing requirements.
Play Protect can also recommend a scan for an APK that Google does not recognize.
A valid release signature does not bypass this scan recommendation.
See [Google's warning guidance](https://developers.google.com/android/play-protect/warning-dev-guidance#app_scan_recommended) for the different installation warnings.

### Backend (src/backend)

Each Rust component owns its `.cargo/config.toml`; there is no repository-root target
directory default. From `src/backend`, its configuration puts output in root
`build/backend/target`. `CARGO_TARGET_DIR` or `--target-dir` can explicitly override it.
Cargo discovers configuration from the working directory, not `--manifest-path`, so
from the repository root use:

```bash
CARGO_TARGET_DIR=build/backend/target cargo build --release --manifest-path src/backend/Cargo.toml
```

```bash
cd src/backend

# Build (release/no-debug is the default project workflow)
cargo build --release

# Run development server
cargo run --release -- -c ../../playground/config.toml

# Linting & formatting
cargo fmt                  # Format code
cargo clippy --release --all-targets

# Testing (integration tests live in tests/backend/, not #[cfg(test)] modules)
cargo test --release --all-targets
cargo test --release auth

# Troubleshooting only: explicitly enable debug symbols in an isolated directory.
CARGO_PROFILE_DEV_DEBUG=2 CARGO_TARGET_DIR=../../build/backend/debug-target cargo build
CARGO_PROFILE_TEST_DEBUG=2 CARGO_TARGET_DIR=../../build/backend/debug-target cargo test auth
```

### LLM service (src/backend_llm)

From `src/backend_llm`, its `.cargo/config.toml` defaults output to root
`build/backend_llm/target`, separate from the API build cache:

```bash
cd src/backend_llm
cargo build --release --locked
cargo test --release --locked
```

Cargo discovers configuration from the working directory, not the manifest path. When
invoking this component from the repository root, explicitly select its output directory:

```bash
CARGO_TARGET_DIR=build/backend_llm/target cargo build --release --locked --manifest-path src/backend_llm/Cargo.toml
```

The LLM Docker build uses the same component-specific directory inside `/app/build/`.

From the repository root, run the OCR adapter and engine tests:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 tests/backend_llm/ocr/server.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/backend_llm/ocr/engine.py
```

### Shared Rust library (src/common)

Independent builds and tests from `src/common` use its `.cargo/config.toml` to put
output in root `build/common/target`:

```bash
cd src/common
cargo build --release
cargo test --release
```

From the repository root, explicitly select the component's directory because Cargo
does not discover nested configuration through `--manifest-path`:

```bash
CARGO_TARGET_DIR=build/common/target cargo test --release --manifest-path src/common/Cargo.toml
```

When common is compiled as an API or LLM dependency, it uses that service's target
directory instead; the dependency's own Cargo configuration is not loaded.

### Frontend (src/frontend)
```bash
cd src/frontend

pnpm dev                  # Dev server (Vite)
pnpm build                # Build changed frontend inputs and export dist/frontend/
pnpm lint                 # ESLint
pnpm test                 # Vitest; accepts a specific test-file filter
pnpm preview              # Preview production build
```

### Docker

Docker build files live in `docker/`; the canonical `docker-compose.yaml` and
`build_docker.sh` live at the git root. Builds use the git root as their context.

```bash
./build_docker.sh /path/to/keystore                                      # Build both images locally
./build_docker.sh publish github yzard /path/to/keystore                 # Build and publish to GHCR
./build_docker.sh publish docker zhuoyin /path/to/keystore               # Build and publish to Docker Hub
docker compose up                                      # Full stack
```

Every Dockerfile has a matching `<Dockerfile>.dockerignore` beside it. Docker resolves these
ignore files against the repository root, which remains the build context.

### Incremental targets and cleanup

Build scripts use a repository lock to serialize shared outputs, including nested Android builds.
The host scripts require Bash, `flock`, Docker Buildx and standard Unix tools.
The combined Docker script also uses Python 3 to make sure that reusable OCI images contain
every referenced blob. OCI is a portable container image format.

`build_docker.sh` tracks Android, frontend assets, API runtime, API service, AI runtime/models,
and LLM service as separate targets. Content fingerprints include source, lockfiles, scripts,
Dockerfiles, upstream artifacts and target platform. A matching fingerprint reuses an existing
valid output. Published targets also compare the registry digest. Failed targets never replace
a successful target receipt. A missing output rebuilds its target and affected dependents.

`docker/Dockerfile.runtime` owns API system packages and the RAW decoder.
`docker/Dockerfile.llm-runtime` owns AI dependencies, pinned model revisions and weight checksums.
Docker retains their reusable images under fingerprinted `momento-build-*` tags.
Source-only service builds export a temporary OCI context from those images and do not run
model download stages again. Docker shares identical layers between resulting service images.
The temporary image archive and OCI context stay under `build/` and disappear after success.
Existing validated `dist/docker/` exports are imported into Docker before their disk copies
are deleted. No exported Docker image directories or archives remain in `dist/` after success.
Initial setup creates reusable images once.
Changing a model/runtime input creates a new artifact. Set `RUNTIME_REFRESH` to a new explicit
key when refreshing otherwise unchanged mutable base-image references.

`docker/Dockerfile.frontend` exports the Web assets as a separate reusable target.
Host `pnpm build` and `pnpm --filter @momento/web build` use an isolated frontend workspace.
They export `dist/frontend/`, retain build reports and remove their own dependency installation,
store, compiler metadata and temporary workspace. Shared development dependencies remain intact.

After successful checks and artifact export, the outermost script clears the entire repository
`build/` directory. Nested entrypoints remove only their own workspace until the parent finishes.
A persistent lock outside `build/` serializes these entrypoints. Do not run manual compiler
commands concurrently with a build entrypoint that owns this directory.

Signing identities, fingerprints and reports remain under
`${XDG_STATE_HOME:-$HOME/.local/state}/momento-build/<checkout-path-hash>/`.
This location must stay outside `build/` and `dist/`. Existing recognized state and reports
are copied there before removing the old directory layout. Final APK/AAB and Web files remain
in `dist/`. Reusable model/runtime/SDK images remain in Docker, including after a Hub push.

Failure preserves the build workspace and returns a nonzero status. Cleanup failure also
returns a nonzero status. No script uses global Docker pruning or deletes another project's
caches. The internal RAW decoder builder runs within this lifecycle, where its SDK fixtures
remain available until decoder checks and image export finish.
