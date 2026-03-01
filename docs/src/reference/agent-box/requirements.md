# Agent-box requirements

## Runtime requirements

- Rust toolchain (workspace uses Rust edition 2024)
- Git
- Jujutsu (`jj`) for JJ workspace flows
- Docker or Podman for container execution

## Portal-related requirements

- `wl-paste` available on host when using portal clipboard methods
- `agent-portal-host` running for wrapper/API operations

## Optional tooling

- Nix / flakes for reproducible development shell workflows
- Nushell to run CLI reference generation script:
  - `nu docs/scripts/generate-cli-reference.nu`
