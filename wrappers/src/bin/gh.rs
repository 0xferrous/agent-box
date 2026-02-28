use agent_box_common::portal_client::PortalClient;
use eyre::Result;
use std::env;
use std::io::{self, Write};

fn main() {
    if let Err(e) = run() {
        eprintln!("gh wrapper error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let gh_args: Vec<String> = env::args().skip(1).collect();
    let reason = Some(format!("gh-wrapper cmd={}", shell_join(&gh_args)));

    let client = PortalClient::from_env_or_config();
    let result = client.gh_exec(gh_args, reason, false)?;

    if !result.stdout.is_empty() {
        io::stdout().write_all(&result.stdout)?;
    }
    if !result.stderr.is_empty() {
        io::stderr().write_all(&result.stderr)?;
    }

    std::process::exit(result.exit_code);
}

// Build a shell-like single-line representation of argv for the portal `reason` field.
// This is only for human-readable audit/prompt context on the host side; execution uses
// the original argv vector and does NOT go through a shell.
fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._/:=@".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
