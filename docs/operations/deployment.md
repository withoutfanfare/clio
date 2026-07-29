# Deploying Clio

Clio uses Git as the source of deployable code. Atlas owns the shared runtime
data, while each Mac builds and installs native binaries from the same exact Git
commit.

This runbook covers the current private deployment. It does not describe a
public, automatically updating product installer.

## Current topology

```text
Mac AI client
  -> local `clio remote-mcp`
  -> SSH
  -> Atlas `/home/ubuntu/.local/bin/clio-mcp`
  -> Atlas SQLite database
```

Atlas has the following responsibilities:

- `/home/ubuntu/src/clio` is its checkout of this repository.
- `/home/ubuntu/.local/bin/clio-mcp` is the active MCP executable.
- `/home/ubuntu/.local/bin/libonnxruntime.so` is the default ONNX Runtime source
  copied into each managed release for local MiniLM embeddings.
- `/home/ubuntu/.local/share/clio/memory.db` is the shared system of record.
- The settings beside the database control server-side embeddings, capture,
  scoring and maintenance.

There is no long-running `clio-mcp` service and no systemd unit to deploy.
Every `clio remote-mcp` SSH connection starts one `clio-mcp` child process.
That process exits when its client disconnects.

Activating a new executable therefore does not interrupt an existing session.
An existing process keeps using the old executable inode until its AI client is
restarted. Restart every client after a release when all machines must use the
same version.

The optional macOS daemon is separate. It operates on a Mac's local database;
it does not provide or maintain the Atlas MCP connection.

## What belongs in Git

Keep these in this repository:

- Rust, Vue and Tauri source;
- `Cargo.lock`, npm lockfiles and build configuration;
- SQLite migrations;
- deployment and installation scripts;
- documentation and non-secret configuration examples.

Never commit:

- `memory.db`, `memory.db-wal` or `memory.db-shm`;
- `clio-settings.json`;
- SSH private keys or host credentials;
- API keys, signing certificates, notarisation credentials or environment
  files containing secrets;
- downloaded embedding model caches or ONNX Runtime libraries;
- database backups;
- compiled binaries, `.app` bundles or DMGs unless a formal release process
  later publishes them as release artefacts.

Atlas does not need a separate source repository. Its
`/home/ubuntu/src/clio` directory should be a clean checkout of this repository,
updated to an exact commit rather than edited directly. Runtime data remains
outside that checkout.

## Atlas infrastructure boundary

The application checkout, migrations and release scripts belong in this
repository. The database, settings, ONNX Runtime, model cache, SSH keys,
installed releases and backups are host state and must not be committed.

The remaining machine provisioning is not yet automated: the Ubuntu user,
system packages, Rust toolchain, SSH policy, firewall and ONNX Runtime are
configured directly on Atlas. The off-host backup destination is still a gap.
Do not create a second repository merely to hold a copy of those live files.
When Atlas must be rebuildable from scratch, a second server is added, or host
changes become regular, capture that provisioning in the existing private
infrastructure repository if there is one, otherwise create a small private
Ansible repository. Keep encrypted secrets in its secret store, not in Git.

## Release requirements

Before deploying:

1. Push the intended commit to the remote repository.
2. Record its full 40-character SHA:

   ```sh
   git rev-parse HEAD
   ```

3. Confirm CI or local verification is green for that commit.
4. Confirm Atlas has enough disk space for a source checkout, release binary
   and backup.
5. Ensure no database restore or other maintenance operation is in progress.

Deploy by SHA, not by a moving branch name. A branch may identify what should
be released, but the SHA records what was actually released. Both release
scripts also require that SHA to be reachable from an approved remote ref. The
default is `origin/develop`; set `CLIO_REMOTE_REF` deliberately when releasing
from a different remote branch.

The release scripts deliberately do not pull or change branches. Update each
checkout explicitly, then leave it clean and detached at the intended commit:

```sh
git fetch --prune origin
git checkout --detach <full-sha>
git status --short
git rev-parse HEAD
```

The last two commands must report no changes and the expected full SHA.
`origin/develop` must also contain that commit unless `CLIO_REMOTE_REF` was
explicitly changed for both the preflight and deployment.

## Atlas releases

Run `scripts/atlas-release.sh` on Atlas. The script:

- validates a clean `/home/ubuntu/src/clio` checkout at the requested commit;
- creates an online SQLite backup before activation;
- builds the Linux `clio` and `clio-mcp` binaries from the requested SHA;
- retains local MiniLM support and copies the configured ONNX Runtime into each
  versioned release;
