use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod commands;

#[derive(Parser)]
#[command(name = "shihaisha")]
#[command(about = "Unified service management -- systemd/launchd/native backends")]
#[command(version)]
struct Cli {
    /// Backend to use (auto-detected if not specified)
    #[arg(long, global = true)]
    backend: Option<String>,

    #[command(subcommand)]
    command: commands::Command,
}

#[tokio::main]
async fn main() -> Result<()> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    commands::run(cli.command, cli.backend).await
}
