# How to share host Nix store with containers

## Goal

Use host Nix binaries/store paths from inside Agent-box containers.

## Configuration

```toml
[runtime.env]
# alternatively set in runtime.env as KEY=VALUE string entries

[runtime]
env = ["NIX_REMOTE=daemon"]

[runtime.mounts.ro]
absolute = ["/nix/store"]

[runtime.mounts.rw]
absolute = ["/nix/var/nix/daemon-socket/"]
```

## Verify

```bash
ab spawn -s my-session
nix --version
```

## Notes

- Store is mounted read-only.
- Daemon socket mount enables requesting builds/fetches through host daemon.
