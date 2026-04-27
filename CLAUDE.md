# shihaisha -- Unified Service Management

> **★★★ CSE / Knowable Construction.** This repo operates under **Constructive Substrate Engineering** — canonical specification at [`pleme-io/theory/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md`](https://github.com/pleme-io/theory/blob/main/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md). The Compounding Directive (operational rules: solve once, load-bearing fixes only, idiom-first, models stay current, direction beats velocity) is in the org-level pleme-io/CLAUDE.md ★★★ section. Read both before non-trivial changes.


Rust workspace providing a single CLI to manage services across systemd, launchd,
supervisord, and a pure-Rust native process supervisor. Write one YAML spec,
deploy to any platform.

## Architecture

```
shihaisha-core      types + traits (ServiceSpec, InitBackend, HealthChecker, ConfigEmitter, ConfigParser)
     |              merge (NixOS-style), diff (structural JSON), graph (Kahn's toposort)
shihaisha-engine    backend implementations + BackendRegistry + health + translator
     |
shihaisha-cli       clap CLI: install/uninstall/start/stop/restart/reload/status/logs/
                    enable/disable/list/daemon-reload/check/backends/daemon
```

### Crate Breakdown

| Crate | Tests | Key Exports |
|-------|-------|-------------|
| `shihaisha-core` | 133 | `ServiceSpec`, `ServiceStatus`, `ServiceGroup`, `ServicePhase`, `InitBackend`, `HealthChecker`, `ConfigEmitter`, `ConfigParser`, `Merge`, `Change`, `resolve_order`, `validate_references`, `Error`, `MockBackend` (behind `test-support` feature) |
| `shihaisha-engine` | 69 | `BackendRegistry`, `NativeBackend`, `LaunchdBackend`, `SystemdBackend`, `SupervisordBackend`, `DefaultHealthChecker` |
| `shihaisha-cli` | 16 | `shihaisha` binary, `execute()` (testable core) |

Plus 3 doc-tests. **Total: 221 tests (218 unit + 3 doc-tests).**

### Backend Feature Flags

The engine crate uses feature flags to conditionally compile backends:

- `native` (default) -- pure-Rust process supervisor, always available
- `launchd` -- macOS launchctl/plist backend
- `systemd` -- Linux systemd/journalctl backend
- `supervisord` -- supervisord INI backend

The CLI enables `native` + `launchd` on macOS, `native` + `systemd` on Linux.

## InitBackend Trait

The core abstraction. Every backend implements this (12 async methods + 2 sync):

```rust
#[async_trait]
pub trait InitBackend: Send + Sync {
    async fn install(&self, spec: &ServiceSpec) -> Result<()>;
    async fn uninstall(&self, name: &str) -> Result<()>;
    async fn start(&self, name: &str) -> Result<()>;
    async fn stop(&self, name: &str) -> Result<()>;
    async fn restart(&self, name: &str) -> Result<()>;
    async fn reload(&self, name: &str) -> Result<()>;
    async fn status(&self, name: &str) -> Result<ServiceStatus>;
    async fn logs(&self, name: &str, lines: u32) -> Result<Vec<String>>;
    async fn enable(&self, name: &str) -> Result<()>;
    async fn disable(&self, name: &str) -> Result<()>;
    async fn list(&self) -> Result<Vec<ServiceStatus>>;
    async fn daemon_reload(&self) -> Result<()>;
    fn available(&self) -> bool;
    fn name(&self) -> &str;
}
```

### Config Traits (split)

```rust
/// Translate ServiceSpec into backend-native config.
pub trait ConfigEmitter: Send + Sync {
    fn emit(&self, spec: &ServiceSpec) -> Result<String>;
    fn extension(&self) -> &str;  // "service", "plist", "conf"
    fn name(&self) -> &str;
}

/// Parse backend-native config back into ServiceSpec (optional).
pub trait ConfigParser: Send + Sync {
    fn parse(&self, content: &str) -> Result<ServiceSpec>;
    fn name(&self) -> &str;
}
```

### HealthChecker Trait

```rust
pub trait HealthChecker: Send + Sync {
    async fn check(&self, spec: &HealthCheckSpec) -> Result<HealthCheckResult>;
    fn name(&self) -> &str;
}
```

Returns `HealthCheckResult { healthy: bool, latency: Duration, message: Option<String> }`.

### Backend Implementations

| Backend | Platform | Config Format | Service Manager | Init Integration |
|---------|----------|---------------|-----------------|------------------|
| `NativeBackend` | Any | YAML files in `~/.config/shihaisha/services/` | Direct process spawn via tokio | None (always available) |
| `LaunchdBackend` | macOS | XML plist in `~/Library/LaunchAgents/` | `launchctl bootstrap/bootout/kickstart` | Full launchd integration |
| `SystemdBackend` | Linux | Unit files in `~/.config/systemd/user/` | `systemctl --user` | Full systemd integration |
| `SupervisordBackend` | Any | INI sections in `~/.config/shihaisha/supervisord/` | `supervisorctl` | supervisord integration |

## Key Types

### ServiceSpec (Canonical YAML)

```yaml
name: my-service
description: Example service
command: /usr/bin/my-service
args: ["--port", "8080"]
service_type: simple          # simple|oneshot|notify|forking|timer|socket
working_directory: /var/lib/my-service
user: nobody
group: nogroup
environment:
  RUST_LOG: info
  PORT: "8080"
critical: true                # failure triggers shutdown of all dependents
restart:
  strategy: on-failure        # always|on-failure|on-success|never
  delay_secs: 5
  max_retries: 3
  reset_after_secs: 300
depends_on:
  after: [database]
  before: [proxy]
  requires: [database]
  wants: [cache]
  conflicts: [old-service]
  conditions:
    database: service_healthy  # service_started|service_healthy|service_completed_successfully
  stop_before: [cache]         # shutdown ordering
  stop_after: [database]
liveness:                      # is the process alive? (restarts on failure)
  type: http
  endpoint: http://localhost:8080/health
  interval_secs: 30
  timeout_secs: 5
  max_failures: 3
readiness:                     # ready to serve? (dependents wait for this)
  type: tcp
  address: 127.0.0.1:5432
startup:                       # finished initializing? (suppresses liveness during startup)
  type: command
  command: /usr/bin/check-init
sockets:
  - listen: "127.0.0.1:8080"
    socket_type: stream
resources:
  memory_max: "512M"
  cpu_quota: "100%"
  tasks_max: 256
logging:
  stdout: journal             # journal|file|null|inherit
  stderr: journal
timeout_start_sec: 90
timeout_stop_sec: 90
overrides:
  systemd:
    Service:
      LimitNOFILE: "65536"
  launchd:
    LowPriorityIO: true
```

### DependencyCondition

Controls when a dependency is considered satisfied:

| Variant | Meaning |
|---------|---------|
| `ServiceStarted` (default) | Process has started |
| `ServiceHealthy` | Readiness probe is passing |
| `ServiceCompletedSuccessfully` | Process exited with code 0 (for oneshot) |

### ServiceGroup (Erlang OTP-style supervision)

```yaml
name: db-group
members: [postgres, pgbouncer]
strategy: one_for_all         # one_for_one|one_for_all|rest_for_one
max_intensity: 5              # max restarts within period
intensity_period_secs: 60
```

### ServicePhase (Kubernetes-style summary)

`Pending | Running | Succeeded | Failed | Unknown`

### Validated Newtypes

| Type | Range | Purpose |
|------|-------|---------|
| `MemorySize` | Parses `"512M"`, `"2G"` etc. | Memory limits |
| `Weight` | 1-10000 | CPU/IO weight |
| `NiceValue` | -20..19 | Process priority |

### HealthCheckResult

```rust
pub struct HealthCheckResult {
    pub healthy: bool,
    pub latency: Duration,
    pub message: Option<String>,
}
```

## Error Handling

Structured `Error` enum with `PartialEq` and `is_retryable()`:

| Variant | Retryable |
|---------|-----------|
| `BackendError { backend, operation, detail }` | Yes |
| `TimeoutError { service, timeout_secs }` | Yes |
| `Io(std::io::Error)` | Yes |
| `ServiceNotFound(String)` | No |
| `ServiceAlreadyExists(String)` | No |
| `BackendUnavailable(String)` | No |
| `ConfigError(String)` | No |
| `DependencyError(String)` | No |
| `HealthCheckFailed(String)` | No |
| `Serialization(String)` | No |

## Merge (NixOS-style)

Full NixOS module system merge semantics via `Merge` trait:

- **Scalars**: overlay wins unconditionally
- **`Option<T>`**: overlay `Some` wins, `None` falls through
- **`Vec<T>`**: concatenate (deduplicated by value)
- **`HashMap<K,V>`**: recursive merge by key, overlay wins on conflict

Enables profile/override stacking like NixOS `lib.mkMerge`.

## Diff (structural JSON)

`diff(old, new) -> Vec<Change>` compares two `ServiceSpec` values via JSON
serialization. Produces `Added`, `Removed`, `Modified` variants with
dot-delimited field paths. `Change` implements `Display` (`+ path: value`,
`- path: value`, `~ path: old -> new`).

## Graph (dependency resolution)

- `resolve_order(specs) -> Vec<String>` -- Kahn's algorithm topological sort
  with deterministic tie-breaking (sorted queue). Reports cycles.
- `validate_references(specs)` -- checks all dependency references
  (`after`, `before`, `requires`, `wants`, `conflicts`, `stop_before`, `stop_after`)
  point to services in the set.

## MockBackend (test-support feature)

`shihaisha-core` provides `MockBackend` behind the `test-support` feature flag.
Records all calls as `Call` variants for assertion in tests. Used by CLI tests
to verify command dispatch without touching real init systems.

## Backend Auto-Detection

`BackendRegistry::detect()` probes the system in priority order:

1. **launchd** (macOS) -- checks for `/bin/launchctl` via `launchctl version`
2. **systemd** (Linux) -- checks for `systemctl --version`
3. **supervisord** (any) -- checks for `supervisorctl version`
4. **native** (always) -- fallback pure-Rust process supervisor

The highest-priority detected backend becomes the default. Users can override
with `--backend <name>`.

## CLI Commands

| Command | Arguments | Description |
|---------|-----------|-------------|
| `install` | `<spec>` | Install service from YAML file |
| `uninstall` | `<name>` | Remove service definition |
| `start` | `<name>` | Start a service |
| `stop` | `<name>` | Stop a service |
| `restart` | `<name>` | Restart a service |
| `reload` | `<name>` | Reload service configuration (SIGHUP) |
| `status` | `[name]` | Show service status (omit name for all) |
| `logs` | `<name> [-n lines]` | Show service logs |
| `enable` | `<name>` | Enable start on boot |
| `disable` | `<name>` | Disable start on boot |
| `list` | | List all managed services |
| `daemon-reload` | | Reload init system daemon config |
| `check` | `<path>` | Validate spec file or directory (runs validate + dependency check) |
| `backends` | | Show available backends |
| `daemon` | | Run as config watcher (planned) |

The `execute()` function is extracted from `run()` for testability: it takes
a resolved `&dyn InitBackend` instead of doing registry detection.

## Build Commands

```bash
# Check compilation (all features)
cargo check --workspace --features shihaisha-engine/launchd,shihaisha-engine/supervisord

# Run tests (221 tests)
cargo test --workspace --features shihaisha-engine/launchd,shihaisha-engine/supervisord

# Build release binary
cargo build --release

# Nix build (via substrate workspace builder)
nix build

# Run CLI
cargo run -- --help
cargo run -- backends
cargo run -- list
```

## Nix Integration

### Flake

Uses `substrate/lib/rust-workspace-release-flake.nix` (workspace builder pattern):
- `toolName = "shihaisha"` (binary name)
- `packageName = "shihaisha-cli"` (workspace member)
- Outputs: `packages`, `overlays.default`, `homeManagerModules.default`, `devShells`

### Home-Manager Module

Namespace: `blackmatter.components.shihaisha`

```nix
blackmatter.components.shihaisha = {
  enable = true;
  backend = "auto";          # auto|systemd|launchd|native|supervisord
  service_dir = "~/.config/shihaisha/services";
  log_level = "info";
  daemon.enable = false;      # run as background config watcher
  extraSettings = {};         # raw YAML escape hatch
};
```

The module:
- Installs the package
- Generates `~/.config/shihaisha/shihaisha.yaml` from typed Nix options
- Optionally runs a launchd agent (macOS) or systemd user service (Linux) for daemon mode

## Adding a New Backend

1. Create `shihaisha-engine/src/<backend>.rs`
2. Implement `InitBackend` trait (12 async + 2 sync methods)
3. Optionally implement `ConfigEmitter` and/or `ConfigParser`
4. Add feature flag in `shihaisha-engine/Cargo.toml`
5. Register in `BackendRegistry::detect()` (in `registry.rs`)
6. Add tests

## Related Repos

| Repo | Relationship |
|------|-------------|
| `substrate` | Nix build patterns (`rust-workspace-release-flake.nix`, `hm-service-helpers.nix`) |
| `shikumi` | Config discovery and hot-reload (planned for daemon mode) |
| `tsunagu` | Daemon lifecycle patterns (PID, sockets, health) |
| `blackmatter` | HM module aggregator (consumes `homeManagerModules.default`) |

## Conventions

- Edition 2024, Rust 1.89.0+, MIT license
- Clippy pedantic, `codegen-units=1`, `lto=true`, `opt-level="z"`, `strip=true`
- No FFI/uniffi dependencies (pure Rust only, compatible with crate2nix)
- All platform calls go through `tokio::process::Command` or feature-gated code
