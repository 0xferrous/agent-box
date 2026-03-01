use agent_box_common::portal::{RequestMethod, ResponseResult};
use agent_box_common::portal_client::PortalClient;
use clap::{Parser, Subcommand};
use eyre::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "agent-portal-cli")]
#[command(about = "Official CLI client for agent portal host service")]
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
    GhExec {
        #[arg(long)]
        reason: Option<String>,
        #[arg(long, default_value_t = false)]
        require_approval: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        argv: Vec<String>,
    },
    Exec {
        #[arg(long)]
        reason: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        argv: Vec<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        env: Vec<String>,
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
    let out_path = match &cli.command {
        Commands::ClipboardReadImage { out, .. } => out.clone(),
        _ => None,
    };

    let method = match cli.command {
        Commands::Ping => RequestMethod::Ping,
        Commands::Whoami => RequestMethod::WhoAmI,
        Commands::ClipboardReadImage { reason, .. } => RequestMethod::ClipboardReadImage { reason },
        Commands::GhExec {
            reason,
            require_approval,
            argv,
        } => RequestMethod::GhExec {
            argv,
            reason,
            require_approval,
        },
        Commands::Exec {
            reason,
            argv,
            cwd,
            env,
        } => RequestMethod::Exec {
            argv,
            reason,
            cwd,
            env: Some(HashMap::from_iter(
                env.into_iter()
                    .map(|x| {
                        let val = x.split_once('=').unwrap();
                        (val.0.to_string(), val.1.to_string())
                    })
                    .collect::<Vec<(String, String)>>(),
            )),
        },
    };

    let client = if let Some(socket) = cli.socket {
        PortalClient::with_socket(socket)
    } else {
        PortalClient::from_env_or_config()
    };

    let result = client.request(method)?;

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
        ResponseResult::GhExec {
            exit_code,
            stdout,
            stderr,
        } => {
            if !stdout.is_empty() {
                print!("{}", String::from_utf8_lossy(&stdout));
            }
            if !stderr.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&stderr));
            }
            std::process::exit(exit_code);
        }
        ResponseResult::Exec { result } => {
            if !result.stdout.is_empty() {
                print!("{}", String::from_utf8_lossy(&result.stdout));
            }
            if !result.stderr.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&result.stderr));
            }
            std::process::exit(result.exit_code);
        }
    }

    Ok(())
}
