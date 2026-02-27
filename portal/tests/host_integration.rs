use agent_box_common::portal::{PortalRequest, PortalResponse, RequestMethod, ResponseResult};
use rmp_serde::{from_read, to_vec_named};
use std::fs;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()))
}

fn write_config(home: &Path, socket_path: &Path) {
    let cfg_path = home.join(".agent-box.toml");
    let content = format!(
        r#"workspace_dir = "{home}/ws"
base_repo_dir = "{home}/repos"

[runtime]
backend = "podman"
image = "test:latest"

[portal]
enabled = true
socket_path = "{}"

[portal.policy.defaults]
clipboard_read_image = "deny"
"#,
        socket_path.display(),
        home = home.display()
    );
    fs::write(cfg_path, content).unwrap();
    fs::create_dir_all(home.join("ws")).unwrap();
    fs::create_dir_all(home.join("repos")).unwrap();
}

fn start_host(home: &Path, socket_path: &Path) -> Child {
    let exe = env!("CARGO_BIN_EXE_agent-portal-host");
    Command::new(exe)
        .arg("--socket")
        .arg(socket_path)
        .env("HOME", home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn host")
}

fn wait_for_socket(path: &Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("socket did not appear: {}", path.display());
}

fn send_req(socket_path: &Path, req: PortalRequest) -> PortalResponse {
    let mut stream = UnixStream::connect(socket_path).expect("connect failed");
    let bytes = to_vec_named(&req).expect("encode failed");
    stream.write_all(&bytes).expect("write failed");
    from_read(&mut stream).expect("decode failed")
}

#[test]
fn host_ping_roundtrip_works() {
    let home = unique_temp_dir("portal-home");
    fs::create_dir_all(&home).unwrap();
    let socket_path = home.join("portal.sock");
    write_config(&home, &socket_path);

    let mut child = start_host(&home, &socket_path);
    wait_for_socket(&socket_path);

    let req = PortalRequest {
        version: 1,
        id: 1,
        method: RequestMethod::Ping,
    };

    let resp = send_req(&socket_path, req);
    assert!(resp.ok);
    assert_eq!(resp.id, 1);
    assert!(matches!(resp.result, Some(ResponseResult::Pong { .. })));

    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_file(&socket_path);
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn host_whoami_roundtrip_works() {
    let home = unique_temp_dir("portal-home");
    fs::create_dir_all(&home).unwrap();
    let socket_path = home.join("portal.sock");
    write_config(&home, &socket_path);

    let mut child = start_host(&home, &socket_path);
    wait_for_socket(&socket_path);

    let req = PortalRequest {
        version: 1,
        id: 2,
        method: RequestMethod::WhoAmI,
    };

    let resp = send_req(&socket_path, req);
    assert!(resp.ok);
    assert_eq!(resp.id, 2);

    match resp.result {
        Some(ResponseResult::WhoAmI {
            pid,
            uid,
            gid: _,
            container_id: _,
        }) => {
            assert!(pid > 0);
            assert!(uid > 0);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_file(&socket_path);
    let _ = fs::remove_dir_all(&home);
}
