use agent_box_common::config::load_config;
use agent_box_common::portal::{
    GhExecPolicyMode, PolicyDecision, PortalRequest, PortalResponse, RequestMethod, ResponseResult,
    extract_podman_container_id_from_cgroup,
};
use clap::Parser;
use eyre::{Context, Result};
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use rmp_serde::{from_read, to_vec_named};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::IsTerminal;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "agent-portal-host")]
#[command(about = "Host portal service for container capability requests")]
struct Cli {
    /// Override socket path
    #[arg(long)]
    socket: Option<String>,
}

#[derive(Debug, Clone)]
struct CallerIdentity {
    pid: i32,
    uid: u32,
    gid: u32,
    container_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GhCommandOperation {
    Read,
    Write,
    ReadWrite,
    Unknown,
}

#[derive(Debug, Clone)]
struct GhPolicyData {
    op_map: HashMap<String, GhCommandOperation>,
    prefixes: HashSet<String>,
    roots: HashSet<String>,
}

#[derive(Debug)]
struct RateLimiter {
    // key -> request timestamps
    seen: HashMap<String, VecDeque<Instant>>,
    per_minute: u32,
    burst: u32,
}

impl RateLimiter {
    fn new(per_minute: u32, burst: u32) -> Self {
        Self {
            seen: HashMap::new(),
            per_minute,
            burst,
        }
    }

    fn allow(&mut self, key: &str) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(60);
        let entries = self.seen.entry(key.to_string()).or_default();

        while let Some(ts) = entries.front() {
            if now.duration_since(*ts) > window {
                let _ = entries.pop_front();
            } else {
                break;
            }
        }

        let limit = self.per_minute.max(self.burst) as usize;
        if entries.len() >= limit {
            return false;
        }
        entries.push_back(now);
        true
    }
}

#[derive(Clone)]
struct AppState {
    cfg: agent_box_common::portal::PortalConfig,
    inflight: Arc<AtomicUsize>,
    rate: Arc<Mutex<RateLimiter>>,
    prompt_inflight: Arc<AtomicUsize>,
    gh_policy: GhPolicyData,
}

fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,agent_portal_host=info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
}

fn main() {
    init_logging();

    if let Err(e) = run() {
        error!(error = %e, "portal host failed");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config()?;
    let portal = config.portal;

    if !portal.enabled {
        return Err(eyre::eyre!("portal is disabled in config"));
    }

    let socket_path = cli.socket.unwrap_or_else(|| portal.socket_path.clone());
    let socket_path = std::path::PathBuf::from(socket_path);

    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent).wrap_err("failed to create socket parent directory")?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .wrap_err("failed to set permissions on socket directory")?;
    }

    if socket_path.exists() {
        fs::remove_file(&socket_path).wrap_err("failed to remove stale socket")?;
    }

    let listener = UnixListener::bind(&socket_path)
        .wrap_err_with(|| format!("failed to bind socket at {}", socket_path.display()))?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .wrap_err("failed to set socket permissions")?;

    info!(socket = %socket_path.display(), "agent-portal-host listening");
    debug!(config = ?portal, "loaded portal config");
    info!(
        clipboard_read_image_policy = ?portal.policy.defaults.clipboard_read_image,
        gh_exec_policy = ?portal.policy.defaults.gh_exec,
        container_overrides = portal.policy.containers.len(),
        "portal default policies"
    );
    debug!(
        max_inflight = portal.limits.max_inflight,
        rate_per_minute = portal.limits.rate_per_minute,
        rate_burst = portal.limits.rate_burst,
        prompt_queue = portal.limits.prompt_queue,
        "runtime limits"
    );

    let state = AppState {
        cfg: portal.clone(),
        inflight: Arc::new(AtomicUsize::new(0)),
        rate: Arc::new(Mutex::new(RateLimiter::new(
            portal.limits.rate_per_minute,
            portal.limits.rate_burst,
        ))),
        prompt_inflight: Arc::new(AtomicUsize::new(0)),
        gh_policy: load_embedded_gh_policy()?,
    };

    for incoming in listener.incoming() {
        let stream = match incoming {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "accept error");
                continue;
            }
        };

        if state.inflight.load(Ordering::Relaxed) >= state.cfg.limits.max_inflight {
            let _ = send_response(
                stream,
                &PortalResponse::err(0, "too_busy", "too many in-flight requests"),
            );
            continue;
        }

        let st = state.clone();
        st.inflight.fetch_add(1, Ordering::Relaxed);
        thread::spawn(move || {
            let _ = handle_client(stream, &st);
            st.inflight.fetch_sub(1, Ordering::Relaxed);
        });
    }

    Ok(())
}

