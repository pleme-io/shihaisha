# shihaisha

Unified service management CLI for Linux, macOS, and any platform with supervisord.

Write one YAML service spec, deploy to systemd, launchd, supervisord, or a built-in
pure-Rust process supervisor. shihaisha translates your spec into the native config
format, manages lifecycle operations, and provides dependency-aware startup ordering
with health checks -- so you stop writing platform-specific unit files by hand.

## Quick Start

```bash
# Build
cargo build --release

# Run all tests (232)
cargo test --workspace --features shihaisha-engine/launchd,shihaisha-engine/supervisord

# Nix build
nix build

# See available backends on this machine
cargo run -- backends
```

## Architecture

| Crate | Tests | Purpose |
|-------|-------|---------|
| `shihaisha-core` | 133 | Types, traits (`InitBackend`, `ConfigEmitter`, `ConfigParser`, `HealthChecker`), merge/diff/graph algorithms, `MockBackend` |
| `shihaisha-engine` | 69 | Backend implementations (`NativeBackend`, `LaunchdBackend`, `SystemdBackend`, `SupervisordBackend`), `BackendRegistry`, health checker |
| `shihaisha-cli` | 16 | `shihaisha` binary -- 15 subcommands, testable `execute()` core |

Plus doc-tests. 232 tests total.

## Service Spec

Minimal YAML to get a service running:

```yaml
name: my-app
command: /usr/bin/my-app
args: ["--port", "8080"]
environment:
  RUST_LOG: info
restart:
  strategy: on-failure
  delay_secs: 5
  max_retries: 3
depends_on:
  requires: [database]
  conditions:
    database: service_healthy
readiness:
  type: tcp
  address: 127.0.0.1:5432
```

Full spec supports `service_type` (simple/oneshot/notify/forking/timer/socket),
resource limits, liveness/readiness/startup probes, socket activation, logging,
supervision groups, and per-backend overrides.

## Usage

```bash
# Install a service from a YAML spec
shihaisha install my-app.yaml

# Lifecycle
shihaisha start my-app
shihaisha stop my-app
shihaisha restart my-app

# Status and logs
shihaisha status my-app
shihaisha logs my-app -n 50

# Enable on boot, list all services
shihaisha enable my-app
shihaisha list

# Validate specs and dependency graph
shihaisha check ./services/

# Force a specific backend
shihaisha --backend native install my-app.yaml
```

## Backends

| Backend | Platform | Config Format | Feature Flag |
|---------|----------|---------------|--------------|
| `native` | Any | YAML in `~/.config/shihaisha/services/` | `native` (default) |
| `launchd` | macOS | XML plist in `~/Library/LaunchAgents/` | `launchd` |
| `systemd` | Linux | Unit files in `~/.config/systemd/user/` | `systemd` |
| `supervisord` | Any | INI in `~/.config/shihaisha/supervisord/` | `supervisord` |

Backend auto-detection probes in priority order: launchd, systemd, supervisord, native.
Override with `--backend <name>`.

## Key Features

| Feature | Description |
|---------|-------------|
| NixOS-style merge | Stack profile + override specs via `Merge` trait |
| Structural diff | `diff(old, new)` produces `Added`/`Removed`/`Modified` changes |
| Dependency graph | Kahn's toposort with cycle detection and reference validation |
| Health probes | HTTP, TCP, and command-based liveness/readiness/startup checks |
| Supervision groups | Erlang OTP-style strategies: `one_for_one`, `one_for_all`, `rest_for_one` |

## Nix Integration

Home-manager module at `blackmatter.components.shihaisha`:

```nix
blackmatter.components.shihaisha = {
  enable = true;
  backend = "auto";
  service_dir = "~/.config/shihaisha/services";
};
```

## License

MIT
