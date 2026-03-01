# How to customize Agent-box container runtime

## Goal

Adjust runtime backend, image, entrypoint, mounts, ports, hosts, and network mode.

## Steps

1. Edit `~/.agent-box.toml`:

    ```toml
    [runtime]
    backend = "podman" # or "docker"
    image = "agent-box:latest"
    entrypoint = "/bin/bash"
    ports = ["8080:8080"]
    hosts = ["host.docker.internal:host-gateway"]

    [runtime.mounts.ro]
    absolute = ["/nix/store"]

    [runtime.mounts.rw]
    home_relative = ["~/.local/share"]
    ```

2. Validate configuration:

    ```bash
    ab dbg validate
    ```

3. Preview resolved profile/runtime:

    ```bash
    ab dbg resolve -p your-profile
    ```

4. Override per spawn when needed:

    ```bash
    ab spawn -s demo -p rust -P 3000:3000 -H myhost:10.0.0.1 --network=bridge
    ```

## Notes

- `--network=host` and port/host mappings may conflict on Docker runtime.
- Overlay mount mode (`o`) is Podman-only.
