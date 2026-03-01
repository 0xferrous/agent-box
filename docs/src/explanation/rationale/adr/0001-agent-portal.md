# ADR 0001: Host Portal Service for Container-to-Host Capabilities

- Status: Accepted
- Date: 2026-02-27

## Context

We need a secure way for containers to access selected host capabilities (starting with host clipboard image read), with optional user approval similar to xdg-desktop-portal behavior.

Directly exposing host Wayland/X11 sockets to containers is too broad and does not provide granular per-action approval/policy controls.

## Decision

Implement a separate host binary (`agent-portal-host`) that runs as a systemd user service and listens on a Unix socket for container requests.

An official CLI client (`agent-portal-cli`) will be provided.

### 1. Identity and container attribution

- Use Unix socket peer credentials (`SO_PEERCRED`) to identify the caller process.
- Resolve peer PID + namespace/cgroup metadata back to a Podman container ID.
- Display resolved Podman container identity to the user in approval prompts.

### 2. Protocol and transport

- Use Unix domain sockets.
- Use MessagePack as the on-wire protocol.
- Design protocol to be method-based and extensible for future host capabilities.

### 3. User prompt integration

- Prompting is done via configurable dmenu-style command.
- User can configure a command such as `rofi -dmenu ...`.

### 4. Policy model

- Policy is configurable.
- It must support allowing `clipboard.read_image` without prompting.
- Per-method/per-container policy overrides are supported.

### 5. Initial clipboard scope

- First capability: `clipboard.read_image` only.
- Restrict to image MIME types (allowlist).
- Log request decisions and outcomes.

### 6. Operational safeguards

- Handle multiple containers concurrently.
- Apply concurrency limits.
- Apply per-container and/or global rate limiting.
- Timeouts are configurable; `0` means no timeout.

### 7. Configuration source

- Reuse `~/.agent-box.toml` for portal configuration.
- Use a dedicated namespaced section (e.g. `[portal]`).

### 8. Data transfer strategy

- MVP may return image bytes inline in MessagePack with size limits.
- File descriptor passing (`SCM_RIGHTS`) is acknowledged and can be added later for efficient large payload transfer across the Unix socket boundary.

## Consequences

### Positive

- Fine-grained host capability mediation for containers.
- Better security posture than mounting host display/session sockets.
- User-visible approval flow with container attribution.
- Extensible foundation for future portal methods.

### Trade-offs

- Host daemon becomes a sensitive trust boundary and must be hardened.
- Identity resolution from PID/ns/cgroup to Podman ID must be robust.
- Prompt UX and policy defaults affect safety and usability.

## Follow-up implementation tasks

1. Add `agent-portal-host` binary skeleton and Unix socket server.
2. Implement peer credential extraction and Podman container ID resolution.
3. Define MessagePack request/response schema and versioning.
4. Implement `clipboard.read_image` method with MIME allowlist and size limits.
5. Add prompt adapter with configurable dmenu-style command.
6. Add policy/rate-limit/timeout plumbing sourced from `~/.agent-box.toml`.
7. Add structured audit logging.
8. Add `agent-portal-cli` official CLI client (`ping`, request method, dump output).