- installs them in a versioned release directory;
- atomically activates both binaries through one `current` release link;
- supports status checks and binary rollback.

Deploy and rollback take a non-blocking lock at
`~/.local/lib/clio/deploy.lock`. A second operation fails immediately instead
of waiting or modifying a release concurrently.

The backup uses Python's SQLite online-backup API and runs `PRAGMA quick_check`
against the result. It retains a standalone database snapshot and a copy of the
settings file, when present, under `~/.local/lib/clio/backups`. Do not copy only
`memory.db` while Clio is running: recent committed data may still be in the
WAL file.

### Check, deploy, inspect and roll back

Use the full commit SHA in every command that accepts one:

```sh
cd /home/ubuntu/src/clio

# Confirm that the commit and host are ready without changing the active release
scripts/atlas-release.sh check <full-sha>

# Back up, build and atomically activate that commit
scripts/atlas-release.sh deploy <full-sha>

# Show stable-link targets, binary hashes and running MCP process versions
scripts/atlas-release.sh status

# Reactivate an existing versioned release; do not restore the database
scripts/atlas-release.sh rollback <full-sha>
```

The Atlas defaults are:

| Purpose | Default | Override |
|---|---|---|
| Git checkout | `/home/ubuntu/src/clio` | `CLIO_REPO_DIR` |
| SQLite database | `~/.local/share/clio/memory.db` | `CLIO_DB_PATH` |
| Settings file | beside the database | `CLIO_SETTINGS_PATH` |
| Versioned releases | `~/.local/lib/clio` | `CLIO_INSTALL_ROOT` |
| Approved Git ref | `origin/develop` | `CLIO_REMOTE_REF` |
| ONNX Runtime | `~/.local/bin/libonnxruntime.so` | `CLIO_ORT_DYLIB` |

Set an override in the environment for both the check and the corresponding
deploy. Do not use an override to place the database or settings inside the Git
checkout.

The stable links are `~/.local/bin/clio` and `~/.local/bin/clio-mcp`. They
normally point through `~/.local/lib/clio/current`, while releases are retained
under `~/.local/lib/clio/releases/<full-sha>/bin`. Each release also contains
the ONNX Runtime needed by its dynamically linked local-embedding build. Deploy
and rollback replace the single `current` symlink atomically, so the CLI and
MCP server cannot be activated from different releases. The first managed
release also preserves any previous binaries and ONNX Runtime in a timestamped
legacy directory.

`status` reports the resolved binary paths and SHA-256 hashes. It also counts
running `clio-mcp` processes and identifies processes still using an older or
deleted executable.

### Migration gate

After taking the online backup, deploy copies that snapshot and its settings to
a disposable directory and lets the candidate CLI apply its migrations there.
It also runs keyword-recall and semantic-search smoke checks, exercising the
live embedding configuration, repository SQLite maths configuration and
dynamically loaded ONNX Runtime. The live database is touched only after the
probe succeeds, and its resulting migration set must match the probe.

If the probe finds pending migrations, deploy records the candidate SHA and
temporarily gates the stable MCP entry point. Every MCP process holds a shared
database maintenance lease before opening SQLite; deploy must acquire the
exclusive lease before migration. This closes the startup race between gating
the entry point and detecting an old process. Existing processes are not
killed. If any remain, deploy stops before changing the live database or active
release. The normal response is to disconnect the clients, deploy, then
reconnect them.

Only after confirming that the old MCP binary is compatible with the proposed
schema may an operator deliberately allow a live migration:

```sh
CLIO_ALLOW_LIVE_MIGRATION=1 \
  scripts/atlas-release.sh deploy <full-sha>
```

With the override, existing processes keep running but new sessions remain
gated until the migration and release activation finish. Do not make this a
persistent environment setting. The override applies to a specific, reviewed
migration; it is not a general deployment convenience. If live migration may
have started but activation fails, the script deliberately leaves new MCP
sessions gated so an incompatible old binary cannot reopen the database.
Re-running the exact candidate SHA recognises that gate and switches `current`
to the candidate before admitting new sessions. A different SHA is rejected;
inspect the live migration state and roll the recorded candidate forward before
reconnecting clients.

After activation, restart the MCP integration in every AI client. A symlink
change affects new processes only.

### Atlas rollback boundary

Binary rollback atomically changes the `current` release link to an earlier
version. It does not reverse:

