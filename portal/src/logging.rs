use eyre::{Result, WrapErr};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, registry};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Clone)]
struct SharedFileWriter {
    file: Arc<Mutex<File>>,
}

impl Write for SharedFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("failed to lock portal log file"))?;
        file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("failed to lock portal log file"))?;
        file.flush()
    }
}

fn log_dir() -> PathBuf {
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state_home).join("agent-box").join("logs");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("agent-box")
            .join("logs");
    }

    std::env::temp_dir().join("agent-box").join("logs")
}

pub fn default_log_path(socket_path: Option<&Path>) -> PathBuf {
    let file_name = socket_path
        .and_then(Path::file_name)
        .unwrap_or_else(|| OsStr::new("agent-portal-host.sock"));
    let mut log_name = PathBuf::from(file_name);
    log_name.set_extension("log");
    log_dir().join(log_name)
}

pub fn init(log_level: Option<&str>, socket_path: Option<&Path>, visible: bool) -> Result<PathBuf> {
    if let Some(path) = LOG_PATH.get() {
        return Ok(path.clone());
    }

    let log_path = default_log_path(socket_path);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).wrap_err("failed to create portal log directory")?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .wrap_err_with(|| format!("failed to open portal log file {}", log_path.display()))?;
    let file = Arc::new(Mutex::new(file));

    let env_filter = match log_level {
        Some(level) => EnvFilter::new(level),
        None => EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
    };

    if visible {
        let file = Arc::clone(&file);
        let file_layer = fmt::layer()
            .with_ansi(false)
            .with_writer(move || SharedFileWriter {
                file: Arc::clone(&file),
            });
        let stderr_layer = fmt::layer()
            .with_ansi(std::io::stderr().is_terminal())
            .with_writer(std::io::stderr);
        registry()
            .with(env_filter)
            .with(stderr_layer)
            .with(file_layer)
            .try_init()
            .map_err(|e| eyre::eyre!("failed to initialize portal logging: {e}"))?;
    } else {
        let file = Arc::clone(&file);
        let file_layer = fmt::layer()
            .with_ansi(false)
            .with_writer(move || SharedFileWriter {
                file: Arc::clone(&file),
            });
        registry()
            .with(env_filter)
            .with(file_layer)
            .try_init()
            .map_err(|e| eyre::eyre!("failed to initialize portal logging: {e}"))?;
    }

    let _ = LOG_PATH.set(log_path.clone());
    tracing::info!(log_file = %log_path.display(), "portal logging initialized");

    Ok(log_path)
}