fn handle_client(mut stream: UnixStream, state: &AppState) -> Result<()> {
    let identity = peer_identity(&stream)?;
    debug!(
        pid = identity.pid,
        uid = identity.uid,
        gid = identity.gid,
        container_id = identity.container_id.as_deref().unwrap_or("(none)"),
        "request received"
    );
    if state.cfg.timeouts.request_ms > 0 {
        stream
            .set_read_timeout(Some(Duration::from_millis(state.cfg.timeouts.request_ms)))
            .wrap_err("failed to set read timeout")?;
        stream
            .set_write_timeout(Some(Duration::from_millis(state.cfg.timeouts.request_ms)))
            .wrap_err("failed to set write timeout")?;
    }

    let req: PortalRequest = from_read(&mut stream).wrap_err("failed to decode msgpack request")?;
    debug!(request_id = req.id, method = ?req.method, "decoded request");

    let rate_key = identity
        .container_id
        .clone()
        .unwrap_or_else(|| format!("pid:{}", identity.pid));

    {
        let mut guard = state
            .rate
            .lock()
            .map_err(|_| eyre::eyre!("rate limiter poisoned"))?;
        if !guard.allow(&rate_key) {
            debug!(key = %rate_key, request_id = req.id, "request rate-limited");
            return send_response(
                stream,
                &PortalResponse::err(req.id, "rate_limited", "request rate exceeded"),
            );
        }
    }

    let response = match req.method {
        RequestMethod::Ping => PortalResponse::ok(
            req.id,
            ResponseResult::Pong {
                now_unix_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
            },
        ),
        RequestMethod::WhoAmI => PortalResponse::ok(
            req.id,
            ResponseResult::WhoAmI {
                pid: identity.pid,
                uid: identity.uid,
                gid: identity.gid,
                container_id: identity.container_id.clone(),
            },
        ),
        RequestMethod::ClipboardReadImage { reason } => {
            let policy = state
                .cfg
                .policy_for_container(identity.container_id.as_deref());
            debug!(
                request_id = req.id,
                policy = ?policy.clipboard_read_image,
                reason = reason.as_deref().unwrap_or("(none)"),
                "processing clipboard.read_image"
            );
            let decision = match policy.clipboard_read_image {
                PolicyDecision::Allow => Ok(true),
                PolicyDecision::Deny => Ok(false),
                PolicyDecision::Ask => prompt_allow(state, &identity, reason.as_deref()),
            };

            match decision {
                Ok(false) => {
                    debug!(request_id = req.id, "clipboard.read_image denied");
                    PortalResponse::err(req.id, "denied", "request denied by policy")
                }
                Err(e) => {
                    debug!(request_id = req.id, error = %e, "clipboard.read_image prompt failed");
                    PortalResponse::err(req.id, "prompt_failed", e.to_string())
                }
                Ok(true) => match clipboard_read_image(
                    &state.cfg.clipboard.allowed_mime,
                    state.cfg.limits.max_clipboard_bytes,
                ) {
                    Ok((mime, bytes)) => {
                        PortalResponse::ok(req.id, ResponseResult::ClipboardImage { mime, bytes })
                    }
                    Err(e) => PortalResponse::err(req.id, "clipboard_failed", e.to_string()),
                },
            }
        }
        RequestMethod::GhExec {
            argv,
            reason,
            require_approval: _,
        } => {
            let policy = state
                .cfg
                .policy_for_container(identity.container_id.as_deref());

            let operation = classify_gh_operation(&state.gh_policy, &argv);
            let should_prompt = match policy.gh_exec {
                GhExecPolicyMode::AskForAll => true,
                GhExecPolicyMode::AskForWrites => {
                    matches!(
                        operation,
                        GhCommandOperation::Write
                            | GhCommandOperation::ReadWrite
                            | GhCommandOperation::Unknown
                    )
                }
                GhExecPolicyMode::AskForNone => false,
                GhExecPolicyMode::DenyAll => {
                    return send_response(
                        stream,
                        &PortalResponse::err(req.id, "denied", "gh.exec denied by policy"),
                    );
                }
            };

            info!(
                request_id = req.id,
                container_id = identity.container_id.as_deref().unwrap_or("(none)"),
                policy = ?policy.gh_exec,
                operation = ?operation,
                should_prompt,
                "gh.exec policy decision"
            );
            debug!(request_id = req.id, argv = ?argv, "gh.exec argv");

            if should_prompt {
                match prompt_allow(state, &identity, reason.as_deref()) {
                    Ok(true) => {}
                    Ok(false) => {
                        return send_response(
                            stream,
                            &PortalResponse::err(req.id, "denied", "request denied by policy"),
                        );
                    }
                    Err(e) => {
                        return send_response(
                            stream,
                            &PortalResponse::err(req.id, "prompt_failed", e.to_string()),
                        );
                    }
                }
            }

            match execute_gh_on_host(&argv) {
                Ok((exit_code, stdout, stderr)) => PortalResponse::ok(
                    req.id,
                    ResponseResult::GhExec {
                        exit_code,
                        stdout,
                        stderr,
                    },
                ),
                Err(e) => PortalResponse::err(req.id, "gh_exec_failed", e.to_string()),
            }
        }
    };

    send_response(stream, &response)
}

