# How to debug Portal wrapper failures

## Goal

Find and fix common failures for `wl-paste`/`gh` wrappers and other Portal clients.

## Quick checklist

1. Is host service running?
   ```bash
   pgrep -a agent-portal-host
   ```
2. Can you ping the socket directly?
   ```bash
   agent-portal-cli ping
   ```
3. Is the wrapper using the expected socket path?
   ```bash
   echo "$AGENT_PORTAL_SOCKET"
   ```
4. Enable host logs:
   ```bash
   RUST_LOG=debug agent-portal-host
   ```

## Common failures

- **failed to connect to socket**
  - socket path mismatch or host service not running
- **denied**
  - policy mode blocks method/container
- **prompt_failed**
  - `prompt_command` missing or exits non-zero in ask-mode
- **clipboard_failed**
  - no allowed image MIME currently in clipboard or host `wl-paste` issue
- **gh_exec_failed**
  - host `gh` unavailable or command failure

## Next actions

- Confirm `[portal.policy]` defaults and overrides.
- Confirm wrapper is first on PATH in container.
- Re-run request via `agent-portal-cli` to isolate wrapper-specific parsing issues.
