# Glossary

- **Agent-box**: CLI (`ab`) that manages repository workspaces and container sessions.
- **Portal**: host daemon + API/CLI for mediated host capability access.
- **Wrapper**: compatibility binary that presents familiar command behavior while forwarding to Portal.
- **Session**: named workspace instance used by Agent-box (`ab new`/`ab spawn`).
- **Workspace type**: JJ workspace or Git worktree.
- **Policy decision**: allow/ask/deny behavior for portal methods.
- **Policy mode (`gh_exec`)**: ask_for_writes, ask_for_all, ask_for_none, deny_all.
- **Prompt command**: configurable dmenu-style command invoked by portal host for ask-mode approvals.
- **Container override policy**: per-container policy table keyed by container ID.