fn send_response(mut stream: UnixStream, response: &PortalResponse) -> Result<()> {
    let bytes = to_vec_named(response).wrap_err("failed to encode msgpack response")?;
    stream
        .write_all(&bytes)
        .wrap_err("failed writing response bytes")
}

fn peer_identity(stream: &UnixStream) -> Result<CallerIdentity> {
    let creds = getsockopt(stream, PeerCredentials).wrap_err("failed to read peer credentials")?;
    let pid = creds.pid();
    let uid = creds.uid();
    let gid = creds.gid();

    let container_id = resolve_container_id(pid);

    Ok(CallerIdentity {
        pid,
        uid,
        gid,
        container_id,
    })
}

fn resolve_container_id(pid: i32) -> Option<String> {
    let path = format!("/proc/{pid}/cgroup");
    let cgroup = fs::read_to_string(path).ok()?;
    extract_podman_container_id_from_cgroup(&cgroup)
}

fn prompt_allow(state: &AppState, identity: &CallerIdentity, reason: Option<&str>) -> Result<bool> {
    if state.prompt_inflight.load(Ordering::Relaxed) >= state.cfg.limits.prompt_queue {
        return Err(eyre::eyre!("prompt queue full"));
    }

    let prompt_cmd = state
        .cfg
        .prompt_command
        .as_ref()
        .ok_or_else(|| eyre::eyre!("prompt_command not configured"))?
        .clone();

    state.prompt_inflight.fetch_add(1, Ordering::Relaxed);

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(prompt_cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err("failed spawning prompt command")?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| eyre::eyre!("failed to open prompt stdin"))?;
        let context = format!(
            "container={} pid={} reason={}",
            identity
                .container_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            identity.pid,
            reason.unwrap_or("(none)")
        );
        let menu = format!("allow-once ({context})\nallow-session ({context})\ndeny ({context})\n");
        stdin
            .write_all(menu.as_bytes())
            .wrap_err("failed writing prompt choices")?;
    }

    let output = if state.cfg.timeouts.prompt_ms == 0 {
        child.wait_with_output().wrap_err("prompt command failed")?
    } else {
        wait_with_timeout(
            &mut child,
            Duration::from_millis(state.cfg.timeouts.prompt_ms),
        )?
        .ok_or_else(|| eyre::eyre!("prompt timed out"))?
    };

    state.prompt_inflight.fetch_sub(1, Ordering::Relaxed);

    if !output.status.success() {
        return Err(eyre::eyre!(
            "prompt command returned non-zero status: {}",
            output.status
        ));
    }

    let selected = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();
    Ok(selected.starts_with("allow"))
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<Option<std::process::Output>> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().wrap_err("failed waiting for prompt")? {
            use std::io::Read;

            let mut stdout = Vec::new();
            let mut stderr = Vec::new();

            if let Some(mut out) = child.stdout.take() {
                out.read_to_end(&mut stdout)
                    .wrap_err("failed reading prompt stdout")?;
            }
            if let Some(mut err) = child.stderr.take() {
                err.read_to_end(&mut stderr)
                    .wrap_err("failed reading prompt stderr")?;
            }

            return Ok(Some(std::process::Output {
                status,
                stdout,
                stderr,
            }));
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn resolve_host_gh_binary() -> String {
    if let Ok(bin) = std::env::var("AGENT_PORTAL_HOST_GH")
        && !bin.trim().is_empty()
    {
        return bin;
    }

    for candidate in ["/run/current-system/sw/bin/gh", "/usr/bin/gh"] {
        if std::path::Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }

    "gh".to_string()
}

fn execute_gh_on_host(argv: &[String]) -> Result<(i32, Vec<u8>, Vec<u8>)> {
    let gh_bin = resolve_host_gh_binary();
    let out = Command::new(&gh_bin)
        .args(argv)
        .output()
        .wrap_err_with(|| format!("failed to run host gh binary: {}", gh_bin))?;

    let exit_code = out.status.code().unwrap_or(1);
    Ok((exit_code, out.stdout, out.stderr))
}

#[derive(Debug, Deserialize)]
struct EmbeddedGhReport {
    commands: Vec<EmbeddedGhCommand>,
}

#[derive(Debug, Deserialize)]
struct EmbeddedGhCommand {
    command: String,
    operation: String,
}

fn load_embedded_gh_policy() -> Result<GhPolicyData> {
    let raw = include_str!("../../gh-leaf-command-read-write-report.json");
    let report: EmbeddedGhReport =
        serde_json::from_str(raw).wrap_err("invalid embedded gh policy JSON")?;

    let mut op_map = HashMap::new();
    let mut prefixes = HashSet::new();
    let mut roots = HashSet::new();

    for row in report.commands {
        let op = match row.operation.as_str() {
            "Read" => GhCommandOperation::Read,
            "Write" => GhCommandOperation::Write,
            "Read/Write" => GhCommandOperation::ReadWrite,
            _ => GhCommandOperation::Unknown,
        };

        let parts: Vec<&str> = row.command.split(' ').collect();
        if let Some(root) = parts.first() {
            roots.insert((*root).to_string());
        }

        for i in 1..=parts.len() {
            prefixes.insert(parts[..i].join(" "));
        }

        op_map.insert(row.command, op);
    }

    Ok(GhPolicyData {
        op_map,
        prefixes,
        roots,
    })
}

fn classify_gh_operation(policy: &GhPolicyData, argv: &[String]) -> GhCommandOperation {
    let Some(start) = argv.iter().position(|tok| policy.roots.contains(tok)) else {
        return GhCommandOperation::Unknown;
    };

    let mut parts: Vec<String> = Vec::new();
    for tok in argv.iter().skip(start) {
        if tok.starts_with('-') {
            continue;
        }

        let mut candidate_parts = parts.clone();
        candidate_parts.push(tok.clone());
        let candidate = candidate_parts.join(" ");
        if policy.prefixes.contains(&candidate) {
            parts.push(tok.clone());
        } else {
            break;
        }
    }

    if parts.is_empty() {
        return GhCommandOperation::Unknown;
    }

    let path = parts.join(" ");
    policy
        .op_map
        .get(&path)
        .copied()
        .unwrap_or(GhCommandOperation::Unknown)
}

fn resolve_host_wl_paste_binary() -> String {
    if let Ok(bin) = std::env::var("AGENT_PORTAL_HOST_WL_PASTE")
        && !bin.trim().is_empty()
    {
        return bin;
    }

    // Prefer host/system binaries to avoid recursive wrapper invocation.
    for candidate in ["/run/current-system/sw/bin/wl-paste", "/usr/bin/wl-paste"] {
        if std::path::Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }

    "wl-paste".to_string()
}

fn clipboard_read_image(allowed_mime: &[String], max_bytes: usize) -> Result<(String, Vec<u8>)> {
    let wl_paste_bin = resolve_host_wl_paste_binary();
    debug!(binary = %wl_paste_bin, "using wl-paste binary");

    let types_out = Command::new(&wl_paste_bin)
        .arg("--list-types")
        .output()
        .wrap_err_with(|| format!("failed to run {} --list-types", wl_paste_bin))?;

    if !types_out.status.success() {
        return Err(eyre::eyre!(
            "wl-paste --list-types failed: {}",
            String::from_utf8_lossy(&types_out.stderr)
        ));
    }

    let offered: Vec<String> = String::from_utf8_lossy(&types_out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mime = allowed_mime
        .iter()
        .find(|m| offered.iter().any(|o| o == *m))
        .cloned()
        .ok_or_else(|| eyre::eyre!("clipboard does not currently contain an allowed image MIME"))?;
    debug!(mime = %mime, "selected clipboard mime");

    let out = Command::new(&wl_paste_bin)
        .args(["--no-newline", "--type", &mime])
        .output()
        .wrap_err_with(|| format!("failed to run {} for image bytes", wl_paste_bin))?;

    if !out.status.success() {
        return Err(eyre::eyre!(
            "wl-paste failed for mime {}: {}",
            mime,
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    if out.stdout.len() > max_bytes {
        return Err(eyre::eyre!(
            "clipboard image exceeds size limit ({} > {})",
            out.stdout.len(),
            max_bytes
        ));
    }

    debug!(bytes = out.stdout.len(), mime = %mime, "clipboard image request accepted");

    Ok((mime, out.stdout))
}
