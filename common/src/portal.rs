use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

fn default_true() -> bool {
    true
}

fn default_socket_path() -> String {
    let uid = nix::unistd::getuid().as_raw();
    format!("/run/user/{uid}/agent-portal/portal.sock")
}

fn default_allowed_mime() -> Vec<String> {
    vec![
        "image/png".to_string(),
        "image/jpeg".to_string(),
        "image/webp".to_string(),
    ]
}

fn default_max_clipboard_bytes() -> usize {
    20 * 1024 * 1024
}

fn default_max_inflight() -> usize {
    32
}

fn default_prompt_queue() -> usize {
    64
}

fn default_rate_per_minute() -> u32 {
    60
}

fn default_rate_burst() -> u32 {
    10
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PolicyDecision {
    Allow,
    Ask,
    Deny,
}

fn default_clipboard_policy() -> PolicyDecision {
    PolicyDecision::Allow
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GhExecPolicyMode {
    #[serde(alias = "allow")]
    AskForNone,
    #[serde(alias = "ask")]
    AskForWrites,
    AskForAll,
    #[serde(alias = "deny")]
    DenyAll,
}

fn default_gh_exec_policy() -> GhExecPolicyMode {
    GhExecPolicyMode::AskForWrites
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MethodPolicy {
    #[serde(default = "default_clipboard_policy")]
    pub clipboard_read_image: PolicyDecision,
    #[serde(default = "default_gh_exec_policy")]
    pub gh_exec: GhExecPolicyMode,
}

impl Default for MethodPolicy {
    fn default() -> Self {
        Self {
            clipboard_read_image: default_clipboard_policy(),
            gh_exec: default_gh_exec_policy(),
        }
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PolicyConfig {
    #[serde(default)]
    pub defaults: MethodPolicy,
    #[serde(default)]
    pub containers: HashMap<String, MethodPolicy>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize, Default, JsonSchema)]
pub struct PortalTimeouts {
    #[serde(default)]
    pub request_ms: u64,
    #[serde(default)]
    pub prompt_ms: u64,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PortalLimits {
    #[serde(default = "default_max_inflight")]
    pub max_inflight: usize,
    #[serde(default = "default_prompt_queue")]
    pub prompt_queue: usize,
    #[serde(default = "default_rate_per_minute")]
    pub rate_per_minute: u32,
    #[serde(default = "default_rate_burst")]
    pub rate_burst: u32,
    #[serde(default = "default_max_clipboard_bytes")]
    pub max_clipboard_bytes: usize,
}

impl Default for PortalLimits {
    fn default() -> Self {
        Self {
            max_inflight: default_max_inflight(),
            prompt_queue: default_prompt_queue(),
            rate_per_minute: default_rate_per_minute(),
            rate_burst: default_rate_burst(),
            max_clipboard_bytes: default_max_clipboard_bytes(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ClipboardConfig {
    #[serde(default = "default_allowed_mime")]
    pub allowed_mime: Vec<String>,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            allowed_mime: default_allowed_mime(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PortalConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_socket_path")]
    pub socket_path: String,
    #[serde(default)]
    pub prompt_command: Option<String>,
    #[serde(default)]
    pub timeouts: PortalTimeouts,
    #[serde(default)]
    pub limits: PortalLimits,
    #[serde(default)]
    pub clipboard: ClipboardConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
}

impl Default for PortalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            socket_path: default_socket_path(),
            prompt_command: None,
            timeouts: PortalTimeouts::default(),
            limits: PortalLimits::default(),
            clipboard: ClipboardConfig::default(),
            policy: PolicyConfig::default(),
        }
    }
}

impl PortalConfig {
    pub fn socket_path_buf(&self) -> PathBuf {
        PathBuf::from(self.socket_path.clone())
    }

    pub fn policy_for_container(&self, container_id: Option<&str>) -> MethodPolicy {
        if let Some(id) = container_id
            && let Some(policy) = self.policy.containers.get(id)
        {
            return policy.clone();
        }
        self.policy.defaults.clone()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortalRequest {
    pub version: u16,
    pub id: u64,
    #[serde(flatten)]
    pub method: RequestMethod,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "method", content = "params")]
pub enum RequestMethod {
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "whoami")]
    WhoAmI,
    #[serde(rename = "clipboard.read_image")]
    ClipboardReadImage { reason: Option<String> },
    #[serde(rename = "gh.exec")]
    GhExec {
        argv: Vec<String>,
        reason: Option<String>,
        require_approval: bool,
    },
    #[serde(rename = "exec")]
    Exec {
        argv: Vec<String>,
        reason: Option<String>,
        cwd: Option<String>,
        env: Option<HashMap<String, String>>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortalError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortalResponse {
    pub version: u16,
    pub id: u64,
    pub ok: bool,
    pub result: Option<ResponseResult>,
    pub error: Option<PortalError>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "data")]
pub enum ResponseResult {
    Pong {
        now_unix_ms: u128,
    },
    WhoAmI {
        pid: i32,
        uid: u32,
        gid: u32,
        container_id: Option<String>,
    },
    ClipboardImage {
        mime: String,
        bytes: Vec<u8>,
    },
    GhExec {
        exit_code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Exec {
        #[serde(flatten)]
        result: ExecResult,
    },
}

impl PortalResponse {
    pub fn ok(id: u64, result: ResponseResult) -> Self {
        Self {
            version: 1,
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: u64, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            version: 1,
            id,
            ok: false,
            result: None,
            error: Some(PortalError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

pub fn extract_podman_container_id_from_cgroup(cgroup_contents: &str) -> Option<String> {
    for line in cgroup_contents.lines() {
        if let Some(start) = line.find("libpod-") {
            let suffix = &line[start + "libpod-".len()..];
            let id = suffix
                .chars()
                .take_while(|ch| ch.is_ascii_hexdigit())
                .collect::<String>();
            if !id.is_empty() {
                return Some(id);
            }
        }

        if let Some(start) = line.find("/libpod/") {
            let suffix = &line[start + "/libpod/".len()..];
            let id = suffix
                .chars()
                .take_while(|ch| ch.is_ascii_hexdigit())
                .collect::<String>();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_libpod_scope_id() {
        let c =
            "0::/user.slice/user-1000.slice/user@1000.service/user.slice/libpod-1234abcd5678.scope";
        assert_eq!(
            extract_podman_container_id_from_cgroup(c),
            Some("1234abcd5678".to_string())
        );
    }

    #[test]
    fn parse_libpod_path_id() {
        let c = "0::/machine.slice/libpod/abcdef1234567890";
        assert_eq!(
            extract_podman_container_id_from_cgroup(c),
            Some("abcdef1234567890".to_string())
        );
    }

    #[test]
    fn parse_no_container_id_returns_none() {
        let c = "0::/user.slice/user-1000.slice/session-1.scope";
        assert_eq!(extract_podman_container_id_from_cgroup(c), None);
    }

    #[test]
    fn default_portal_config_is_sane() {
        let cfg = PortalConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.socket_path.contains("agent-portal/portal.sock"));
        assert_eq!(cfg.limits.max_inflight, 32);
        assert_eq!(cfg.limits.prompt_queue, 64);
        assert_eq!(cfg.limits.rate_per_minute, 60);
        assert_eq!(cfg.limits.rate_burst, 10);
        assert_eq!(cfg.timeouts.request_ms, 0);
        assert_eq!(cfg.timeouts.prompt_ms, 0);
        assert!(
            cfg.clipboard
                .allowed_mime
                .contains(&"image/png".to_string())
        );
        assert_eq!(cfg.policy.defaults.gh_exec, GhExecPolicyMode::AskForWrites);
    }

    #[test]
    fn policy_for_container_uses_override_when_present() {
        let mut cfg = PortalConfig::default();
        cfg.policy.defaults.clipboard_read_image = PolicyDecision::Ask;
        cfg.policy.defaults.gh_exec = GhExecPolicyMode::AskForWrites;
        cfg.policy.containers.insert(
            "abc123".to_string(),
            MethodPolicy {
                clipboard_read_image: PolicyDecision::Deny,
                gh_exec: GhExecPolicyMode::DenyAll,
            },
        );

        let p = cfg.policy_for_container(Some("abc123"));
        assert_eq!(p.clipboard_read_image, PolicyDecision::Deny);
        assert_eq!(p.gh_exec, GhExecPolicyMode::DenyAll);

        let p2 = cfg.policy_for_container(Some("missing"));
        assert_eq!(p2.clipboard_read_image, PolicyDecision::Ask);
        assert_eq!(p2.gh_exec, GhExecPolicyMode::AskForWrites);
    }

    #[test]
    fn portal_response_helpers_build_expected_shapes() {
        let ok = PortalResponse::ok(42, ResponseResult::Pong { now_unix_ms: 10 });
        assert!(ok.ok);
        assert_eq!(ok.id, 42);
        assert!(ok.result.is_some());
        assert!(ok.error.is_none());

        let err = PortalResponse::err(7, "denied", "nope");
        assert!(!err.ok);
        assert_eq!(err.id, 7);
        assert!(err.result.is_none());
        assert_eq!(err.error.as_ref().map(|e| e.code.as_str()), Some("denied"));
    }

    #[test]
    fn exec_request_constructs_correctly() {
        let req = RequestMethod::Exec {
            argv: vec!["ls".to_string(), "-la".to_string()],
            reason: Some("list files".to_string()),
            cwd: Some("/tmp".to_string()),
            env: Some({
                let mut map = HashMap::new();
                map.insert("PATH".to_string(), "/usr/bin".to_string());
                map
            }),
        };

        match req {
            RequestMethod::Exec {
                argv,
                reason,
                cwd,
                env,
            } => {
                assert_eq!(argv, vec!["ls", "-la"]);
                assert_eq!(reason, Some("list files".to_string()));
                assert_eq!(cwd, Some("/tmp".to_string()));
                assert_eq!(
                    env.as_ref().and_then(|e| e.get("PATH")),
                    Some(&"/usr/bin".to_string())
                );
            }
            _ => panic!("Expected Exec variant"),
        }
    }

    #[test]
    fn exec_request_minimal_fields() {
        let req = RequestMethod::Exec {
            argv: vec!["echo".to_string(), "hello".to_string()],
            reason: None,
            cwd: None,
            env: None,
        };

        match req {
            RequestMethod::Exec {
                argv,
                reason,
                cwd,
                env,
            } => {
                assert_eq!(argv, vec!["echo", "hello"]);
                assert!(reason.is_none());
                assert!(cwd.is_none());
                assert!(env.is_none());
            }
            _ => panic!("Expected Exec variant"),
        }
    }

    #[test]
    fn exec_result_constructs_correctly() {
        let result = ExecResult {
            exit_code: 0,
            stdout: b"hello world".to_vec(),
            stderr: vec![],
        };

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"hello world");
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn exec_response_constructs_correctly() {
        let result = ResponseResult::Exec {
            result: ExecResult {
                exit_code: 1,
                stdout: vec![],
                stderr: b"error: something went wrong".to_vec(),
            },
        };
        let response = PortalResponse::ok(123, result);

        assert!(response.ok);
        assert_eq!(response.id, 123);
        assert!(response.error.is_none());

        match response.result {
            Some(ResponseResult::Exec { result }) => {
                assert_eq!(result.exit_code, 1);
                assert_eq!(result.stderr, b"error: something went wrong");
            }
            _ => panic!("Expected Exec result variant"),
        }
    }
}
