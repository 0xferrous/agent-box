use crate::config::load_config;
use crate::portal::{PortalRequest, PortalResponse, RequestMethod, ResponseResult};
use eyre::{Context, Result};
use rmp_serde::{from_read, to_vec_named};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct PortalClient {
    pub socket_path: String,
}

#[derive(Debug, Clone)]
pub struct ClipboardImage {
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct GhExecResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl PortalClient {
    pub fn from_env_or_config() -> Self {
        if let Ok(path) = std::env::var("AGENT_PORTAL_SOCKET") {
            return Self { socket_path: path };
        }

        if let Ok(cfg) = load_config() {
            return Self {
                socket_path: cfg.portal.socket_path,
            };
        }

        Self {
            socket_path: crate::portal::PortalConfig::default().socket_path,
        }
    }

    pub fn with_socket(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn request(&self, method: RequestMethod) -> Result<ResponseResult> {
        let req = PortalRequest {
            version: 1,
            id: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            method,
        };

        let mut stream = UnixStream::connect(&self.socket_path)
            .wrap_err_with(|| format!("failed to connect to socket {}", self.socket_path))?;

        let bytes = to_vec_named(&req).wrap_err("failed to encode request")?;
        stream
            .write_all(&bytes)
            .wrap_err("failed to write request")?;

        let response: PortalResponse =
            from_read(&mut stream).wrap_err("failed to decode response")?;

        if !response.ok {
            let e = response
                .error
                .map(|x| format!("{}: {}", x.code, x.message))
                .unwrap_or_else(|| "unknown error".to_string());
            return Err(eyre::eyre!(e));
        }

        response
            .result
            .ok_or_else(|| eyre::eyre!("missing response result"))
    }

    pub fn clipboard_read_image(&self, reason: Option<String>) -> Result<ClipboardImage> {
        let result = self.request(RequestMethod::ClipboardReadImage { reason })?;
        match result {
            ResponseResult::ClipboardImage { mime, bytes } => Ok(ClipboardImage { mime, bytes }),
            other => Err(eyre::eyre!("unexpected response: {other:?}")),
        }
    }

    pub fn gh_exec(
        &self,
        argv: Vec<String>,
        reason: Option<String>,
        require_approval: bool,
    ) -> Result<GhExecResult> {
        let result = self.request(RequestMethod::GhExec {
            argv,
            reason,
            require_approval,
        })?;
        match result {
            ResponseResult::GhExec {
                exit_code,
                stdout,
                stderr,
            } => Ok(GhExecResult {
                exit_code,
                stdout,
                stderr,
            }),
            other => Err(eyre::eyre!("unexpected response: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_socket_sets_path() {
        let c = PortalClient::with_socket("/tmp/example.sock");
        assert_eq!(c.socket_path, "/tmp/example.sock");
    }

    #[test]
    fn from_env_prefers_agent_portal_socket() {
        let key = "AGENT_PORTAL_SOCKET";
        let prev = std::env::var(key).ok();

        unsafe {
            std::env::set_var(key, "/tmp/from-env.sock");
        }

        let c = PortalClient::from_env_or_config();
        assert_eq!(c.socket_path, "/tmp/from-env.sock");

        match prev {
            Some(v) => unsafe {
                std::env::set_var(key, v);
            },
            None => unsafe {
                std::env::remove_var(key);
            },
        }
    }
}
