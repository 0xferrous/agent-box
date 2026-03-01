# Portal architecture overview

Portal is a host-side mediation layer for selected capabilities that should not be exposed directly to containers.

## Core model

- `agent-portal-host` listens on a Unix socket.
- Clients send MessagePack requests with method payloads.
- Host resolves caller identity from peer credentials.
- Host applies policy (default + per-container overrides).
- Host performs allowed operation and returns structured response.

## Why this model

Direct host socket passthrough is broad and hard to audit. Portal centralizes control, policy, and observability behind method-level requests.

## Implemented methods

- `ping`
- `whoami`
- `clipboard.read_image`
- `gh.exec`

## Important constraints

- Prompting happens host-side, never in container wrappers.
- Timeouts and rate limiting are explicit configuration concerns.
- Host binary resolution avoids recursive wrapper execution.
