# ADR 0002: Transparent Portal Access via Wrapper Binaries

- Status: Accepted
- Date: 2026-02-27

## Context

Portal-based host capability access is safer than direct host socket passthrough, but requires callers to know and integrate a custom API.

Many agent tools (e.g. `pi`) already call standard host utilities directly (such as `wl-paste`). Requiring every agent/tool to learn portal APIs is not transparent and hurts adoption.

## Decision

Provide transparent wrapper binaries that mimic standard utility behavior while internally using the portal socket API.

Initial wrapper:

- `wl-paste` wrapper using portal `clipboard.read_image`.

Implementation structure:

- New top-level `wrappers/` crate for compatibility binaries.
- Shared Rust portal client API in `common` (`portal_client`) for direct Rust consumers.
- A generic helper CLI in wrappers (`agent-portal-client`) for scripts and debugging.

## Scope (MVP)

The wrapper supports the Wayland usage pattern required by `pi` image paste flow:

1. `wl-paste --list-types`
2. `wl-paste --type <selected-mime-type> --no-newline`

Behavior:

- `--list-types`: returns the currently available clipboard image MIME type exposed by portal.
- `--type ... --no-newline`: returns raw image bytes from portal, enforcing MIME match when requested.

## Consequences

### Positive

- Agent/tools remain unaware of portal internals.
- Drop-in compatibility improves usability.
- Single mediation point (portal) keeps policy/security controls centralized.

### Trade-offs

- Wrapper compatibility must track behavior of real utilities.
- Some utility flags may not be supported initially.
- Extra binary indirection adds maintenance surface.

## Follow-up

- Expand wrapper flag compatibility as needed.
- Add more wrappers for other host capability entry points.
- Consider packaging wrappers on PATH ahead of native tools inside container images.
