use agent_box_common::config::load_config;
use agent_box_common::portal::{PortalRequest, PortalResponse, RequestMethod, ResponseResult};
use clap::{Parser, Subcommand};
use eyre::{Context, Result};
use rmp_serde::{from_read, to_vec_named};
use std::fs;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "agent-portal-cli")]
#[command(about = "Debug client for agent portal host service")]
struct Cli {
    /// Override socket path
    #[arg(long)]
    socket: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Ping,
    Whoami,
    ClipboardReadImage {
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config()?;
    let socket = cli
        .socket
        .unwrap_or_else(|| config.portal.socket_path.clone());

    let out_path = match &cli.command {
        Commands::ClipboardReadImage { out, .. } => out.clone(),
        _ => None,
    };

    let method = match cli.command {
        Commands::Ping => RequestMethod::Ping,
        Commands::Whoami => RequestMethod::WhoAmI,
        Commands::ClipboardReadImage { reason, .. } => RequestMethod::ClipboardReadImage { reason },
    };

    let req = PortalRequest {
        version: 1,
        id: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        method,
    };

    let mut stream = UnixStream::connect(&socket)
        .wrap_err_with(|| format!("failed to connect to socket {}", socket))?;
    let bytes = to_vec_named(&req).wrap_err("failed to encode request")?;
    stream
        .write_all(&bytes)
        .wrap_err("failed to write request")?;

    let response: PortalResponse = from_read(&mut stream).wrap_err("failed to decode response")?;

    if !response.ok {
        let e = response
            .error
            .map(|x| format!("{}: {}", x.code, x.message))
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(eyre::eyre!(e));
    }

    let result = response
        .result
        .ok_or_else(|| eyre::eyre!("missing response result"))?;

    match result {
        ResponseResult::Pong { now_unix_ms } => {
            println!("pong {}", now_unix_ms);
        }
        ResponseResult::WhoAmI {
            pid,
            uid,
            gid,
            container_id,
        } => {
            println!("pid={pid} uid={uid} gid={gid}");
            println!(
                "container_id={}",
                container_id.unwrap_or_else(|| "(none)".to_string())
            );
        }
        ResponseResult::ClipboardImage { mime, bytes } => {
            if let Some(path) = out_path {
                fs::write(&path, &bytes)
                    .wrap_err_with(|| format!("failed writing {}", path.display()))?;
                println!(
                    "wrote {} bytes ({}) to {}",
                    bytes.len(),
                    mime,
                    path.display()
                );
            } else {
                println!("received {} bytes ({})", bytes.len(), mime);
            }
        }
    }

    Ok(())
}
