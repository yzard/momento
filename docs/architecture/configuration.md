# Configuration and deployment contracts

Read when changing configuration, runtime updates, credentials, paths, logging, or container initialization.

## Configuration

Both binaries (`momento-api`, `llm-service`) require `-c|--config PATH`. A missing or malformed
config is a hard startup failure — it never falls back to defaults, because that would silently
start the server against the wrong data directory. Momento resolves the exact
`${LLM_SERVICE_ADDRESS}` placeholder when it appears in `llm.server_address`; the environment value is
required and must be non-empty in that case. Momento also accepts exact runtime overrides from
`RESET_ADMIN_PASSWORD`, `SECRET_KEY`, and `LLM_SERVICE_API_KEY`; llm-service accepts
`LLM_SERVICE_API_KEY`. No other config
interpolation or application environment variable is supported.

Both binaries also support `--init-config`, which writes their source-owned commented operational
template and exits. The Docker entrypoints invoke it only when the expected config file is absent:
`/data/config.toml` for Momento and `/data/config_llm.toml` for llm-service. Each service owns all
of its fallback and operational-template values in its `config/defaults.rs`; its commented TOML
source contains named placeholders for shared values. The playground uses the same entrypoints
to create its local configuration files. Tests use the generated templates with test credentials.
During normal startup, Momento only replaces its configuration file when it consumes the one-shot
administrator password reset. It replaces the file atomically.

Momento constructs one `ConfigManager` from the exact CLI config path and the fully validated
configuration. Runtime code reads immutable snapshots from that manager instead of retaining its
own `Config` copy. Administrator configuration endpoints must accept typed fields, validate the
prospective complete configuration, preserve unrelated TOML values and comments with `toml_edit`,
atomically replace the same config path, then publish the new snapshot. A failed validation or
write leaves both the file and live snapshot unchanged. The AI schedule endpoint currently updates
the seven `[llm].*_cron` fields this way; active cron loops subscribe to the manager and reschedule
immediately without restarting Momento. Other configuration fields remain startup-only until their
runtime owners explicitly subscribe to changes.

Momento has no `[admin]` configuration section. An empty database creates `admin` / `admin` with a
required password change. Setting `[server].reset_admin_password = true` is a one-start recovery
request: startup atomically writes it back to `false`, keeps the stored password unchanged, and
accepts temporary `admin` / `admin` API authentication only in that process for the existing
administrator. A successful forced password change replaces the stored hash; a restart before the
change removes the temporary override and restores normal authentication with the unchanged stored
username and password. `RESET_ADMIN_PASSWORD=true` applies the same request from the environment;
operators must remove that override after startup so it is not requested again on every restart.

Momento filesystem locations derive from `server.data_dir`:

```toml
[server]
data_dir = "/data"          # database.sqlite, originals/, thumbnails/, previews/, imports/, webdav/, ...
static_dir = "/app/static"  # built frontend served as a fallback
```

`main` calls `constants::init_paths(&config.server.data_dir)` once after parsing the
config; everything else reads `constants::paths()`. Never hardcode a path under the data
directory and never add a new `std::env::var` call. The config loaders own all supported environment
access: `LLM_SERVICE_ADDRESS`, `RESET_ADMIN_PASSWORD`, `SECRET_KEY`, and `LLM_SERVICE_API_KEY` for
Momento, and `LLM_SERVICE_API_KEY` for llm-service. Add a config field rather than another
environment setting.

Log paths are not configurable. Each service writes plain daily rotated files below
`server.data_dir/logs/` (`momento-api.YYYY-MM-DD.log` or `llm-service.YYYY-MM-DD.log`). Log events
contain the timestamp, level, message, and structured fields; they do not repeat the service name
or process ID. File logs never contain ANSI escapes. Console logs keep the timestamp dim regardless
of severity, then color the level, message, and structured fields as one span: DEBUG/INFO white,
WARN yellow, and ERROR/fatal paths red.

llm-service configures only `server.data_dir`; its durable queue and runtime cache are fixed at
`server.data_dir/llm/queue/` and `server.data_dir/llm/cache/`, while its logs remain in
`server.data_dir/logs/`. The `llm/` subtree must be durable. Queue jobs are accepted only with a
non-empty hexadecimal Momento job ID and a manifest sent before its binary input frames.

Runtime executables, scripts, model paths, model versions, loopback URLs, CUDA device selection,
and embedding dimensions are owned by `RuntimeCatalog` and the llm-service image, not TOML.
Service configuration retains only enablement, startup/request timeouts, model concurrency, OCR
token limits, and task-specific thresholds. Local runtime requests contain job/input descriptors,
never media bytes or caller-supplied paths.

`llm.server_address` contains only the llm-service host and port. Momento owns the WebSocket scheme
and `/api/v1/llm/connect` path as implementation details; `0.0.0.0` is only a server bind address
and is never a client destination. llm-service has no Momento address. Its
top-level `[scheduler]` section owns inference settings and every result-delivery setting, with
result-delivery fields prefixed by `result_delivery_`. The Compose deployment mounts one shared
data root into both containers; llm-service only uses its config, `logs/`, and `llm/` subtree and
does not read Momento originals, previews, or thumbnails.

Metadata, LLM submission, and LLM result workers are event-driven. Each starts by draining its
durable queue, then waits on a versioned no-lost-wakeup signal. New work wakes each
worker immediately. Submission and result preparation both use the global Momento application
worker pool; neither section has a concurrency field. AI-result persistence remains a single
logical writer and never creates concurrent database writers.

The Dockerfiles and entrypoints use `PUID`/`PGID`/`UMASK`/`TZ`; Compose additionally supplies
`LLM_SERVICE_ADDRESS`, `RESET_ADMIN_PASSWORD`, `SECRET_KEY`, and the shared
`LLM_SERVICE_API_KEY`. They prepare
filesystem ownership and invoke each binary with its generated config path. `RUST_LOG` and `RUST_BACKTRACE` are ecosystem-standard runtime knobs read
by `tracing-subscriber`, not application config.
