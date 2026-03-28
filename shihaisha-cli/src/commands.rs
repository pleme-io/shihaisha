use anyhow::Result;
use clap::Subcommand;
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
    /// Show available backends
    Backends,
    /// Run as daemon (watch config directory for changes)
    Daemon,
}

pub async fn run(command: Command, backend_name: Option<String>) -> Result<()> {
    let registry = BackendRegistry::detect().await;

    let backend = if let Some(ref name) = backend_name {
        registry
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("backend '{name}' not available"))?
    } else {
        registry
            .default_backend()
            .ok_or_else(|| anyhow::anyhow!("no backends available"))?
    };

    match command {
        Command::Install { spec } => {
            let content = tokio::fs::read_to_string(&spec).await?;
            let service_spec: shihaisha_core::ServiceSpec =
                serde_yaml_ng::from_str(&content)?;
            backend.install(&service_spec).await?;
            println!("Installed service '{}'", service_spec.name);
        }
        Command::Uninstall { name } => {
            backend.uninstall(&name).await?;
            println!("Uninstalled service '{name}'");
        }
        Command::Start { name } => {
            backend.start(&name).await?;
            println!("Started service '{name}'");
        }
        Command::Stop { name } => {
            backend.stop(&name).await?;
            println!("Stopped service '{name}'");
        }
        Command::Restart { name } => {
            backend.restart(&name).await?;
            println!("Restarted service '{name}'");
        }
        Command::Reload { name } => {
            backend.reload(&name).await?;
            println!("Reloaded service '{name}'");
        }
        Command::Status { name } => {
            if let Some(name) = name {
                let status = backend.status(&name).await?;
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                let services = backend.list().await?;
                for svc in &services {
                    println!("{:<30} {:?}", svc.name, svc.state);
                }
            }
        }
        Command::Logs { name, lines } => {
            let logs = backend.logs(&name, lines).await?;
            for line in &logs {
                println!("{line}");
            }
        }
        Command::Enable { name } => {
            backend.enable(&name).await?;
            println!("Enabled service '{name}'");
        }
        Command::Disable { name } => {
            backend.disable(&name).await?;
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
                        "{:<30} {:<12?} {:<8} {}",
                        svc.name, svc.state, pid, uptime
                    );
                }
            }
        }
        Command::DaemonReload => {
            backend.daemon_reload().await?;
            println!("Daemon configuration reloaded");
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
        }
        Command::Daemon => {
            println!("Starting shihaisha daemon (watching for config changes)...");
            // TODO: implement config watcher with shikumi
            println!("Daemon mode not yet implemented");
        }
    }
    Ok(())
}
