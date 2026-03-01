# Agent-box: use profiles effectively

## Goal

Use profiles to keep `~/.agent-box.toml` modular and compose task-specific runtime settings.

## When to use this

Use profiles when you want to:

- avoid one giant `[runtime]` block
- share reusable config chunks (for example `nix`, `gpg`, `rust`)
- apply extra settings only for specific sessions

## Step 1: Define reusable profiles

Create named profiles under `[profiles.*]`:

```toml
[profiles.nix]
env = ["NIX_REMOTE=daemon"]
mounts.ro.absolute = ["/nix/store"]
mounts.rw.absolute = ["/nix/var/nix/daemon-socket/"]

[profiles.rust]
mounts.rw.home_relative = ["~/.cargo"]

[profiles.gpg]
mounts.rw.absolute = [
  "/run/user/1000/gnupg/S.gpg-agent:~/.gnupg/S.gpg-agent",
]
```

## Step 2: Create a baseline profile

Compose a default profile with `extends`:

```toml
default_profile = "base"

[profiles.base]
extends = ["nix", "rust"]
```

This profile is automatically applied to `ab spawn`.

## Step 3: Add extra profiles per command

Apply additional profiles in CLI order:

```bash
ab spawn -r myrepo -s mysession -p gpg
```

You can stack multiple flags:

```bash
ab spawn -r myrepo -s mysession -p rust -p gpg
```

## Step 4: Verify the resolved config

Inspect what Agent-box will actually use:

```bash
ab dbg resolve
ab dbg resolve -p rust -p gpg
```

Validate that your config is accepted:

```bash
ab dbg validate
```

## Merge behavior checklist

- Scalars: later layers override earlier layers
- Arrays: values append
- Objects: merge recursively
- Profile order matters for scalar conflicts

## Related reference

- [Agent-box config reference](../../reference/agent-box/config.md)