- SQLite migrations;
- settings changes;
- memories written after deployment;
- embedding model or capture-provider changes.

Clio migrations are forward-only. If a release has changed the schema, only
roll back to a binary that remains compatible with the migrated database.
The rollback command fails closed while the MCP entry point is migration-gated;
complete the roll-forward deployment before considering a later binary
rollback.

A database restore is a separate recovery operation. Stop or disconnect every
writer first, validate the selected backup, preserve the failed database, then
restore it during a controlled outage. Restart all clients afterwards so no
old process retains an open connection to the replaced database.

## Installing on a Mac

Run `scripts/macos-install.sh` on each Mac. Building on the destination creates
the correct native binary for both Apple Silicon (`arm64`) and Intel
(`x86_64`) Macs and avoids moving incompatible binaries between machines.

The normal shared-memory client requires only `clio`. The default install
builds that lightweight bridge without local embedding features and installs it
at `$CARGO_HOME/bin/clio`, or `~/.cargo/bin/clio` when `CARGO_HOME` is unset. It
does not install a local `clio-mcp`; that binary is useful only for a
local-database configuration.

From a checkout of this repository:

```sh
# Validate the host and requested commit without installing
scripts/macos-install.sh check <full-sha>

# Install the native `clio` SSH bridge
scripts/macos-install.sh install <full-sha>

# Also install the local daemon and reload an existing LaunchAgent
scripts/macos-install.sh install <full-sha> --with-daemon

# Also build, ad-hoc sign and install the desktop app
scripts/macos-install.sh install <full-sha> --with-app

# Install both optional components
scripts/macos-install.sh install <full-sha> --with-daemon --with-app
```

Use the same full SHA deployed to Atlas unless deliberately testing a
compatible client change.

The source checkout is the repository containing the script. Optional paths
can be changed with:

| Purpose | Default | Override |
|---|---|---|
| Git checkout | repository containing the script | `CLIO_REPO_DIR` |
| CLI and daemon directory | `$CARGO_HOME/bin`, or `~/.cargo/bin` when unset | `CLIO_BIN_DIR` |
| Desktop app | `/Applications/Clio.app` | `CLIO_APP_PATH` |
| Daemon LaunchAgent | `~/Library/LaunchAgents/com.clio.daemon.plist` | `CLIO_PLIST_PATH` |
| Approved Git ref | `origin/develop` | `CLIO_REMOTE_REF` |

Before replacement, existing binaries and apps are retained under
`~/Library/Application Support/Clio Deploy Backups/<full-sha>`.

### Optional daemon

Install the daemon only on a Mac that needs local inbox watching, local
auto-linking or local maintenance. A daemon always uses local storage; keep it
disabled on a Mac whose normal tooling should use Atlas.

With `--with-daemon`, the installer reads an existing LaunchAgent to preserve
its executable and database paths. It uses `launchctl bootout` before
replacement, then `launchctl bootstrap` and Clio health checks. If the new
daemon fails activation or health checks, the installer restores the previous
binary and starts it again. If restoration also fails, it reports the retained
backup path for manual recovery. An interruption or termination signal during
replacement triggers the same restoration path.

If the plist does not exist, the binary is installed but is not started;
configure it with:

```sh
clio daemon install
```

For a manual reload of an existing plist, use:

```sh
launchctl bootout "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.clio.daemon.plist" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.clio.daemon.plist"
clio daemon status
clio daemon doctor
```

For local development, `./build.sh daemon` uses the same
`launchctl bootout`/`bootstrap` sequence when a plist exists. It now exits with
an error if `launchctl` does not report a daemon PID after restart.

Do not run a local daemon merely because a Mac uses Atlas.

### Optional desktop app

Build the Tauri app only on Macs where the visual interface is wanted.
Quit Clio before using `--with-app`. The installer creates both an `.app` and a
DMG, verifies the signature, installs the app at `/Applications/Clio.app` by
default, and retains the DMG with the deployment backups. It runs locked
`npm ci` installation from the committed UI lockfile before building. If app
installation fails or is interrupted after the previous app is moved aside,
the installer restores that previous version.

The current lockfile resolves the private `@stuntrocket/ui` package through the
developer Mac's local Verdaccio registry. Start that existing registry before a
`--with-app` build and stop it afterwards. A clean Mac or CI runner cannot yet
reproduce the app build without access to the same package archive; this is
tracked as `CLIO-REL-002` in the operational roadmap. CLI-only installs do not
need the npm registry.

