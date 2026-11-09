# Container startup permissions

Both entrypoints use `docker/entrypoint_common.sh`. `PUID` and `PGID` select the
runtime identity (default 1000:1000); `UMASK` controls new file permissions
(default 022). These are container entrypoint settings, not application config.

Startup initializes an empty `/data` volume and creates missing application
directories with the selected ownership. Existing non-empty `/data` and existing
directories are not chowned or chmodded. Startup checks read/write/search access
to the fixed required directories as the runtime user, without walking media,
Journal, or queue contents. Existing config files must be readable; existing API
SQLite database/WAL/SHM files must also be writable. A failed check exits with the
path, UID, GID, and missing access requirement. Deeper file failures are reported
when the application accesses those files; startup does not audit the whole tree.

The LLM bundled Triton cache is copied as the runtime user, without replacing
existing cache files. Newly generated LLM configuration is mode 600; existing
configuration contents and permissions are preserved. Image-build verification
caches are removed during the image build, not at service startup.

For an existing populated volume, provision its access permissions before
starting. Changing PUID/PGID does not migrate data ownership. Stop the service and
explicitly repair the affected host path or grant the runtime identity access
using host ACLs/groups. For NAS this means inspecting the exact reported path
below `/data/docker/data/momento`; do not recursively change `/data/docker` or
other services. Normal restarts must not run a recursive ownership repair.

Container regression tests (use the existing local service images with changed
entrypoints mounted read-only; isolated disposable test volumes, no NAS data):

```sh
sh tests/docker/entrypoint.sh
sh tests/docker/entrypoint_llm.sh
```
