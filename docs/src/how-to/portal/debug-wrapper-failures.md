# How to debug Portal wrapper failures

## Goal

Find and fix common failures for `wl-paste`/`gh` wrappers and other Portal clients.

## Quick checklist

1. Is host service running?
   - If `[portal].global = true`:
     ```bash
     pgrep -a agent-portal-host
     ```
   - If `[portal].global = false`, `ab spawn` should start one automatically for that session.
2. Can you ping the socket directly?
   ```bash
   agent-portal-cli ping
   ```
3. Is the wrapper using the expected socket path?
   ```bash
   echo "$AGENT_PORTAL_SOCKET"
   ```
4. Enable host logging with `RUST_LOG`:
   ```bash
   RUST_LOG=agent_portal=debug,agent_portal_host=trace agent-portal-host
   ```
   When `agent-portal-host` is run directly, logs are visible in the terminal and also written to the log file. Managed hosts started by `ab spawn` log only to files.
5. Inspect the log file:
   - Log files live under:
     ```text
     ${XDG_STATE_HOME:-$HOME/.local/state}/agent-box/logs/
     ```
   - The log filename matches the socket filename, with `.sock` replaced by `.log`.
   - Example: `portal.sock` -> `portal.log`
   - In managed per-container mode (`[portal].global = false`), each spawned socket gets its own matching log file.
   - Use `RUST_LOG=debug ab spawn ...` if you want more verbose managed-host logs.

## Common failures

- **failed to connect to socket**
  - socket path mismatch or host service not running
- **denied**
  - policy mode blocks method/container
- **prompt_failed**
  - `prompt_command` missing or exits non-zero in ask-mode
- **clipboard_failed**
  - no allowed image MIME currently in clipboard or a host Wayland clipboard access issue
- **gh_exec_failed**
  - host `gh` unavailable or command failure

## Next actions

- Confirm `[portal.policy]` defaults and overrides.
- Confirm wrapper is first on PATH in container.
- Re-run request via `agent-portal-cli` to isolate wrapper-specific parsing issues.
