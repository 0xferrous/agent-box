use agent_box_common::config::load_config;
use clap::Parser;
use eyre::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tracing::error;

#[derive(Parser, Debug)]
#[command(name = "agent-portal-host")]
#[command(about = "Host portal service for container capability requests")]
struct Cli {
    /// Override socket path
    #[arg(long)]
    socket: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    let config = match load_config() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };
    let socket_path = PathBuf::from(
        cli.socket
            .clone()
            .unwrap_or_else(|| config.portal.socket_path.clone()),
    );

    if let Err(e) = agent_portal::logging::init(None, Some(&socket_path), true) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    if let Err(e) = run(cli, config.portal) {
        error!(error = %e, "portal host failed");
        std::process::exit(1);
    }
}

fn run(cli: Cli, portal: agent_box_common::portal::PortalConfig) -> Result<()> {
    let path = std::env::var("PATH").unwrap_or_default();
    let path = path.split(':').collect::<Vec<_>>();
    tracing::info!(path = ?path, "PATH");

    let socket_path = PathBuf::from(cli.socket.unwrap_or_else(|| portal.socket_path.clone()));

    agent_portal::host::run_with_config_and_socket(
        portal,
        socket_path,
        Arc::new(AtomicBool::new(false)),
    )
}
