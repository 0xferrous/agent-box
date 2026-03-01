# How to forward GPG agent to Agent-box containers

## Goal

Use host GPG keys/signing from inside container sessions.

## Steps

1. Discover host socket locations:

```bash
gpgconf --list-dirs
```

2. Configure overlay mount for `~/.gnupg` (Podman):

```toml
[runtime.mounts.o]
home_relative = ["~/.gnupg"]

[runtime.mounts.rw]
home_relative = [
  "/run/user/1000/gnupg/S.gpg-agent:~/.gnupg/S.gpg-agent",
  "/run/user/1000/gnupg/S.keyboxd:~/.gnupg/S.keyboxd",
]
```

3. Spawn session and test:

```bash
ab spawn -s my-session
gpg --list-secret-keys
```

## Notes

- Replace `1000` with your UID.
- For smartcards, add `S.scdaemon` socket mapping.
- Docker does not support overlay mounts; Podman is preferred here.

## Troubleshooting

- Lock conflicts: remove stale locks (`find ~/.gnupg -name '.#lk*' -delete`)
- IPC issues: verify socket path with `gpg-connect-agent 'getinfo socket_name' /bye`
