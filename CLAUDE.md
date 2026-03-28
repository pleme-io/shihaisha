# shihaisha -- Unified Service Management

Rust workspace providing a single CLI to manage services across systemd, launchd,
and a pure-Rust native process supervisor. Write one YAML spec, deploy to any
platform.

## Architecture

```
shihaisha-core      types + traits (ServiceSpec, InitBackend, HealthChecker, ConfigTranslator)
     |
shihaisha-engine    backend implementations + BackendRegistry + health + translator
     |
shihaisha-cli       clap CLI: install/start/stop/restart/status/logs/list/backends/daemon
```

### Crate Breakdown

| Crate | Purpose | Key Exports |
|-------|---------|-------------|
| `shihaisha-core` | Platform-independent types and trait definitions | `ServiceSpec`, `ServiceStatus`, `InitBackend`, `HealthChecker`, `ConfigTranslator`, `Error` |
| `shihaisha-engine` | Backend implementations and auto-detection | `BackendRegistry`, `NativeBackend`, `LaunchdBackend`, `SystemdBackend`, `DefaultHealthChecker` |
| `shihaisha-cli` | Binary entry point | `shihaisha` binary |

### Backend Feature Flags

The engine crate uses feature flags to conditionally compile backends:

- `native` (default) -- pure-Rust process supervisor, always available
- `launchd` -- macOS launchctl/plist backend
- `systemd` -- Linux systemd/journalctl backend

The CLI enables `native` + `launchd` on macOS, `native` + `systemd` on Linux.

## InitBackend Trait

The core abstraction. Every backend implements this:

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

### Backend Implementations

| Backend | Platform | Config Format | Service Manager | Init Integration |
|---------|----------|---------------|-----------------|------------------|
| `NativeBackend` | Any | YAML files in `~/.config/shihaisha/services/` | Direct process spawn via tokio | None (always available) |
| `LaunchdBackend` | macOS | XML plist in `~/Library/LaunchAgents/` | `launchctl bootstrap/bootout/kickstart` | Full launchd integration |
| `SystemdBackend` | Linux | Unit files in `~/.config/systemd/user/` | `systemctl --user` | Full systemd integration |

## Service Spec Format (Canonical YAML)

```yaml
name: my-service
description: Example service
command: /usr/bin/my-service
args:
  - --port
  - "8080"
service_type: simple          # simple|oneshot|notify|forking|timer|socket
working_directory: /var/lib/my-service
user: nobody
group: nogroup
environment:
  RUST_LOG: info
  PORT: "8080"
restart:
  strategy: on-failure        # always|on-failure|on-success|never
  delay_secs: 5
  max_retries: 3
  reset_after_secs: 300
depends_on:
  after: [database]
  requires: [database]
health:
  type: http                  # http|tcp|command|file
  endpoint: http://localhost:8080/health
  interval_secs: 30
  timeout_secs: 5
  max_failures: 3
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

## Backend Auto-Detection

`BackendRegistry::detect()` probes the system in priority order:

1. **launchd** (macOS) -- checks for `/bin/launchctl` via `launchctl version`
2. **systemd** (Linux) -- checks for `systemctl --version`
3. **native** (always) -- fallback pure-Rust process supervisor

The highest-priority detected backend becomes the default. Users can override
with `--backend <name>`.

## Health Check System

Four check types, all executed by `DefaultHealthChecker`:

| Type | Mechanism |
|------|-----------|
| `http` | TCP connect to host:port (lightweight, no reqwest dep) |
| `tcp` | Raw TCP connect with timeout |
| `command` | Execute a command, check exit code |
| `file` | Check file existence |

Health checks run at configurable intervals with failure thresholds.

## Build Commands

```bash
# Check compilation (all features)
cargo check --workspace --features shihaisha-engine/launchd

# Run tests (76 tests)
cargo test --workspace --features shihaisha-engine/launchd

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
  backend = "auto";          # auto|systemd|launchd|native
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
2. Implement `InitBackend` trait
3. Add feature flag in `shihaisha-engine/Cargo.toml`
4. Register in `BackendRegistry::detect()` (in `registry.rs`)
5. Optionally implement `ConfigTranslator` in `translator/<backend>_translator.rs`
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
