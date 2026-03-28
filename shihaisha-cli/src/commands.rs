use anyhow::Result;
use clap::Subcommand;
use shihaisha_core::InitBackend;
use shihaisha_engine::BackendRegistry;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Command {
    /// Install a service from a YAML spec file
    Install {
        /// Path to service spec YAML file
        spec: PathBuf,
    },
    /// Uninstall a service
    Uninstall {
        /// Service name
        name: String,
    },
    /// Start a service
    Start {
        /// Service name
        name: String,
    },
    /// Stop a service
    Stop {
        /// Service name
        name: String,
    },
    /// Restart a service
    Restart {
        /// Service name
        name: String,
    },
    /// Reload a service's configuration
    Reload {
        /// Service name
        name: String,
    },
    /// Show service status
    Status {
        /// Service name (omit for all services)
        name: Option<String>,
    },
    /// Show service logs
    Logs {
        /// Service name
        name: String,
        /// Number of lines to show
        #[arg(short = 'n', default_value = "50")]
        lines: u32,
    },
    /// Enable a service to start on boot
    Enable {
        /// Service name
        name: String,
    },
    /// Disable a service from starting on boot
    Disable {
        /// Service name
        name: String,
    },
    /// List all managed services
    List,
    /// Reload the init system's daemon configuration
    DaemonReload,
    /// Validate service specs without installing them
    Check {
        /// Path to service spec YAML file or directory
        path: PathBuf,
    },
    /// Show available backends
    Backends,
    /// Run as daemon (watch config directory for changes)
    Daemon,
}

/// Execute a command against a specific backend.
///
/// This is the testable core of the CLI: it takes a resolved backend
/// reference instead of doing registry detection itself.
pub async fn execute(command: &Command, backend: &dyn InitBackend) -> Result<()> {
    match command {
        Command::Install { spec } => {
            let content = tokio::fs::read_to_string(spec).await?;
            let service_spec: shihaisha_core::ServiceSpec =
                serde_yaml_ng::from_str(&content)?;
            backend.install(&service_spec).await?;
            println!("Installed service '{}'", service_spec.name);
        }
        Command::Uninstall { name } => {
            backend.uninstall(name).await?;
            println!("Uninstalled service '{name}'");
        }
        Command::Start { name } => {
            backend.start(name).await?;
            println!("Started service '{name}'");
        }
        Command::Stop { name } => {
            backend.stop(name).await?;
            println!("Stopped service '{name}'");
        }
        Command::Restart { name } => {
            backend.restart(name).await?;
            println!("Restarted service '{name}'");
        }
        Command::Reload { name } => {
            backend.reload(name).await?;
            println!("Reloaded service '{name}'");
        }
        Command::Status { name } => {
            if let Some(name) = name {
                let status = backend.status(name).await?;
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                let services = backend.list().await?;
                for svc in &services {
                    println!("{:<30} {:?}", svc.name, svc.state);
                }
            }
        }
        Command::Logs { name, lines } => {
            let logs = backend.logs(name, *lines).await?;
            for line in &logs {
                println!("{line}");
            }
        }
        Command::Enable { name } => {
            backend.enable(name).await?;
            println!("Enabled service '{name}'");
        }
        Command::Disable { name } => {
            backend.disable(name).await?;
            println!("Disabled service '{name}'");
        }
        Command::List => {
            let services = backend.list().await?;
            if services.is_empty() {
                println!("No managed services");
            } else {
                println!(
                    "{:<30} {:<12} {:<8} {}",
                    "NAME", "STATE", "PID", "UPTIME"
                );
                for svc in &services {
                    let pid = svc
                        .pid
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "-".into());
                    let uptime = svc
                        .uptime_secs
                        .map(|u| format!("{u}s"))
                        .unwrap_or_else(|| "-".into());
                    println!(
                        "{:<30} {:<12} {:<8} {}",
                        svc.name, svc.state, pid, uptime
                    );
                }
            }
        }
        Command::DaemonReload => {
            backend.daemon_reload().await?;
            println!("Daemon configuration reloaded");
        }
        Command::Check { .. } | Command::Backends | Command::Daemon => {
            // These commands need the registry or no backend at all.
            // They are handled in `run()` directly.
        }
    }
    Ok(())
}

