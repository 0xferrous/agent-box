# How to run Portal with a custom wrapper

## Goal

Add a wrapper binary that forwards a host-capability request through Portal.

## Prerequisites

- Running `agent-portal-host`
- Access to socket path (`AGENT_PORTAL_SOCKET` or config default)
- Wrapper implemented in Rust (`wrappers/` convention)

## Steps

1. Define wrapper CLI shape that matches the tool you want to emulate.
2. In wrapper, create a `PortalClient` using env/config resolution.
3. Translate wrapper input into a Portal method request.
4. Forward host response bytes/stdout/stderr to wrapper stdout/stderr.
5. Exit with mapped exit code from portal response.
6. Place wrapper ahead of native utility on PATH in target environment.

## Validation

- Wrapper command succeeds against live `agent-portal-host`.
- Expected output format matches calling tool expectations.
- Failure path returns useful stderr and non-zero code.

## Related

- [Portal wrapper contract](../../reference/portal/wrapper-contract.md)
- [Portal config](../../reference/portal/config.md)