For personal Macs, ad-hoc signing identifies the bundle consistently without
requiring an Apple Developer certificate:

```sh
cd crates/clio-tauri
APPLE_SIGNING_IDENTITY=- cargo tauri build --ci --bundles app,dmg
```

The repository and Mac installer use `-` as the default signing identity.
`APPLE_SIGNING_IDENTITY` can select a different identity. An ad-hoc signature
is not suitable for public distribution and does not satisfy Gatekeeper's
Developer ID assessment. A public release needs a Developer ID Application
certificate, hardened runtime, notarisation and stapling. Signing certificates
and notarisation credentials must stay outside Git.

Configure the shared route before opening the app:

```sh
clio settings use-remote \
  --host atlas \
  --remote-db-path /home/ubuntu/.local/share/clio/memory.db \
  --mcp-binary /home/ubuntu/.local/bin/clio-mcp \
  --cli-binary /home/ubuntu/.local/bin/clio \
  --bridge-command /absolute/local/path/to/clio
```

Finder-launched Tauri reads this persisted route. `CLIO_REMOTE_HOST`,
`CLIO_REMOTE_DB_PATH`, `CLIO_REMOTE_BINARY` and optionally
`CLIO_REMOTE_COMMAND` override it for development. A broken remote
configuration is reported as disconnected and never falls back to local
storage.

## macOS rollback boundary

Re-run the installer for a previously verified SHA. This rebuilds native
binaries from that commit.

- Restart every AI client after replacing `clio`.
- If the optional daemon is installed, reload its LaunchAgent and check its
  status.
- If the optional app is installed, quit it fully before replacing the bundle.
- A Mac rollback does not roll back Atlas data or settings.

## Acceptance checklist

Complete this after an Atlas or client release:

- [ ] The Git commit exists on the remote and its full SHA was recorded.
- [ ] The SHA is reachable from the selected `CLIO_REMOTE_REF`.
- [ ] Atlas reports a clean source checkout at that SHA.
- [ ] Atlas created and validated an online backup before activation.
- [ ] The migration probe passed; any live-migration override has a recorded
      compatibility review.
- [ ] `atlas-release.sh status` reports the expected release paths and hashes.
- [ ] No `clio-mcp` systemd service was introduced.
- [ ] Every AI client was restarted after Atlas or local bridge activation.
- [ ] `macos-install.sh check` reports the intended native architecture and SHA.
- [ ] A remote keyword recall succeeds from each Mac.
- [ ] A memory written from one Mac can be recalled from another.
- [ ] Project namespace detection is correct on both machines.
- [ ] Direct CLI commands and session-start hooks use Atlas on shared clients.
- [ ] Generated MCP entries point to the Atlas SSH bridge.
- [ ] A Finder-launched Tauri app reports the Atlas backend as connected.
- [ ] Semantic search and capture are checked only if their server-side
      providers are configured.
- [ ] The optional daemon, if installed, reports healthy after reload.
- [ ] The optional app, if installed, passes `codesign --verify --deep --strict`.
- [ ] No background build, development server or unexpected service remains.

## Backup gap

The deployment backup currently remains on Atlas. It protects against a bad
release or accidental local change, but not against loss of the Atlas host,
volume or account.

Add an encrypted, access-controlled off-host backup before treating Atlas as
fully disaster-recoverable. The future job should copy verified standalone
SQLite snapshots, retain multiple generations, alert on failure and exercise a
restore periodically. Do not replicate the live database and WAL files with a
generic file synchroniser.

## Next automation threshold

The checked-in scripts are the appropriate current installer: they build each
Mac for its native architecture and make Atlas releases auditable by Git SHA.
Move to CI-produced release artefacts when releases become frequent or more
than a few Macs need updates. That release should publish a checksummed Linux
`x86_64` bundle containing Clio and its compatible ONNX Runtime, plus macOS
`arm64` and macOS `x86_64` artefacts. It should sign and notarise the Mac app,
and keep Atlas database migration as an explicit operator step rather than an
unattended auto-update. Until then, native builds avoid a signing and
cross-compilation pipeline without weakening the database gate.

## Deferred work

The canonical register for unfinished deployment and infrastructure work is the
[operational roadmap](roadmap.md). Review it after every Atlas deployment and
on the first working day of each month. Keep status and completion evidence in
that one file so the runbook and roadmap cannot drift apart.