pub async fn run(command: Command, backend_name: Option<String>) -> Result<()> {
    let registry = BackendRegistry::detect().await;

    // Commands that need the registry rather than a single backend,
    // or don't need a backend at all.
    match &command {
        Command::Check { path } => {
            if path.is_dir() {
                let mut specs = Vec::new();
                let mut entries = tokio::fs::read_dir(path).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let p = entry.path();
                    if p.extension().is_some_and(|e| e == "yaml" || e == "yml") {
                        let content = tokio::fs::read_to_string(&p).await?;
                        let spec: shihaisha_core::ServiceSpec =
                            serde_yaml_ng::from_str(&content)
                                .map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
                        spec.validate()
                            .map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
                        specs.push(spec);
                    }
                }
                shihaisha_core::validate_references(&specs)
                    .map_err(|e| anyhow::anyhow!("dependency error: {e}"))?;
                let _order = shihaisha_core::resolve_order(&specs)
                    .map_err(|e| anyhow::anyhow!("dependency cycle: {e}"))?;
                println!("Validated {} specs, no errors", specs.len());
            } else {
                let content = tokio::fs::read_to_string(path).await?;
                let spec: shihaisha_core::ServiceSpec = serde_yaml_ng::from_str(&content)?;
                spec.validate()?;
                println!("Valid: {}", spec.name);
            }
            return Ok(());
        }
        Command::Backends => {
            println!("Available backends:");
            for name in registry.available_backends() {
                let marker = if name == registry.default_name() {
                    " (default)"
                } else {
                    ""
                };
                println!("  {name}{marker}");
            }
            return Ok(());
        }
        Command::Daemon => {
            anyhow::bail!("daemon mode not yet implemented (requires shikumi config watcher)");
        }
        _ => {}
    }

    let backend = if let Some(ref name) = backend_name {
        registry
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("backend '{name}' not available"))?
    } else {
        registry
            .default_backend()
            .ok_or_else(|| anyhow::anyhow!("no backends available"))?
    };

    execute(&command, backend).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use shihaisha_core::mock::{Call, MockBackend};
    use shihaisha_core::ServiceSpec;
    use std::io::Write as _;

    #[tokio::test]
    async fn install_records_call() {
        let mock = MockBackend::new();

        // Write a valid spec YAML to a temp file
        let mut tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let spec = ServiceSpec::new("test-install", "/bin/echo");
        let yaml = serde_yaml_ng::to_string(&spec).expect("serialize");
        tmp.write_all(yaml.as_bytes()).expect("write");

        let cmd = Command::Install {
            spec: tmp.path().to_path_buf(),
        };
        execute(&cmd, &mock).await.expect("execute install");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Install(name) if name == "test-install"));
    }

    #[tokio::test]
    async fn start_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::Start {
            name: "my-svc".to_owned(),
        };
        execute(&cmd, &mock).await.expect("execute start");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Start(name) if name == "my-svc"));
    }

    #[tokio::test]
    async fn stop_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::Stop {
            name: "my-svc".to_owned(),
        };
        execute(&cmd, &mock).await.expect("execute stop");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Stop(name) if name == "my-svc"));
    }

    #[tokio::test]
    async fn restart_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::Restart {
            name: "my-svc".to_owned(),
        };
        execute(&cmd, &mock).await.expect("execute restart");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Restart(name) if name == "my-svc"));
    }

    #[tokio::test]
    async fn status_returns_ok() {
        let mock = MockBackend::new();
        let cmd = Command::Status {
            name: Some("my-svc".to_owned()),
        };
        execute(&cmd, &mock).await.expect("execute status");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Status(name) if name == "my-svc"));
    }

    #[tokio::test]
    async fn list_returns_ok() {
        let mock = MockBackend::new();
        let cmd = Command::List;
        execute(&cmd, &mock).await.expect("execute list");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::List));
    }

    #[tokio::test]
    async fn daemon_reload_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::DaemonReload;
        execute(&cmd, &mock).await.expect("execute daemon-reload");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::DaemonReload));
    }

    #[tokio::test]
    async fn uninstall_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::Uninstall {
            name: "myservice".to_owned(),
        };
        execute(&cmd, &mock).await.expect("execute uninstall");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Uninstall(name) if name == "myservice"));
    }

    #[tokio::test]
    async fn reload_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::Reload {
            name: "myservice".to_owned(),
        };
        execute(&cmd, &mock).await.expect("execute reload");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Reload(name) if name == "myservice"));
    }

    #[tokio::test]
    async fn logs_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::Logs {
            name: "myservice".to_owned(),
            lines: 100,
        };
        execute(&cmd, &mock).await.expect("execute logs");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Logs(name, 100) if name == "myservice"));
    }

    #[tokio::test]
    async fn enable_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::Enable {
            name: "myservice".to_owned(),
        };
        execute(&cmd, &mock).await.expect("execute enable");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Enable(name) if name == "myservice"));
    }

    #[tokio::test]
    async fn disable_records_call() {
        let mock = MockBackend::new();
        let cmd = Command::Disable {
            name: "myservice".to_owned(),
        };
        execute(&cmd, &mock).await.expect("execute disable");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::Disable(name) if name == "myservice"));
    }

    #[tokio::test]
    async fn status_all_services_records_list_call() {
        let mock = MockBackend::new();
        let cmd = Command::Status { name: None };
        execute(&cmd, &mock).await.expect("execute status all");

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], Call::List));
    }

    #[tokio::test]
    async fn install_missing_file_returns_error() {
        let mock = MockBackend::new();
        let cmd = Command::Install {
            spec: PathBuf::from("/tmp/shihaisha-nonexistent-spec-file.yaml"),
        };
        let result = execute(&cmd, &mock).await;
        assert!(result.is_err(), "expected error for missing spec file");
    }

    #[tokio::test]
    async fn check_valid_spec_file() {
        let spec = ServiceSpec::new("check-test", "/bin/echo");
        let yaml = serde_yaml_ng::to_string(&spec).expect("serialize");

        let mut tmp = tempfile::NamedTempFile::new().expect("tempfile");
        tmp.write_all(yaml.as_bytes()).expect("write");

        // Use run() which handles Check internally
        let cmd = Command::Check {
            path: tmp.path().to_path_buf(),
        };
        let result = run(cmd, None).await;
        assert!(result.is_ok(), "valid spec should pass check: {result:?}");
    }

    #[tokio::test]
    async fn check_valid_directory() {
        let dir = tempfile::tempdir().expect("tempdir");

        let db = ServiceSpec::new("db", "/usr/bin/db");
        let mut app = ServiceSpec::new("app", "/usr/bin/app");
        app.depends_on.after = vec!["db".to_owned()];

        let db_yaml = serde_yaml_ng::to_string(&db).expect("serialize");
        let app_yaml = serde_yaml_ng::to_string(&app).expect("serialize");

        tokio::fs::write(dir.path().join("db.yaml"), db_yaml)
            .await
            .expect("write");
        tokio::fs::write(dir.path().join("app.yaml"), app_yaml)
            .await
            .expect("write");

        let cmd = Command::Check {
            path: dir.path().to_path_buf(),
        };
        let result = run(cmd, None).await;
        assert!(
            result.is_ok(),
            "valid directory should pass check: {result:?}",
        );
    }
}
